use std::io::{BufRead, BufReader, Read, Write, stdin, stdout};
use std::os::unix::net::UnixStream;
use std::time::{SystemTime, UNIX_EPOCH};
use whiteroom_core::challenge::Challenge;
use whiteroom_core::sandbox::{DEFAULT, Limits};
use whiteroom_core::{Db, Module, Res, ai_call, ai_parse, ai_request, read_native, run_review, run_session, run_test, write_native};

const USAGE: &str = "whiteroom — пилот WHITE ROOM

  init <module.toml>        загрузить модуль, случайно разделить цели на группы
  session [новых]           сессия: повторения + новые вопросы (по умолчанию 8)
  done <цель> <минут>       цель изучена; минут — сколько учил её вне WHITE ROOM
  test                      отложенные проверки, срок которых наступил
  status                    цели, группы, сроки проверок
  export                    все данные в JSON (stdout)
  track                     записывать активное окно Hyprland (работает до Ctrl+C)
  log [часов]               что ты делал за последние часы (по умолчанию 4)

Сессия:
  start [--windows] [--browser] [тема]   начать с выбранными источниками
  stop                      закончить и показать сводку
  block [add|rm <домен>]    сайты, блокируемые во время сессии
  review [часов]            ИИ задаёт 2–3 вопроса по последней сессии (нужен ANTHROPIC_API_KEY)
  browser-setup             связать whiteroom с расширением в Chromium

Практика:
  challenge add <файл.json> проверить челлендж в песочнице и добавить
  challenges                список челленджей
  show <id>                 условие; для pwn — файлы в ~/whiteroom/challenges/<id>
  hint <id>                 следующая подсказка: идея → метод → первый шаг → решение
  solve <id> <ответ|флаг>   сдать ответ (математика — выражение, например 3*sqrt(2)/4)
  pwn run <id>              запустить бинарь в песочнице (pwntools: process(['whiteroom','pwn','run','<id>']))
  pwn gdb <id>              gdb в песочнице
  practice <pwn|math> [ч]   агент составит задачу по теме последней сессии (нужен ANTHROPIC_API_KEY)

База: $WHITEROOM_DB или ~/.local/share/whiteroom/whiteroom.db";

fn db_path() -> Res<String> {
    if let Ok(p) = std::env::var("WHITEROOM_DB") {
        return Ok(p);
    }
    let dir = std::env::var("XDG_DATA_HOME")
        .map(Into::into)
        .or_else(|_| std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))
        .map_err(|_| "не задан HOME")?
        .join("whiteroom");
    std::fs::create_dir_all(&dir).map_err(|x| x.to_string())?;
    Ok(dir.join("whiteroom.db").to_string_lossy().into())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Активное окно в формате события socket2: `activewindow>>class,title`.
fn active_window(dir: &std::path::Path) -> Res<String> {
    let mut q = UnixStream::connect(dir.join(".socket.sock")).map_err(|x| x.to_string())?;
    q.write_all(b"j/activewindow").map_err(|x| x.to_string())?;
    let mut js = String::new();
    q.read_to_string(&mut js).map_err(|x| x.to_string())?;
    let w: serde_json::Value = serde_json::from_str(&js).unwrap_or_default();
    let field = |k: &str| w[k].as_str().unwrap_or("").to_string();
    Ok(format!("activewindow>>{},{}", field("class"), field("title")))
}

