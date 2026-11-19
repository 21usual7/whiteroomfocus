use crate::Res;
use serde_json::Value;
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Read;
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
