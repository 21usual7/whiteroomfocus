use crate::{Db, Item, Res, e};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub const REVIEW_BYTES: usize = 12 * 1024;
pub const MATH_BYTES: usize = 28 * 1024;
pub const REVIEW_OUTPUT: u64 = 2048;
pub const MATH_OUTPUT: u64 = 6144;
const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_ATTACHMENTS: usize = 4;
const FRAGMENT_BYTES: usize = 900;

pub struct SessionSources {
    pub topic: Option<String>,
    pub windows: String,
    pub pages: Vec<SourceExcerpt>,
}

pub struct SourceExcerpt {
    pub name: String,
    pub text: String,
}

impl SourceExcerpt {
    pub(crate) fn append_page_log(&self, output: &mut String) {
        output.push_str(&format!("── {}\n{}\n\n", self.name, self.text));
    }
}

pub struct PreparedRequest {
    pub body: Value,
    pub bytes: usize,
    pub omitted: usize,
}

pub fn read_attachments(paths: &[&str]) -> Res<Vec<SourceExcerpt>> {
    if paths.len() > MAX_ATTACHMENTS {
        return Err(format!(
            "Можно приложить до {MAX_ATTACHMENTS} файлов на запрос."
        ));
    }
    paths
        .iter()
        .map(|path| {
            let path = Path::new(path);
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("Нужно указать файл с текстовым именем.")?;
            let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            if !extension.eq_ignore_ascii_case("txt") && !extension.eq_ignore_ascii_case("md") {
                return Err(format!("{name}: поддерживаются .txt и .md."));
            }
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)
                .map_err(|error| format!("{name}: {error}"))?;
            if !file
                .metadata()
                .map_err(|error| error.to_string())?
                .is_file()
            {
                return Err(format!("{name}: нужен обычный файл."));
            }
            let mut bytes = Vec::new();
            file.take((MAX_FILE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("{name}: {error}"))?;
            if bytes.len() > MAX_FILE_BYTES {
                return Err(format!(
                    "{name}: размер превышает {} КиБ.",
                    MAX_FILE_BYTES / 1024
                ));
            }
            let text = String::from_utf8(bytes).map_err(|_| format!("{name}: ожидается UTF-8."))?;
            if text.trim().is_empty() || text.contains('\0') {
                return Err(format!("{name}: нужен непустой текст UTF-8."));
            }
            Ok(SourceExcerpt {
                name: name.into(),
                text,
            })
        })
        .collect()
}

pub fn prefix(text: &str, bytes: usize) -> &str {
    let mut end = bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn fragments(text: &str) -> impl Iterator<Item = &str> {
    text.split("\n\n").flat_map(|paragraph| {
        let mut rest = paragraph.trim();
        std::iter::from_fn(move || {
            if rest.is_empty() {
                return None;
            }
            let candidate = prefix(rest, FRAGMENT_BYTES);
            let end = if candidate.len() == rest.len() {
                candidate.len()
            } else {
                candidate
                    .rfind('\n')
                    .filter(|&index| index >= FRAGMENT_BYTES / 2)
                    .or_else(|| {
                        candidate
                            .rfind(char::is_whitespace)
                            .filter(|&index| index >= FRAGMENT_BYTES / 2)
                    })
                    .unwrap_or(candidate.len())
            };
            let (part, tail) = rest.split_at(end);
            rest = tail.trim_start();
            Some(part)
        })
    })
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.chars().count() >= 3)
        .map(str::to_lowercase)
}