/// Слушает сокет событий Hyprland и пишет отрезки фокуса в базу. Всё хранится только локально.
fn track(path: &str, requested: Option<i64>) -> Res<()> {
    let db = Db::open(path)?;
    let Some((id, true, _)) = db.capture_state()? else {
        return Err("Запись окон выключена.".into());
    };
    if requested.is_some_and(|x| x != id) {
        return Err("Сессия уже завершена.".into());
    }
    let dir = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").map_err(|_| "нет XDG_RUNTIME_DIR")?)
        .join("hypr")
        .join(std::env::var("HYPRLAND_INSTANCE_SIGNATURE").map_err(|_| "Hyprland не запущен (нет HYPRLAND_INSTANCE_SIGNATURE)")?);
    // Текущее окно: события придут только при следующем переключении.
    db.focus_event(id, &active_window(&dir)?, now())?;
    let (idle_path, idle_dir) = (path.to_string(), dir.clone());
    std::thread::spawn(move || {
        let Ok(db) = Db::open(&idle_path) else { return };
        // shortcut: 5 минут без ввода — простой; долгое чтение без прокрутки тоже попадёт сюда. Настройку — когда будет GUI.
        let r = whiteroom_core::idle::watch(5 * 60 * 1000, |idle| {
            if !matches!(db.capture_state(), Ok(Some((active,true,_))) if active == id) {
                return;
            }
            let line = if idle {
                format!("activewindow>>{},", whiteroom_core::IDLE)
            } else {
                active_window(&idle_dir).unwrap_or_default()
            };
            let _ = db.focus_event(id, &line, now());
        });
        if let Err(x) = r {
            eprintln!("Простой не учитывается: {x}");
        }
    });
    let tick_path = path.to_string();
    std::thread::spawn(move || {
        let Ok(db) = Db::open(&tick_path) else { return };
        loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let _ = db.focus_tick(id, now());
        }
    });
    println!("Записываю активное окно (только локально). Ctrl+C — остановить.");
    let events = UnixStream::connect(dir.join(".socket2.sock")).map_err(|x| x.to_string())?;
    for line in BufReader::new(events).lines() {
        if !matches!(db.capture_state()?, Some((active,true,_)) if active == id) {
            break;
        }
        db.focus_event(id, &line.map_err(|x| x.to_string())?, now())?;
    }
    Err("Hyprland закрыл сокет событий".into())
}

fn confirm_request(
    body: &serde_json::Value,
    input: &mut impl BufRead,
    output: &mut impl Write,
    send: impl FnOnce(&serde_json::Value) -> Res<serde_json::Value>,
) -> Res<Option<serde_json::Value>> {
    writeln!(
        output,
        "Запрос в Claude API:\n{}",
        serde_json::to_string_pretty(body).map_err(|e| e.to_string())?
    )
    .map_err(|e| e.to_string())?;
    write!(output, "Отправить? [y/N] ").map_err(|e| e.to_string())?;
    output.flush().map_err(|e| e.to_string())?;
    let mut yes = String::new();
    input.read_line(&mut yes).map_err(|e| e.to_string())?;
    if yes.trim().eq_ignore_ascii_case("y") {
        send(body).map(Some)
    } else {
        Ok(None)
    }
}

fn confirmed_call(body: &serde_json::Value) -> Res<Option<serde_json::Value>> {
    confirm_request(body, &mut stdin().lock(), &mut stdout().lock(), |request| {
        let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| "Нет ANTHROPIC_API_KEY.")?;
        ai_call(&key, request)
    })
}

/// Агент составляет задачу по теме сессии; не прошедшая проверку в песочнице перегенерируется с текстом ошибки.
fn practice(kind: &str, hours: Option<i64>, now: i64) -> Res<()> {
    let db = Db::open(&db_path()?)?;
    let activity = db.review_activity(hours, now)?;
    let done: Vec<String> = db.challenges()?.into_iter().map(|r| r.ch.title).collect();
    let mut prev: Option<(String, String)> = None;
    for attempt in 1..=3 {
        println!("Составляю задачу (попытка {attempt}/3)…");
        let req =
            whiteroom_core::challenge::ai_request(&activity, kind, &done, prev.as_ref().map(|(a, b)| (a.as_str(), b.as_str())));
        let Some(response) = confirmed_call(&req)? else {
            return Ok(());
        };
        let (text, ch) = whiteroom_core::challenge::from_response(&response)?;
        println!("Проверяю в песочнице…");
        match ch.validate() {
            Ok(ready) => {
                let id = db.add_challenge(&ready, now)?;
                println!("Готово: челлендж {id}. Условие: whiteroom show {id}");
                return Ok(());
            }
            Err(x) => {
                println!("Не прошла проверку: {x}");
                prev = Some((text, x));
            }
        }
    }
    Err("За 3 попытки не получилось составить проверяемую задачу.".into())
}

