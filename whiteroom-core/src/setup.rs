//! Сетап рабочих столов Hyprland для направления фокус-сессии.
//! Конфиг — `~/.config/whiteroom/setups/<режим>/<направление>.toml`, правится вручную.

use crate::Res;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

const TEMPLATE: &str = r#"# Сетап рабочих столов для «{title}».
# Применяется при входе в фокус-сессию с этим направлением.
# Пока ниже нет ни одного [[window]] и close_others = false, ничего не происходит.

# Закрыть все прочие окна (кроме WHITE ROOM) после открытия новых.
# Окна закрываются штатно: редактор с несохранённым файлом спросит подтверждение.
close_others = false

# app: terminal | tmux | browser | notion | webapp | command
#
# [[window]]
# workspace = 2
# app = "tmux"
# session = "{mode}"            # tmux new-session -A: подключится к уже живой сессии
#
# [[window]]
# workspace = 1
# app = "browser"               # пустое окно браузера
# urls = []                     # или ["https://developer.mozilla.org"]
#
# [[window]]
# workspace = 4
# app = "notion"
#
# [[window]]
# workspace = 3
# app = "command"
# command = "obsidian"
"#;

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    #[serde(default)]
    pub close_others: bool,
    #[serde(default)]
    pub window: Vec<Window>,
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Window {
    pub workspace: u32,
    pub app: String,
    pub session: Option<String>,
    #[serde(default)]
    pub urls: Vec<String>,
    pub command: Option<String>,
}

fn quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

impl Window {
    /// Команда для `hyprctl dispatch exec`; её разбирает оболочка, поэтому аргументы в кавычках.
    fn command(&self) -> Res<String> {
        let urls = || {
            if let Some(bad) = self
                .urls
                .iter()
                .find(|url| !url.starts_with("https://") && !url.starts_with("http://"))
            {
                return Err(format!("Адрес должен начинаться с https://: {bad}"));
            }
            Ok(self.urls.iter().map(|url| quote(url)).collect::<Vec<_>>())
        };
        Ok(match self.app.as_str() {
            "terminal" => "xdg-terminal-exec".into(),
            "tmux" => format!(
                "xdg-terminal-exec tmux new-session -A -s {}",
                quote(self.session.as_deref().unwrap_or("whiteroom"))
            ),
            "browser" => format!("omarchy-launch-browser --new-window {}", urls()?.join(" ")),
            "notion" => "omarchy-launch-webapp https://www.notion.so".into(),
            "webapp" => format!(
                "omarchy-launch-webapp {}",
                urls()?
                    .first()
                    .ok_or("Для webapp укажи urls = [\"https://…\"]")?
            ),
            "command" => self
                .command
                .clone()
                .filter(|command| !command.trim().is_empty())
                .ok_or("Для app = \"command\" укажи command = \"…\"")?,
            other => return Err(format!("Неизвестное приложение «{other}» в сетапе")),
        }
        .trim_end()
        .to_string())
    }
}

pub fn path(mode: &str, direction: &str) -> Res<PathBuf> {
    if !crate::focus::MODES
        .iter()
        .any(|m| m.id == mode && m.options.contains(&direction))
    {
        return Err("Неизвестный режим или направление".into());
    }
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map_err(|_| "не задан HOME")?;
    Ok(dir
        .join("whiteroom/setups")
        .join(mode)
        .join(format!("{direction}.toml")))
}

/// Создаёт конфиг-шаблон, если его ещё нет, и возвращает путь.
pub fn ensure(mode: &str, direction: &str) -> Res<PathBuf> {
    let path = path(mode, direction)?;
    if !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|err| err.to_string())?;
        let text = TEMPLATE
            .replace("{title}", direction)
            .replace("{mode}", mode);
        std::fs::write(&path, text).map_err(|err| err.to_string())?;
    }
    Ok(path)
}

pub fn load(mode: &str, direction: &str) -> Res<Setup> {
    let path = ensure(mode, direction)?;
    let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
    toml::from_str(&text).map_err(|err| format!("Ошибка в {}: {err}", path.display()))
}

fn hyprctl(args: &[&str]) -> Res<String> {
    let out = Command::new("hyprctl")
        .args(args)
        .output()
        .map_err(|err| format!("hyprctl недоступен: {err}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if text.starts_with("error") {
        return Err(format!(
            "hyprctl: {}",
            text.lines().next().unwrap_or_default()
        ));
    }
    Ok(text)
}

/// Диспетчер в Lua-синтаксисе Hyprland 0.56+ (конфиг Omarchy на Lua).
/// shortcut: старый синтаксис `dispatch exec …` не поддержан, понадобится — добавить ветку.
fn dispatch(lua: &str) -> Res<()> {
    hyprctl(&["dispatch", lua]).map(|_| ())
}

/// Адреса окон, кроме окон процесса `keep_pid` и спецворкспейсов (скретчпады).
fn clients(keep_pid: u32) -> Res<Vec<String>> {
    let list: Value = serde_json::from_str(&hyprctl(&["clients", "-j"])?)
        .map_err(|_| "hyprctl вернул не JSON")?;
    Ok(list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["pid"].as_u64() != Some(keep_pid as u64))
        .filter(|c| c["workspace"]["id"].as_i64().is_some_and(|id| id > 0))
        .filter_map(|c| c["address"].as_str().map(String::from))
        .collect())
}