pub fn prepare(
    sources: &SessionSources,
    attachments: &[SourceExcerpt],
    max_bytes: usize,
    max_output: u64,
    make_body: impl Fn(&str) -> Value,
) -> Res<PreparedRequest> {
    let (duration, window_lines) = sources
        .windows
        .split_once('\n')
        .unwrap_or((&sources.windows, ""));
    let topic = sources.topic.as_deref().unwrap_or_default();
    let topic_words: HashSet<_> = words(topic).collect();
    let mut activity = String::new();
    if !duration.is_empty() {
        activity.push_str(duration);
        activity.push('\n');
    }
    let sources = attachments
        .iter()
        .map(|file| {
            (
                format!("Файл {}", prefix(&file.name, 160)),
                file.text.as_str(),
            )
        })
        .chain(sources.pages.iter().map(|page| {
            (
                format!("Страница {}", prefix(&page.name, 300)),
                page.text.as_str(),
            )
        }))
        .chain([("Окна".to_owned(), window_lines)]);
    let mut seen = HashSet::new();
    let mut ranked = Vec::new();
    let mut omitted = 0;
    for (label, text) in sources {
        for fragment in fragments(text) {
            let normalized = fragment.split_whitespace().collect::<Vec<_>>().join(" ");
            if !seen.insert(normalized) {
                omitted += 1;
                continue;
            }
            let matches: HashSet<_> = words(fragment)
                .filter(|word| topic_words.contains(word))
                .collect();
            ranked.push((matches.len(), format!("\n[{label}]\n{fragment}\n")));
        }
    }
    if ranked.is_empty() {
        return Err("Нет записанной активности или приложенного текста для разбора.".into());
    }
    ranked.sort_by_key(|(relevance, _)| std::cmp::Reverse(*relevance));

    let encode = |text: &str| -> Res<(Value, usize)> {
        let mut body = make_body(text);
        body["max_tokens"] = max_output.into();
        let size = serde_json::to_vec(&body)
            .map_err(|error| error.to_string())?
            .len();
        Ok((body, size))
    };
    let (mut body, mut bytes) = encode(&activity)?;
    let mut included = 0;
    for (_, fragment) in ranked {
        let previous = activity.len();
        activity.push_str(&fragment);
        let (candidate, size) = encode(&activity)?;
        if size <= max_bytes {
            body = candidate;
            bytes = size;
            included += 1;
        } else {
            activity.truncate(previous);
            omitted += 1;
        }
    }
    if included == 0 {
        return Err("Лимита входного запроса недостаточно для одного фрагмента.".into());
    }
    Ok(PreparedRequest {
        body,
        bytes,
        omitted,
    })
}

/// Явно переданный выделенный текст: те же ограничения, что у файла .txt/.md.
pub fn selected_text(text: &str) -> Res<SourceExcerpt> {
    if text.trim().is_empty() || text.contains('\0') {
        return Err("Выделенный текст пуст.".into());
    }
    if text.len() > MAX_FILE_BYTES {
        return Err(format!(
            "Выделенный текст больше {} КиБ.",
            MAX_FILE_BYTES / 1024
        ));
    }
    Ok(SourceExcerpt {
        name: "выделенный текст".into(),
        text: text.into(),
    })
}

// ───────────── Разбор сессии (D17): вопросы Claude сначала на просмотр ─────────────

pub const SOURCE_UNKNOWN: &str = "связь с источником неизвестна";

/// Черновики вопросов Claude. status: pending → accepted | rejected; accepted → disputed.
pub(crate) fn migrate(c: &rusqlite::Connection) -> Res<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS ai_draft(id INTEGER PRIMARY KEY, study_id INT, created INT NOT NULL,
           status TEXT NOT NULL DEFAULT 'pending', data TEXT NOT NULL, source TEXT, item_id TEXT, note TEXT);",
    )
    .map_err(e)
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiDraft {
    pub id: i64,
    pub session_id: Option<i64>,
    pub status: String,
    pub prompt: String,
    pub kind: String,
    pub answers: Vec<String>,
    pub hint: Option<String>,
    pub explain: String,
    /// Дословный фрагмент отправленного материала, на который ссылается вопрос.
    pub source: Option<String>,
    pub source_label: String,
    pub item_id: Option<String>,
}