fn start(topic: &str, now: i64, windows: bool, browser: bool) -> Res<()> {
    let db = Db::open(&db_path()?)?;
    let id = db.study_start_with_sources(topic, now, windows, browser)?;
    println!("Сессия «{topic}» началась.");
    if windows && !whiteroom_core::tracker_start(&std::env::current_exe().map_err(|x| x.to_string())?, id) {
        println!("Трекер окон не запустился.");
    }
    let block = db.blocked()?;
    if !block.is_empty() {
        println!("В Chromium заблокированы (в течение 30 с): {}", block.join(", "));
    }
    println!("Закончить: whiteroom stop");
    Ok(())
}

fn stop(now: i64) -> Res<()> {
    let db = Db::open(&db_path()?)?;
    let (id, started, topic) = db.stop_session(None, now)?;
    whiteroom_core::tracker_stop(id);
    println!(
        "Сессия «{topic}» закончена: {} мин.\n\n{}",
        (now - started) / 60,
        db.focus_range(started, now, Some(id))?
    );
    println!("Дальше: whiteroom review — вопросы по сессии, whiteroom practice pwn|math — задача по теме.");
    Ok(())
}

fn review(hours: Option<i64>, now: i64) -> Res<()> {
    let db = Db::open(&db_path()?)?;
    let activity = db.review_activity(hours, now)?;
    println!("Составляю вопросы…");
    let Some(response) = confirmed_call(&ai_request(&activity))? else {
        return Ok(());
    };
    let (summary, items) = ai_parse(&response, now)?;
    run_review(&db, &mut stdin().lock(), &mut stdout(), now, &summary, &items)
}

const HOST: &str = "com.whiteroom.host";
// ID следует из ключа "key" в extension/manifest.json.
const EXTENSION_ID: &str = "loeenilmfolklcifabeaeaedhnoffdca";

/// Режим native messaging: Chromium запускает нас с origin расширения первым аргументом.
fn native() -> Res<()> {
    let db = Db::open(&db_path()?)?;
    let (mut inp, mut out) = (stdin().lock(), stdout().lock());
    let mut site_grant: Option<(i64, String)> = None;
    while let Some(msg) = read_native(&mut inp)? {
        let res = match msg["type"].as_str() {
            Some("state") => Ok(()),
            Some("site_permission") => {
                site_grant = None;
                match (db.capture_state()?, msg["domain"].as_str()) {
                    (Some((id, _, true)), Some(domain)) if msg["capture"].as_i64() == Some(id) => {
                        site_grant = Some((id, domain.to_owned()));
                        Ok(())
                    }
                    _ => Err("Запись вкладок выключена.".into()),
                }
            }
            _ => {
                let grant = site_grant.take().filter(|(id, _)| Some(*id) == msg["capture"].as_i64());
                db.page_event(&msg, grant.as_ref().map(|(_, s)| s.as_str()), now())
            }
        };
        let mut r = db.browser_state()?;
        if msg["type"] == "state" {
            r["request"] = msg["request"].clone();
        }
        r["ok"] = res.is_ok().into();
        if let Err(x) = res {
            r["error"] = x.into();
        }
        write_native(&mut out, &r)?;
    }
    Ok(())
}

fn browser_setup() -> Res<()> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|x| x.to_string())?;
    let home = std::env::var("HOME").map_err(|_| "не задан HOME")?;
    let dir = std::path::PathBuf::from(home).join(".config/chromium/NativeMessagingHosts");
    std::fs::create_dir_all(&dir).map_err(|x| x.to_string())?;
    let manifest = serde_json::json!({
        "name": HOST, "description": "WHITE ROOM", "type": "stdio",
        "path": exe, "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")]
    });
    let file = dir.join(format!("{HOST}.json"));
    std::fs::write(&file, serde_json::to_string_pretty(&manifest).map_err(|x| x.to_string())?).map_err(|x| x.to_string())?;
    println!("Записал {}\nТеперь chromium://extensions → «Режим разработчика» → «Загрузить распакованное» → папка extension/.", file.display());
    Ok(())
}

