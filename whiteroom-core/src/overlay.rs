use std::env;
use std::fs;
use std::io::{BufRead, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::tutor::{self, Tutor};
use crate::{Db, Res};

pub fn start(cli: &Path, id: i64, started: i64) -> Res<()> {
    if env::var_os("WAYLAND_DISPLAY").is_none() {
        return Ok(());
    }
    let config = install(Some(id))?;
    ensure_window(cli, &config, Some((id, started)), "ping")
}

pub fn open(path: &str, cli: &Path) -> Res<()> {
    if env::var_os("WAYLAND_DISPLAY").is_none() {
        return Err("Всплывающий тьютор доступен в Wayland.".into());
    }
    let db = Db::open(path)?;
    db.help_allowed()?;
    let session = db.study_active()?.map(|(id, started, _)| (id, started));
    let config = install(session.map(|(id, _)| id))?;
    ensure_window(cli, &config, session, "open")
}

fn call_window(path: &Path, action: &str) -> bool {
    quickshell()
        .arg("ipc")
        .arg("--path")
        .arg(path)
        .args(["call", "tutor", action])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn ensure_window(cli: &Path, config: &Path, session: Option<(i64, i64)>, action: &str) -> Res<()> {
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(config.with_extension("lock"))
        .map_err(crate::e)?;
    lock.lock().map_err(crate::e)?;
    if call_window(config, action) {
        return Ok(());
    }
    launch(cli, config, session)?;
    for _ in 0..24 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if call_window(config, action) {
            return Ok(());
        }
    }
    Err("Окно WHITE ROOM не запустилось в Wayland.".into())
}

fn install(session_id: Option<i64>) -> Res<PathBuf> {
    let directory =
        PathBuf::from(env::var_os("XDG_RUNTIME_DIR").ok_or("Не задан XDG_RUNTIME_DIR.")?)
            .join("whiteroom");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)
        .map_err(|err| err.to_string())?;
    let config = directory.join(match session_id {
        Some(id) => format!("timer-{id}.qml"),
        None => "tutor.qml".to_owned(),
    });
    write_config(
        &directory.join("TutorPopup.qml"),
        include_bytes!("TutorPopup.qml"),
    )?;
    write_config(&config, include_bytes!("timer.qml"))?;
    Ok(config)
}