#[derive(Deserialize)]
pub struct DraftEdit {
    pub prompt: String,
    pub kind: String,
    pub answers: Vec<String>,
    #[serde(default)]
    pub hint: Option<String>,
    pub explain: String,
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Db {
    /// Последняя или указанная завершённая сессия и только её записи (D4).
    pub fn finished_session_sources(&self, id: Option<i64>) -> Res<(i64, SessionSources)> {
        let (id, start, end, topic): (i64, i64, i64, String) = self
            .c
            .query_row(
                "SELECT id, started, ended, topic FROM study
                 WHERE ended IS NOT NULL AND (?1 IS NULL OR id=?1) ORDER BY id DESC LIMIT 1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(e)?
            .ok_or("Нет завершённой учебной сессии для разбора.")?;
        Ok((
            id,
            SessionSources {
                topic: Some(topic),
                windows: self.focus_range(start, end, Some(id))?,
                pages: self.page_excerpts(start, end, Some(id))?,
            },
        ))
    }

    /// Ответ Claude → черновики на просмотр. В повторения ничего не попадает до принятия.
    /// Ссылка на источник сохраняется, только если цитата дословно есть в отправленном запросе.
    pub fn stage_review(
        &self,
        study_id: Option<i64>,
        body: &Value,
        response: &Value,
        now: i64,
    ) -> Res<(String, usize)> {
        self.help_allowed()?;
        let (summary, items) = crate::ai_parse(response, now)?;
        let sent = squash(body["messages"][0]["content"].as_str().unwrap_or_default());
        let raw: Vec<Value> = response["content"]
            .as_array()
            .and_then(|c| c.iter().rev().find(|b| b["type"] == "text"))
            .and_then(|b| b["text"].as_str())
            .and_then(|t| serde_json::from_str::<Value>(t).ok())
            .and_then(|v| v["questions"].as_array().cloned())
            .unwrap_or_default();
        let tx = self.c.unchecked_transaction().map_err(e)?;
        for it in &items {
            let source = raw
                .iter()
                .find(|q| q["prompt"] == it.prompt.as_str())
                .and_then(|q| q["source"].as_str())
                .map(squash)
                .filter(|quote| quote.chars().count() >= 8 && sent.contains(quote.as_str()));
            self.c
                .execute(
                    "INSERT INTO ai_draft(study_id, created, data, source) VALUES (?,?,?,?)",
                    params![study_id, now, serde_json::to_string(it).map_err(e)?, source],
                )
                .map_err(e)?;
        }
        tx.commit().map_err(e)?;
        Ok((summary, items.len()))
    }

    /// Some(сессия) — все черновики этой сессии; None — все, ждущие решения.
    pub fn ai_drafts(&self, session_id: Option<i64>) -> Res<Vec<AiDraft>> {
        self.help_allowed()?;
        let mut st = self
            .c
            .prepare(
                "SELECT id, study_id, status, data, source, item_id FROM ai_draft
                 WHERE (?1 IS NULL AND status='pending') OR study_id=?1 ORDER BY id",
            )
            .map_err(e)?;
        let rows: Vec<(
            i64,
            Option<i64>,
            String,
            String,
            Option<String>,
            Option<String>,
        )> = st
            .query_map([session_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?;
        rows.into_iter()
            .map(|(id, session_id, status, data, source, item_id)| {
                let it: Item = serde_json::from_str(&data).map_err(e)?;
                Ok(AiDraft {
                    id,
                    session_id,
                    status,
                    prompt: it.prompt,
                    kind: it.kind,
                    answers: it.answer,
                    hint: it.hint,
                    explain: it.explain,
                    source_label: source
                        .as_ref()
                        .map_or(SOURCE_UNKNOWN.into(), |s| format!("«{s}»")),
                    source,
                    item_id,
                })
            })
            .collect()
    }

    fn pending_item(&self, id: i64) -> Res<Item> {
        let data: String = self
            .c
            .query_row(
                "SELECT data FROM ai_draft WHERE id=? AND status='pending'",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(e)?
            .ok_or("Черновик уже принят, отклонён или не найден.")?;
        serde_json::from_str(&data).map_err(e)
    }

    /// Правка до добавления: те же правила, что для ответа Claude.
    pub fn ai_draft_edit(&self, id: i64, edit: &DraftEdit) -> Res<()> {
        let mut it = self.pending_item(id)?;
        let answers: Vec<String> = edit
            .answers
            .iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect();
        if edit.prompt.trim().is_empty() || answers.is_empty() {
            return Err("Нужны вопрос и хотя бы один ответ.".into());
        }
        match edit.kind.as_str() {
            "text" => {}
            "number" if answers.iter().all(|a| crate::parse_num(a).is_some()) => {}
            "number" => return Err("Для числового вопроса ответ должен быть числом.".into()),
            _ => return Err("Тип вопроса: number или text.".into()),
        }
        it.prompt = edit.prompt.trim().into();
        it.kind = edit.kind.clone();
        it.answer = answers;
        it.hint = edit.hint.clone().filter(|h| !h.trim().is_empty());
        it.explain = edit.explain.trim().into();
        self.c
            .execute(
                "UPDATE ai_draft SET data=? WHERE id=? AND status='pending'",
                params![serde_json::to_string(&it).map_err(e)?, id],
            )
            .map_err(e)?;
        Ok(())
    }

    pub fn ai_draft_reject(&self, id: i64) -> Res<()> {
        self.pending_item(id)?;
        self.c
            .execute(
                "UPDATE ai_draft SET status='rejected' WHERE id=? AND status='pending'",
                [id],
            )
            .map_err(e)?;
        Ok(())
    }

    /// Принятый вопрос — материал практики (цель `ai`), не форма сравнительного пилота.
    pub fn ai_draft_accept(&self, id: i64, now: i64) -> Res<String> {
        let tx = self.c.unchecked_transaction().map_err(e)?;
        let mut it = self.pending_item(id)?;
        it.id = format!("ai-d{id}");
        it.objective = "ai".into();
        it.role = "practice".into();
        self.add_ai_items(std::slice::from_ref(&it), now)?;
        self.c
            .execute(
                "UPDATE ai_draft SET status='accepted', item_id=? WHERE id=?",
                params![it.id, id],
            )
            .map_err(e)?;
        tx.commit().map_err(e)?;
        Ok(it.id)
    }

    /// «Сообщить об ошибке»: вопрос уходит из повторений и учебной оценки, попытки остаются в истории.
    pub fn ai_item_dispute(&self, item_id: &str, note: &str, now: i64) -> Res<()> {
        let tx = self.c.unchecked_transaction().map_err(e)?;
        self.c
            .execute(
                "INSERT OR IGNORE INTO objective(id, title, arm, assigned_at)
                 VALUES ('ai-disputed', 'Спорные вопросы ИИ', 'disputed', ?)",
                [now],
            )
            .map_err(e)?;
        let n = self
            .c
            .execute(
                "UPDATE item SET objective='ai-disputed',
                   data=json_set(data, '$.objective', 'ai-disputed') WHERE id=? AND objective='ai'",
                [item_id],
            )
            .map_err(e)?;
        if n == 0 {
            return Err(format!("{item_id}: это не принятый вопрос Claude."));
        }
        self.c
            .execute("DELETE FROM card WHERE item_id=?", [item_id])
            .map_err(e)?;
        self.c
            .execute(
                "UPDATE ai_draft SET status='disputed', note=? WHERE item_id=?",
                params![note.trim(), item_id],
            )
            .map_err(e)?;
        tx.commit().map_err(e)
    }
}

/// Терминальный просмотр черновиков: Enter — добавить, «-» — отклонить, «=ответ» — исправить ключ.
pub fn run_drafts(db: &Db, inp: &mut impl BufRead, out: &mut impl Write, now: i64) -> Res<()> {
    let drafts = db.ai_drafts(None)?;
    if drafts.is_empty() {
        writeln!(out, "Вопросов на просмотре нет.").map_err(e)?;
        return Ok(());
    }
    for (n, d) in drafts.iter().enumerate() {
        writeln!(
            out,
            "\n── {}/{} Вопрос: {}\nКлюч: {}\nОбъяснение: {}\nИсточник: {}",
            n + 1,
            drafts.len(),
            d.prompt,
            d.answers.join(" | "),
            d.explain,
            d.source_label
        )
        .map_err(e)?;
        loop {
            let Some(a) = crate::ask(
                inp,
                out,
                "Enter — добавить в повторения, «-» — отклонить, «=ответ» — исправить ключ: ",
            )?
            else {
                writeln!(out, "\nОстальные вопросы ждут: whiteroom drafts").map_err(e)?;
                return Ok(());
            };
            if a == "-" {
                db.ai_draft_reject(d.id)?;
                writeln!(out, "Отклонён.").map_err(e)?;
            } else if a.is_empty() || a.starts_with('=') {
                if let Some(key) = a.strip_prefix('=') {
                    let edit = DraftEdit {
                        prompt: d.prompt.clone(),
                        kind: d.kind.clone(),
                        answers: vec![key.into()],
                        hint: d.hint.clone(),
                        explain: d.explain.clone(),
                    };
                    if let Err(x) = db.ai_draft_edit(d.id, &edit) {
                        writeln!(out, "{x}").map_err(e)?;
                        continue;
                    }
                }
                let item = db.ai_draft_accept(d.id, now)?;
                writeln!(
                    out,
                    "Добавлен {item}. Ошибка в вопросе: whiteroom dispute {item} <что не так>"
                )
                .map_err(e)?;
            } else {
                continue;
            }
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_json_budget_includes_escaped_content_and_preserves_relevant_sources() {
        let sources = SessionSources {
            topic: Some("квадратное уравнение".into()),
            windows: "Всего 4 мин\nРедактор\n".into(),
            pages: vec![SourceExcerpt {
                name: "study.example".into(),
                text: "квадратное уравнение: x² + 3x + 2 = 0\n\nповтор\n\nповтор".into(),
            }],
        };
        let file = SourceExcerpt {
            name: "задачи.md".into(),
            text: format!(
                "квадратное уравнение: дискриминант {{\"D\": 1}}\n\n{}",
                "\\\"".repeat(5000)
            ),
        };
        let draft = prepare(
            &sources,
            &[file],
            800,
            200,
            |text| serde_json::json!({"system": "Инструкция".repeat(6), "activity": text}),
        )
        .unwrap();
        let activity = draft.body["activity"].as_str().unwrap();
        assert!(activity.contains("Файл задачи.md"));
        assert!(activity.contains("Страница study.example"));
        assert!(activity.contains("x² + 3x + 2 = 0"));
        assert_eq!(activity.matches("повтор").count(), 1);
        assert!(draft.omitted > 1);
        assert_eq!(draft.bytes, serde_json::to_vec(&draft.body).unwrap().len());
        assert!(draft.bytes <= 800);
        assert_eq!(draft.body["max_tokens"], 200);
    }
}