/// Открывает окна сетапа по одному и переносит каждое на его рабочий стол,
/// затем при `close_others` закрывает окна, бывшие до запуска.
/// Перенос по адресу нового окна, а не правилом exec: браузер отдаёт окно
/// уже запущенному процессу с другим pid.
pub fn apply(mode: &str, direction: &str, keep_pid: u32) -> Res<String> {
    let setup = load(mode, direction)?;
    if setup.window.is_empty() && !setup.close_others {
        return Ok(String::new());
    }
    let commands = setup
        .window
        .iter()
        .map(|w| w.command().map(|c| (w.workspace, c)))
        .collect::<Res<Vec<_>>>()?;
    // Лаунчеры браузера переводят фокус на своё окно — в конце вернём пользователя на исходный стол.
    let home: Value = serde_json::from_str(&hyprctl(&["activeworkspace", "-j"])?)
        .map_err(|_| "hyprctl вернул не JSON")?;
    let old = clients(keep_pid)?;
    // shortcut: новым окном считается первое появившееся; чужое окно в эти секунды перепутает перенос.
    let mut known = old.clone();
    let mut missed = Vec::new();
    for (workspace, command) in commands {
        // Длинная скобка Lua: команда передаётся как есть, без экранирования кавычек.
        if command.contains("]==]") {
            return Err(format!("Недопустимая команда в сетапе: {command}"));
        }
        dispatch(&format!("hl.dsp.exec_cmd([==[{command}]==])"))?;
        let started = Instant::now();
        let fresh = loop {
            std::thread::sleep(Duration::from_millis(200));
            let now = clients(keep_pid)?;
            if let Some(new) = now.iter().find(|a| !known.contains(a)) {
                break Some(new.clone());
            }
            if started.elapsed() > Duration::from_secs(8) {
                break None;
            }
        };
        match fresh {
            Some(address) => {
                dispatch(&format!(
                    "hl.dsp.window.move({{ workspace = \"{workspace}\", follow = false, window = \"address:{address}\" }})"
                ))?;
                known.push(address);
            }
            None => missed.push(command),
        }
    }
    if setup.close_others {
        for address in &old {
            dispatch(&format!(
                "hl.dsp.window.close({{ window = \"address:{address}\" }})"
            ))?;
        }
    }
    if let Some(id) = home["id"].as_i64() {
        dispatch(&format!("hl.dsp.focus({{ workspace = \"{id}\" }})"))?;
    }
    Ok(if missed.is_empty() {
        String::new()
    } else {
        format!("Сетап: окно не появилось за 8 с — {}", missed.join("; "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_is_a_no_op_and_presets_build_quoted_commands() {
        let empty: Setup =
            toml::from_str(&TEMPLATE.replace("{title}", "X").replace("{mode}", "ctf")).unwrap();
        assert_eq!(
            empty,
            Setup {
                close_others: false,
                window: vec![]
            }
        );

        let setup: Setup = toml::from_str(
            r#"close_others = true
            [[window]]
            workspace = 2
            app = "tmux"
            session = "a'b"
            [[window]]
            workspace = 1
            app = "browser"
            [[window]]
            workspace = 3
            app = "browser"
            urls = ["https://x.org/?q=1&r=2"]
            [[window]]
            workspace = 4
            app = "notion""#,
        )
        .unwrap();
        let commands: Vec<_> = setup.window.iter().map(|w| w.command().unwrap()).collect();
        assert_eq!(
            commands,
            [
                r"xdg-terminal-exec tmux new-session -A -s 'a'\''b'",
                "omarchy-launch-browser --new-window",
                "omarchy-launch-browser --new-window 'https://x.org/?q=1&r=2'",
                "omarchy-launch-webapp https://www.notion.so",
            ]
        );

        let bad = |text: &str| {
            toml::from_str::<Setup>(text)
                .map_err(|e| e.to_string())
                .and_then(|s| s.window[0].command())
                .is_err()
        };
        assert!(bad(
            "[[window]]\nworkspace = 1\napp = \"browser\"\nurls = [\"file:///etc\"]"
        ));
        assert!(bad("[[window]]\nworkspace = 1\napp = \"vim\""));
        assert!(bad("[[window]]\nworkspace = 1\napp = \"command\""));
        assert!(bad(
            "[[window]]\nworkspace = 1\napp = \"terminal\"\ntypo = 1"
        ));
        assert!(path("ctf", "../../etc").is_err());
    }
}
