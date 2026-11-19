use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::{InvokeResponseBody, IpcResponse};
use tauri::utils::acl::Value;
use whiteroom_core::{Db, Res};

static STUDY: Mutex<()> = Mutex::new(());

fn open_db() -> Res<Db> {
    if let Ok(path) = std::env::var("WHITEROOM_DB") {
        return Db::open(&path);
    }
    let dir = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .map_err(|_| "не задан HOME")?
        .join("whiteroom");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Db::open(&dir.join("whiteroom.db").to_string_lossy())
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
async fn focus_history() -> Res<String> {
    json(open_db()?.focus_history()?)
}

#[tauri::command]
async fn study_start(
    topic: String,
    mode: String,
    direction: String,
    windows: bool,
    browser: bool,
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
        let tracking = !windows || whiteroom_core::tracker_start(&cli, id);
        json((
            started,
            if topic.is_empty() { &direction } else { topic },
            tracking,
            (id, windows, browser),
        ))
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
        db.focus_range(started, ended, Some(id))
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn review_queue() -> Res<String> {
    let db = open_db()?;
    let queue: Vec<_> = db
        .due_reviews(now())?
        .into_iter()
        .map(|item| (item, "review"))
        .chain(db.new_items(8)?.into_iter().map(|item| (item, "new")))
        .map(|(item, role)| (item.id, role, item.prompt, item.code, item.hint))
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
async fn blocked() -> Res<String> {
    json(open_db()?.blocked()?)
}

#[tauri::command]
async fn block_add(domain: String) -> Res<String> {
    open_db()?.block_add(&domain)?;
    Ok(String::new())
}

#[tauri::command]
async fn block_rm(domain: String) -> Res<String> {
    if !open_db()?.block_rm(&domain)? {
        return Err(format!("«{domain}» не в списке"));
    }
    Ok(String::new())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            status,
            focus_catalog,
            focus_history,
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
            blocked,
            block_add,
            block_rm
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить WHITE ROOM");
}