fn write_config(path: &Path, contents: &[u8]) -> Res<()> {
    if fs::read(path).ok().as_deref() != Some(contents) {
        let pending = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&pending, contents)
            .and_then(|()| fs::rename(pending, path))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn quickshell() -> Command {
    let mut command = Command::new("quickshell");
    command
        .env_remove("QS_CONFIG_NAME")
        .env_remove("QS_MANIFEST")
        .env("QT_QPA_PLATFORM", "wayland");
    command
}

fn launch(cli: &Path, path: &Path, session: Option<(i64, i64)>) -> Res<()> {
    if !cli.is_file() {
        return Err("Для таймера Wayland собери CLI: cargo build -p whiteroom-core".into());
    }
    let mut child = quickshell()
        .arg("--no-duplicate")
        .arg("--path")
        .arg(path)
        .env("WHITEROOM_TIMER_CLI", cli)
        .env(
            "WHITEROOM_TIMER_ID",
            session.map_or_else(|| "idle".to_owned(), |(id, _)| id.to_string()),
        )
        .env(
            "WHITEROOM_TIMER_STARTED",
            session.map_or(0, |(_, started)| started).to_string(),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|err| format!("Не запустилось окно Wayland: {err}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn watch(path: &str, session_id: Option<i64>) -> Res<()> {
    let db = Db::open(path)?;
    let mut warned = std::collections::HashMap::new();
    let mut tick = 0u32;
    // «Не беспокоить» Omarchy: запоминаем прежнее состояние и возвращаем его после сессии.
    let muted = session_id.is_some_and(|id| db.study_muted(id).unwrap_or(false));
    let was_dnd = muted && dnd("isDnd").as_deref() == Some("on");
    if muted && !was_dnd {
        dnd("setDnd on");
    }
    while db.study_active()?.map(|session| session.0) == session_id {
        if session_id.is_some() && tick % 7 == 0 {
            guard_apps(&db, &mut warned);
        }
        tick = tick.wrapping_add(1);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    if muted && !was_dnd {
        dnd("setDnd off");
    }
    Ok(())
}

fn dnd(command: &str) -> Option<String> {
    let out = Command::new("omarchy-shell")
        .arg("notifications")
        .args(command.split(' '))
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

const APP_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Окна из списка блокировки: предупреждение, через 10 секунд закрываем окно и завершаем процесс.
/// shortcut: приложение, свёрнутое в трей без окна, не видно; когда понадобится — искать по имени процесса.
fn guard_apps(db: &Db, warned: &mut std::collections::HashMap<i64, std::time::Instant>) {
    let Ok(apps) = db.blocked_apps() else { return };
    let clients = Command::new("hyprctl")
        .args(["clients", "-j"])
        .stderr(Stdio::null())
        .output()
        .ok()
        .and_then(|out| serde_json::from_slice::<Value>(&out.stdout).ok());
    let mut seen = Vec::new();
    for client in clients.iter().filter_map(Value::as_array).flatten() {
        let class = client["class"].as_str().unwrap_or_default().to_lowercase();
        let (Some(pid), true) = (
            client["pid"].as_i64().filter(|pid| *pid > 1),
            apps.iter().any(|app| class.contains(app.as_str())),
        ) else {
            continue;
        };
        seen.push(pid);
        let since = *warned.entry(pid).or_insert_with(|| {
            let _ = Command::new("notify-send")
                .args(["-u", "critical", "-t", "10000", "WHITE ROOM"])
                .arg(format!("«{class}» закроется через 10 секунд: идёт учебная сессия."))
                .status();
            std::time::Instant::now()
        });
        if since.elapsed() >= APP_GRACE {
            let _ = Command::new("hyprctl")
                .args(["dispatch", "closewindow"])
                .arg(format!("address:{}", client["address"].as_str().unwrap_or_default()))
                .stdout(Stdio::null())
                .status();
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
        }
    }
    warned.retain(|pid, _| seen.contains(pid));
}

#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Request {
    Context,
    Prepare {
        question: Option<String>,
        concept_id: Option<String>,
        #[serde(default)]
        claude: bool,
    },
    Send {
        preview_id: String,
    },
    Cancel {
        preview_id: String,
    },
}

pub fn tutor_stdio(path: &str, session_id: Option<i64>) -> Res<()> {
    let db = Db::open(path)?;
    let tutor = Tutor::new();
    let mut output = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(crate::e)?;
        let result = serde_json::from_str::<Request>(&line)
            .map_err(crate::e)
            .and_then(|request| match request {
                Request::Context => {
                    let context = tutor::context(&db)?;
                    if context.session_id != session_id {
                        return Err("Сессия изменилась. Открой тьютора заново.".into());
                    }
                    serde_json::to_value(context).map_err(crate::e)
                }
                Request::Prepare {
                    question,
                    claude: true,
                    ..
                } => {
                    let preview = tutor.prepare_claude(
                        &db,
                        session_id,
                        question.as_deref().unwrap_or_default(),
                    )?;
                    serde_json::to_value(preview).map_err(crate::e)
                }
                Request::Prepare {
                    question,
                    concept_id,
                    claude: false,
                } => {
                    let preview = tutor.prepare(
                        &db,
                        session_id,
                        question.as_deref(),
                        concept_id.as_deref(),
                    )?;
                    serde_json::to_value(preview).map_err(crate::e)
                }
                Request::Send { preview_id } => {
                    let answer = tutor.send(&db, &preview_id, tutor::request_claude)?;
                    serde_json::to_value(answer).map_err(crate::e)
                }
                Request::Cancel { preview_id } => {
                    tutor.cancel(&preview_id)?;
                    Ok(Value::Null)
                }
            });
        let response = match result {
            Ok(value) => json!({"value":value}),
            Err(message) => json!({"error":message}),
        };
        serde_json::to_writer(&mut output, &response).map_err(crate::e)?;
        writeln!(output)
            .and_then(|()| output.flush())
            .map_err(crate::e)?;
    }
    Ok(())
}