fn challenge_dir(id: i64) -> Res<std::path::PathBuf> {
    let home = std::env::var("HOME").map_err(|_| "не задан HOME")?;
    Ok(std::path::PathBuf::from(home).join("whiteroom/challenges").join(id.to_string()))
}

fn id(s: &str) -> Res<i64> {
    s.parse().map_err(|_| format!("id — число, а не «{s}»"))
}

fn pwn(id: i64, gdb: bool) -> Res<()> {
    let job = Db::open(&db_path()?)?.pwn_job(id)?;
    let cmd = if gdb { "cd /work/out && exec gdb -q ./chal" } else { "cd /work/out && exec ./chal" };
    let lim = Limits { secs: if gdb { 3600 } else { 600 }, ..DEFAULT };
    let code = whiteroom_core::sandbox::run_interactive(&job, &["sh", "-c", cmd], lim)?;
    std::process::exit(code);
}

fn run() -> Res<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let now = now();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    if a.first().is_some_and(|x| x.starts_with("chrome-extension://")) {
        return native();
    }
    let db = || Db::open(&db_path()?);
    match a.as_slice() {
        ["init", path] => {
            let src = std::fs::read_to_string(path).map_err(|x| format!("{path}: {x}"))?;
            let m = Module::parse(&src)?;
            println!("Модуль «{}»: {} вопросов.", m.title, m.item.len());
            for (id, arm) in db()?.load(&m, now)? {
                println!("  {id:<10} → {}", if arm == "wr" { "WHITE ROOM" } else { "учишь сам" });
            }
            println!("Цели «учишь сам» изучай своим обычным способом, по тем же темам.");
        }
        ["session"] => run_session(&db()?, &mut stdin().lock(), &mut stdout(), now, 8)?,
        ["session", n] => run_session(&db()?, &mut stdin().lock(), &mut stdout(), now, n.parse().map_err(|_| USAGE)?)?,
        ["done", obj, min] => {
            db()?.done(obj, min.parse().map_err(|_| "минут — целое число")?, now)?;
            println!("Отмечено. Проверка по «{obj}» откроется через 7 дней.");
        }
        ["test"] => run_test(&db()?, &mut stdin().lock(), &mut stdout(), now)?,
        ["status"] => print!("{}", db()?.status(now)?),
        ["export"] => println!("{}", serde_json::to_string_pretty(&db()?.export()?).map_err(|x| x.to_string())?),
        ["track"] => track(&db_path()?, None)?,
        ["track", session] => track(&db_path()?, Some(id(session)?))?,
        ["log"] => print!("{}", db()?.focus_log(now - 4 * 3600)?),
        ["log", h] => print!("{}", db()?.focus_log(now - h.parse::<i64>().map_err(|_| USAGE)? * 3600)?),
        ["browser-setup"] => browser_setup()?,
        ["challenge", "add", path] => {
            let src = std::fs::read_to_string(path).map_err(|x| format!("{path}: {x}"))?;
            let ch: Challenge = serde_json::from_str(&src).map_err(|x| format!("{path}: {x}"))?;
            println!("Проверяю в песочнице: компиляция и эталонное решение…");
            let ready = ch.validate()?;
            let id = db()?.add_challenge(&ready, now)?;
            println!("Добавлен челлендж {id}: {}. Дальше: whiteroom show {id}", ready.ch.title);
        }
        ["challenges"] => {
            for r in db()?.challenges()? {
                let st = if r.solved { "решён" } else if r.attempts > 0 { "в работе" } else { "новый" };
                println!("{:>3}  [{}] {}  ·  {}  ·  сложность {}/5  ·  {st}, подсказок {}", r.id, r.ch.kind, r.ch.title, r.ch.topic, r.ch.difficulty, r.hints);
            }
        }
        ["show", n] => {
            let (n, db) = (id(n)?, db()?);
            let r = db.challenge(n)?;
            println!("{}  ({}, сложность {}/5)\n\n{}\n", r.ch.title, r.ch.topic, r.ch.difficulty, r.ch.statement);
            let dir = challenge_dir(n)?;
            db.export_files(n, &dir)?;
            if r.ch.kind == "pwn" {
                println!("Файлы для анализа: {}\nЗапуск — только через песочницу: whiteroom pwn run {n}  ·  отладка: whiteroom pwn gdb {n}", dir.display());
            }
            println!("Сдать: whiteroom solve {n} <{}>  ·  подсказка: whiteroom hint {n}", if r.ch.kind == "pwn" { "флаг" } else { "ответ" });
        }
        ["hint", n] => {
            let (lvl, h) = db()?.next_hint(id(n)?)?;
            let name = ["идея", "метод", "первый шаг", "полное решение"][lvl as usize - 1];
            println!("Подсказка {lvl}/4 ({name}): {h}");
        }
        ["solve", n, rest @ ..] if !rest.is_empty() => {
            let ok = db()?.solve(id(n)?, &rest.join(" "), now)?;
            println!("{}", if ok { "✓ Решено." } else { "✗ Неверно. Подсказка: whiteroom hint <id>" });
        }
        ["practice", kind] if ["pwn", "math"].contains(kind) => practice(kind, None, now)?,
        ["practice", kind, h] if ["pwn", "math"].contains(kind) => practice(kind, Some(h.parse().map_err(|_| USAGE)?), now)?,
        ["pwn", "run", n] => pwn(id(n)?, false)?,
        ["pwn", "gdb", n] => pwn(id(n)?, true)?,
        ["review"] => review(None, now)?,
        ["review", h] => review(Some(h.parse().map_err(|_| USAGE)?), now)?,
        ["start", args @ ..] => {
            let topic = args.iter().copied().filter(|s| !matches!(*s,"--windows"|"--browser")).collect::<Vec<_>>().join(" ");
            start(if topic.is_empty() { "без темы" } else { &topic }, now, args.contains(&"--windows"), args.contains(&"--browser"))?;
        }
        ["stop"] => stop(now)?,
        ["block"] => println!("{}", db()?.blocked()?.join("\n")),
        ["block", "add", d] => db()?.block_add(d)?,
        ["block", "rm", d] => {
            if !db()?.block_rm(d)? {
                return Err(format!("{d} не в списке"));
            }
        }
        _ => return Err(USAGE.into()),
    }
    Ok(())
}

