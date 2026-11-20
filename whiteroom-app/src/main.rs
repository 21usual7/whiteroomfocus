use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::{InvokeResponseBody, IpcResponse};
use tauri::utils::acl::Value;
use whiteroom_core::tutor::Tutor;
use whiteroom_core::{Db, Res};

static STUDY: Mutex<()> = Mutex::new(());
static TUTOR: Tutor = Tutor::new();
static REVIEW: whiteroom_core::ReviewDraft = whiteroom_core::ReviewDraft::new();

fn db_path() -> Res<String> {
    if let Ok(path) = std::env::var("WHITEROOM_DB") {
        return Ok(path);
    }
    let dir = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .map_err(|_| "не задан HOME")?
        .join("whiteroom");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(dir.join("whiteroom.db").to_string_lossy().into())
}

fn open_db() -> Res<Db> {
    Db::open(&db_path()?)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

fn json(value: impl IpcResponse) -> Res<String> {
    // Используем сериализацию Tauri, чтобы не менять Cargo.lock рабочего пространства.
    match value.body().map_err(|err| err.to_string())? {
        InvokeResponseBody::Json(value) => Ok(value),
        InvokeResponseBody::Raw(_) => Err("Ожидался ответ в формате JSON".into()),
    }
}

#[tauri::command]
async fn status() -> Res<String> {
    open_db()?.status(now())
}

#[tauri::command]
async fn study_active() -> Res<String> {
    json(open_db()?.active_focus()?)
}

#[tauri::command]
async fn focus_catalog() -> Res<String> {
    json(whiteroom_core::focus::MODES)
}

#[tauri::command]
async fn month_activity() -> Res<String> {
    json(open_db()?.month_activity(now())?)
}

#[tauri::command]
async fn focus_history() -> Res<String> {
    json(open_db()?.focus_history()?)
}

#[tauri::command]
async fn tutor_context() -> Res<String> {
    json(whiteroom_core::tutor::context(&open_db()?)?)
}

#[tauri::command]
async fn tutor_prepare(
    session_id: Option<i64>,
    question: Option<String>,
    concept_id: Option<String>,
    claude: Option<bool>,
) -> Res<String> {
    if claude == Some(true) {
        let db = open_db()?;
        return json(TUTOR.prepare_claude(
            &db,
            session_id,
            question.as_deref().unwrap_or_default(),
        )?);
    }
    json(TUTOR.prepare(
        &open_db()?,
        session_id,
        question.as_deref(),
        concept_id.as_deref(),
    )?)
}

#[tauri::command]
async fn tutor_send(preview_id: String) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let db = open_db()?;
        json(TUTOR.send(&db, &preview_id, whiteroom_core::tutor::request_claude)?)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn tutor_cancel(preview_id: String) -> Res<()> {
    TUTOR.cancel(&preview_id)
}

#[tauri::command]
async fn briefing(offset: i64) -> Res<String> {
    json(open_db()?.briefing(now(), offset)?)
}

#[tauri::command]
async fn study_start(
    topic: String,
    mode: String,
    direction: String,
    windows: bool,
    browser: bool,
    estimate: i64,
    mute: bool,
) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _study = STUDY.lock().map_err(|err| err.to_string())?;
        let topic = topic.trim();
        let cli = std::env::current_exe()
            .map_err(|err| err.to_string())?
            .with_file_name("whiteroom");
        if windows && !cli.is_file() {
            return Err("Собери CLI: cargo build -p whiteroom-core".into());
        }
        let started = now();
        let id = open_db()?
            .focus_start_with_sources(&mode, &direction, topic, started, windows, browser)?;
        open_db()?.study_estimate_set(id, estimate)?;
        open_db()?.study_mute_set(id, mute)?;
        let tracking = !windows || whiteroom_core::tracker_start(&cli, id);
        let timer_error = whiteroom_core::overlay::start(&cli, id, started).err();
        json((
            started,
            if topic.is_empty() { &direction } else { topic },
            tracking,
            (id, windows, browser),
            timer_error,
        ))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn setup_open(mode: String, direction: String) -> Res<String> {
    let path = whiteroom_core::setup::ensure(&mode, &direction)?;
    std::process::Command::new("omarchy-launch-editor")
        .arg(&path)
        .spawn()
        .or_else(|_| std::process::Command::new("xdg-open").arg(&path).spawn())
        .map_err(|err| format!("Не удалось открыть {}: {err}", path.display()))?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn setup_get(mode: String, direction: String) -> Res<String> {
    json(whiteroom_core::setup::load(&mode, &direction)?)
}

#[tauri::command]
async fn setup_save(mode: String, direction: String, setup: whiteroom_core::setup::Setup) -> Res<()> {
    whiteroom_core::setup::save(&mode, &direction, &setup).map(|_| ())
}

#[tauri::command]
async fn setup_apply(mode: String, direction: String) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        whiteroom_core::setup::apply(&mode, &direction, std::process::id())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn revoke_capture(id: i64, source: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || {
        open_db()?.revoke_capture(id, &source)?;
        if source == "windows" {
            whiteroom_core::tracker_stop(id);
        }
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn study_stop(id: i64) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _study = STUDY.lock().map_err(|err| err.to_string())?;
        let db = open_db()?;
        let ended = now();
        let (_, started, _) = db.stop_session(Some(id), ended)?;
        whiteroom_core::tracker_stop(id);
        Ok(db.estimate_line(id, started, ended)? + &db.focus_range(started, ended, Some(id))?)
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Разбор конкретной завершённой сессии; files — содержимое выбранных .txt/.md (имя, текст).
#[tauri::command]
async fn review_prepare(
    session_id: Option<i64>,
    text: Option<String>,
    files: Option<Vec<(String, String)>>,
) -> Res<String> {
    let files = files.unwrap_or_default();
    if files.len() > 4 {
        return Err("Можно приложить до 4 файлов на запрос.".into());
    }
    let mut attachments = Vec::new();
    for (name, body) in files {
        let lower = name.to_lowercase();
        if !lower.ends_with(".txt") && !lower.ends_with(".md") {
            return Err(format!("{name}: поддерживаются .txt и .md."));
        }
        let mut file = whiteroom_core::ai_context::selected_text(&body)
            .map_err(|_| format!("{name}: нужен непустой текст до 256 КиБ."))?;
        file.name = name;
        attachments.push(file);
    }
    if let Some(text) = text.filter(|text| !text.trim().is_empty()) {
        attachments.push(whiteroom_core::ai_context::selected_text(&text)?);
    }
    json(REVIEW.prepare_for(&open_db()?, session_id, &attachments)?)
}

#[tauri::command]
async fn review_drafts(session_id: Option<i64>) -> Res<String> {
    json(open_db()?.ai_drafts(session_id)?)
}

#[tauri::command]
async fn draft_edit(id: i64, edit: whiteroom_core::ai_context::DraftEdit) -> Res<()> {
    open_db()?.ai_draft_edit(id, &edit)
}

#[tauri::command]
async fn draft_reject(id: i64) -> Res<()> {
    open_db()?.ai_draft_reject(id)
}

#[tauri::command]
async fn draft_accept(id: i64) -> Res<String> {
    open_db()?.ai_draft_accept(id, now())
}

#[tauri::command]
async fn item_dispute(item_id: String, note: String) -> Res<()> {
    open_db()?.ai_item_dispute(&item_id, &note, now())
}

#[tauri::command]
async fn self_test_active() -> Res<bool> {
    open_db()?.self_test_active()
}

/// Следующий вопрос недельного блока; каждый вызов записывается как показ.
#[tauri::command]
async fn test_current() -> Res<String> {
    json(open_db()?.test_current(now())?)
}

#[tauri::command]
async fn test_submit(block: i64, item_id: String, answer: String, ms: i64) -> Res<String> {
    json(open_db()?.test_submit(block, &item_id, &answer, ms, now())?)
}

#[tauri::command]
async fn pilot_text() -> Res<String> {
    open_db()?.pilot_text(now())
}

#[tauri::command]
async fn review_send(preview_id: String) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || {
        json(REVIEW.send(
            &open_db()?,
            &preview_id,
            now(),
            whiteroom_core::tutor::request_claude,
        )?)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn review_cancel() -> Res<()> {
    REVIEW.cancel()
}

#[tauri::command]
async fn review_queue() -> Res<String> {
    let db = open_db()?;
    db.help_allowed()?;
    let queue: Vec<_> = db
        .due_reviews(now())?
        .into_iter()
        .map(|item| (item, "review"))
        .chain(db.new_items(8)?.into_iter().map(|item| (item, "new")))
        .chain(db.new_ai_items()?.into_iter().map(|item| (item, "new")))
        .map(|(item, role)| {
            let claude = item.objective == "ai";
            (item.id, role, item.prompt, item.code, item.hint, claude)
        })
        .collect();
    json(queue)
}

#[tauri::command]
async fn session_begin(planned: i64, predicted: i64) -> Res<String> {
    if planned < 1 || !(0..=planned).contains(&predicted) {
        return Err("Введи число от 0 до количества вопросов".into());
    }
    Ok(open_db()?
        .session_begin(now(), planned, predicted)?
        .to_string())
}

#[tauri::command]
async fn answer(
    session_id: String,
    item_id: String,
    role: String,
    answer: String,
    hint: bool,
    ms: i64,
) -> Res<String> {
    if !matches!(role.as_str(), "review" | "new" | "retry") {
        return Err("Неизвестная роль вопроса".into());
    }
    let session_id = session_id.parse::<i64>().map_err(|err| err.to_string())?;
    let db = open_db()?;
    let item = db.item(&item_id)?;
    let answer = answer.trim();
    let ok = whiteroom_core::grade(&item, answer);
    db.record(
        Some(session_id),
        &item,
        &role,
        answer,
        ok,
        role != "retry" && hint && item.hint.is_some(),
        ms,
        now(),
    )?;
    json(Value::Map(
        [
            ("ok".into(), ok.into()),
            (
                "answer".into(),
                item.answer.into_iter().next().unwrap_or_default().into(),
            ),
            ("explain".into(), item.explain.into()),
        ]
        .into(),
    ))
}

#[tauri::command]
async fn session_end(id: String, correct: i64) -> Res<String> {
    let id = id.parse::<i64>().map_err(|err| err.to_string())?;
    open_db()?.session_end(id, correct, now())?;
    Ok(String::new())
}

#[tauri::command]
async fn challenges() -> Res<String> {
    let rows: Vec<_> = open_db()?
        .challenges()?
        .into_iter()
        .map(|row| {
            let ch = row.ch;
            let status = if row.solved {
                "решён"
            } else if row.attempts > 0 {
                "в работе"
            } else {
                "новый"
            };
            (
                row.id,
                ch.title,
                ch.kind,
                ch.topic,
                ch.difficulty,
                status,
                ch.statement,
            )
        })
        .collect();
    json(rows)
}

#[tauri::command]
async fn next_hint(id: i64) -> Res<String> {
    let (level, hint) = open_db()?.next_hint(id)?;
    Ok(format!("Подсказка {level}/4. {hint}"))
}

#[tauri::command]
async fn solve(id: i64, answer: String) -> Res<String> {
    tauri::async_runtime::spawn_blocking(move || json(open_db()?.solve(id, &answer, now())?))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn export_files(id: i64) -> Res<String> {
    let dir = PathBuf::from(std::env::var("HOME").map_err(|_| "не задан HOME")?)
        .join("whiteroom/challenges")
        .join(id.to_string());
    open_db()?.export_files(id, &dir)?;
    Ok(dir.display().to_string())
}

#[tauri::command]
async fn blocked_sites() -> Res<String> {
    json(open_db()?.blocked_sites()?)
}

#[tauri::command]
async fn site_block(domain: String) -> Res<()> {
    open_db()?.site_block(&domain)
}

#[tauri::command]
async fn site_unblock(domain: String) -> Res<()> {
    open_db()?.site_unblock(&domain)
}

#[tauri::command]
async fn blocked_apps() -> Res<String> {
    json(open_db()?.blocked_apps()?)
}

#[tauri::command]
async fn app_block(name: String) -> Res<()> {
    open_db()?.app_block(&name)
}

#[tauri::command]
async fn app_unblock(name: String) -> Res<()> {
    open_db()?.app_unblock(&name)
}


// ── WHITE ROOM 1.0: «Сегодня», цели, учебный блок, прогресс, данные, материалы (whiteroom_core::learn) ──

/// Пример для чистой базы: модуль по анализу журналов встроен в приложение и работает офлайн.
const EXAMPLE: &str = include_str!("../../modules/log-analysis.toml");

#[tauri::command]
async fn today(offset: i64) -> Res<String> {
    Ok(open_db()?.today(now(), offset)?.to_string())
}

#[tauri::command]
async fn goals() -> Res<String> {
    Ok(open_db()?.goals()?.to_string())
}

#[tauri::command]
async fn goal_example() -> Res<String> {
    let m = whiteroom_core::Module::parse(EXAMPLE)?;
    Ok(open_db()?.goal_create(&m, now())?.to_string())
}

#[tauri::command]
async fn goal_from_module(id: String) -> Res<String> {
    let db = open_db()?;
    let m = db.module_src(&id)?;
    Ok(db.goal_create(&m, now())?.to_string())
}

#[tauri::command]
async fn goal_update(id: i64, edit: whiteroom_core::learn::GoalEdit) -> Res<()> {
    open_db()?.goal_update(id, &edit, now())
}

#[tauri::command]
async fn goal_archive(id: i64) -> Res<()> {
    open_db()?.goal_archive(id, now())
}

#[tauri::command]
async fn prereq_set(skill: String, needs: Vec<String>) -> Res<()> {
    open_db()?.prereq_set(&skill, &needs)
}

#[tauri::command]
async fn plan_preview(goal: i64) -> Res<String> {
    json(serde_plan(&open_db()?.plan_build(goal, now())?)?)
}

fn serde_plan(p: &[whiteroom_core::learn::Step]) -> Res<Value> {
    Ok(Value::List(
        p.iter()
            .map(|s| {
                Value::Map(
                    [
                        ("kind".into(), s.kind.clone().into()),
                        ("item".into(), s.item.clone().map_or(Value::Null, Into::into)),
                        ("min".into(), (s.min as f64).into()),
                        ("why".into(), s.why.clone().into()),
                    ]
                    .into(),
                )
            })
            .collect(),
    ))
}

#[tauri::command]
async fn block_start(goal: i64, predicted: Option<i64>) -> Res<String> {
    Ok(open_db()?.block_start(goal, predicted, now())?.to_string())
}

#[tauri::command]
async fn block_open() -> Res<Option<i64>> {
    open_db()?.block_open()
}

#[tauri::command]
async fn block_state(id: i64) -> Res<String> {
    Ok(open_db()?.block_state(id)?.to_string())
}

#[tauri::command]
async fn block_plan_set(id: i64, plan: Vec<whiteroom_core::learn::Step>) -> Res<()> {
    open_db()?.block_plan_set(id, &plan, now())
}

#[tauri::command]
async fn block_answer(id: i64, step: i64, answer: String, rating: Option<String>, ms: i64) -> Res<String> {
    Ok(open_db()?.block_answer(id, step, &answer, rating.as_deref(), ms, now())?.to_string())
}

#[tauri::command]
async fn block_hint(id: i64, step: i64) -> Res<String> {
    open_db()?.block_hint(id, step)
}

#[tauri::command]
async fn block_reveal(id: i64, step: i64) -> Res<String> {
    Ok(open_db()?.block_reveal(id, step)?.to_string())
}

#[tauri::command]
async fn block_retry(id: i64, step: i64, answer: String, ms: i64) -> Res<bool> {
    open_db()?.block_retry(id, step, &answer, ms, now())
}

#[tauri::command]
async fn block_skip(id: i64, step: i64) -> Res<()> {
    open_db()?.block_skip(id, step, now())
}

#[tauri::command]
async fn block_note(id: i64, note: String) -> Res<()> {
    open_db()?.block_note(id, &note, now())
}

#[tauri::command]
async fn block_thought(id: i64, text: String) -> Res<()> {
    open_db()?.block_thought(id, &text, now())
}

#[tauri::command]
async fn block_close(id: i64, energy: Option<i64>, reflection: Option<String>) -> Res<String> {
    Ok(open_db()?.block_close(id, energy, reflection.as_deref(), now())?.to_string())
}

#[tauri::command]
async fn block_summary(id: i64) -> Res<String> {
    Ok(open_db()?.block_summary(id)?.to_string())
}

#[tauri::command]
async fn spread_reviews(days: i64) -> Res<i64> {
    open_db()?.spread_reviews(days, now())
}

#[tauri::command]
async fn progress() -> Res<String> {
    Ok(open_db()?.progress(now())?.to_string())
}

#[tauri::command]
async fn seen() -> Res<String> {
    Ok(open_db()?.seen(&db_path()?)?.to_string())
}

/// Очистка записей окон и страниц сессии (`session = null` — записи без сессии); `apply = false` — только счёт.
#[tauri::command]
async fn activity_clear(session: Option<i64>, apply: bool) -> Res<String> {
    Ok(open_db()?.activity_clear(session, apply)?.to_string())
}

/// Копия рядом с базой: `<db>.<время>.bak`. Путь возвращается для показа.
#[tauri::command]
async fn backup() -> Res<String> {
    let dest = format!("{}.{}.bak", db_path()?, now());
    open_db()?.backup(&dest)?;
    Ok(dest)
}

#[tauri::command]
async fn modules() -> Res<String> {
    Ok(open_db()?.modules()?.to_string())
}

#[tauri::command]
async fn module_get(id: String) -> Res<whiteroom_core::Module> {
    open_db()?.module_src(&id)
}

#[tauri::command]
async fn module_preview(module: whiteroom_core::Module) -> Res<String> {
    let mut m = module;
    whiteroom_core::Module::check(&mut m)?;
    Ok(open_db()?.module_preview(&m)?.to_string())
}

#[tauri::command]
async fn module_save(module: whiteroom_core::Module) -> Res<()> {
    let mut m = module;
    whiteroom_core::Module::check(&mut m)?;
    open_db()?.module_put(&m, now())
}

#[tauri::command]
async fn module_undo(id: String) -> Res<()> {
    open_db()?.module_undo(&id, now())
}

/// Разбор файла модуля (TOML) для редактора; в базу ничего не пишется.
#[tauri::command]
async fn module_parse(text: String) -> Res<whiteroom_core::Module> {
    whiteroom_core::Module::parse(&text)
}

#[tauri::command]
async fn anki_import(tsv: String, id: String, title: String) -> Res<whiteroom_core::Module> {
    whiteroom_core::learn::anki_import(&tsv, &id, &title)
}

#[tauri::command]
async fn anki_export(id: String) -> Res<String> {
    Ok(whiteroom_core::learn::anki_export(&open_db()?.module_src(&id)?))
}

/// Версии окна, ядра, схемы и соседнего CLI. Разные версии окна и CLI — понятная ошибка в интерфейсе.
#[tauri::command]
async fn versions() -> Res<String> {
    let cli = std::env::current_exe()
        .map_err(|err| err.to_string())?
        .with_file_name("whiteroom");
    let cli_v = std::process::Command::new(&cli)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let app = env!("CARGO_PKG_VERSION");
    let ok = cli_v.as_deref() == Some(app);
    json(Value::Map(
        [
            ("app".into(), app.to_string().into()),
            ("schema".into(), (whiteroom_core::SCHEMA as f64).into()),
            ("cli".into(), cli_v.clone().map_or(Value::Null, Into::into)),
            ("match".into(), ok.into()),
            (
                "message".into(),
                if ok {
                    String::new()
                } else {
                    format!(
                        "CLI рядом с окном ({}) — версия {}, окно — {app}. Учёба в окне работает; запуск записи и таймера может не совпасть. Переустанови обе программы из одного архива.",
                        cli.display(),
                        cli_v.as_deref().unwrap_or("не найдена")
                    )
                }
                .into(),
            ),
        ]
        .into(),
    ))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            status,
            focus_catalog,
            focus_history,
            tutor_context,
            tutor_prepare,
            tutor_send,
            tutor_cancel,
            setup_open,
            setup_get,
            setup_save,
            setup_apply,
            review_prepare,
            review_drafts,
            draft_edit,
            draft_reject,
            draft_accept,
            item_dispute,
            self_test_active,
            test_current,
            test_submit,
            pilot_text,
            month_activity,
            review_send,
            review_cancel,
            briefing,
            study_active,
            study_start,
            study_stop,
            revoke_capture,
            review_queue,
            session_begin,
            answer,
            session_end,
            challenges,
            next_hint,
            solve,
            export_files,
            blocked_sites,
            site_block,
            site_unblock,
            blocked_apps,
            app_block,
            app_unblock,
            today,
            goals,
            goal_example,
            goal_from_module,
            goal_update,
            goal_archive,
            prereq_set,
            plan_preview,
            block_start,
            block_open,
            block_state,
            block_plan_set,
            block_answer,
            block_hint,
            block_reveal,
            block_retry,
            block_skip,
            block_note,
            block_thought,
            block_close,
            block_summary,
            spread_reviews,
            progress,
            seen,
            activity_clear,
            backup,
            modules,
            module_get,
            module_preview,
            module_save,
            module_undo,
            module_parse,
            anki_import,
            anki_export,
            versions
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить WHITE ROOM");
}