fn main() {
    if let Err(x) = run() {
        eprintln!("{x}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod consent_tests {
    use super::*;

    #[test]
    fn each_outbound_body_requires_its_own_exact_preview() {
        let prior = ("предыдущий ответ", "результат проверки");
        let titles = vec!["предыдущая тема".to_owned()];
        let bodies = [
            ai_request("первая сессия"),
            whiteroom_core::challenge::ai_request("вторая сессия", "math", &titles, None),
            whiteroom_core::challenge::ai_request("вторая сессия", "math", &titles, Some(prior)),
        ];
        for body in bodies {
            assert_eq!(body["model"], "claude-haiku-5-5");
            assert!(body.get("fallbacks").is_none());
            let mut output = vec![];
            let mut sent = vec![];
            let response = confirm_request(&body, &mut "y\n".as_bytes(), &mut output, |request| {
                sent.push(request.clone());
                Ok(serde_json::Value::Null)
            })
            .unwrap();
            assert!(response.is_some());
            let text = String::from_utf8(output).unwrap();
            let preview = text
                .strip_prefix("Запрос в Claude API:\n")
                .unwrap()
                .strip_suffix("\nОтправить? [y/N] ")
                .unwrap();
            assert_eq!(sent, [serde_json::from_str::<serde_json::Value>(preview).unwrap()]);
            for decline in ["", "\n", "n\n"] {
                assert!(
                    confirm_request(&body, &mut decline.as_bytes(), &mut vec![], |_| panic!(
                        "declined request reached transport"
                    ))
                    .unwrap()
                    .is_none()
                );
            }
        }
    }
}
