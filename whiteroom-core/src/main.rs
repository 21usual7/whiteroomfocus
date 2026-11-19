use std::io::{BufRead, BufReader, Read, Write, stdin, stdout};
use std::os::unix::net::UnixStream;
use std::time::{SystemTime, UNIX_EPOCH};
use whiteroom_core::ai_context::{self, MATH_BYTES, MATH_OUTPUT, REVIEW_BYTES, REVIEW_OUTPUT};
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
  allow <тема>              сайты, доступные во время сессии
  allow add|rm <тема> <домен>   изменить список для темы
  review [часов] [--attach файл]    вопросы по последней сессии и выбранным материалам
  browser-setup             связать whiteroom с расширением в Chromium
  tutor-overlay             открыть чат тьютора поверх окон Wayland

Практика:
  challenge add <файл.json> проверить челлендж в песочнице и добавить
  challenges                список челленджей
  show <id>                 условие; для pwn — файлы в ~/whiteroom/challenges/<id>
  hint <id>                 следующая подсказка: идея → метод → первый шаг → решение
  solve <id> <ответ|флаг>   сдать ответ (математика — выражение, например 3*sqrt(2)/4)
  pwn run <id>              запустить бинарь в песочнице (pwntools: process(['whiteroom','pwn','run','<id>']))
  pwn gdb <id>              gdb в песочнице
  practice <pwn|math> [ч]   агент составит задачу по теме последней сессии (нужен ANTHROPIC_API_KEY)
  practice math [ч] [--attach файл]    добавить текстовый материал к задаче по математике

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

fn context_options<'a>(mut args: &[&'a str]) -> Res<(Option<i64>, Vec<&'a str>)> {
    let mut hours = None;
    let mut files = Vec::new();
    while let Some((argument, rest)) = args.split_first() {
        args = rest;
        if *argument == "--attach" {
            let (file, remaining) = args.split_first().ok_or("После --attach укажи файл.")?;
            files.push(*file);
            args = remaining;
        } else if hours.is_none() {
            hours = Some(argument.parse().map_err(|_| USAGE)?);
        } else {
            return Err(USAGE.into());
        }
    }
    Ok((hours, files))
}

fn display_context(draft: ai_context::PreparedRequest) -> serde_json::Value {
    let estimate = draft.bytes.div_ceil(3);
    println!(
        "Вход: {} байт JSON, примерно {estimate} токенов. Пропущено фрагментов: {}.",
        draft.bytes, draft.omitted
    );
    draft.body
}

/// Агент составляет задачу по теме сессии; не прошедшая проверку в песочнице перегенерируется с текстом ошибки.
fn practice(kind: &str, hours: Option<i64>, files: &[&str], now: i64) -> Res<()> {
    let db = Db::open(&db_path()?)?;
    let sources = if kind == "math" {
        Some(db.review_sources(hours, now)?)
    } else {
        None
    };
    let activity = if sources.is_none() {
        db.review_activity(hours, now)?
    } else {
        String::new()
    };
    let attachments = ai_context::read_attachments(files)?;
    let done: Vec<String> = db
        .challenges()?
        .into_iter()
        .filter(|row| kind != "math" || row.ch.kind == "math")
        .map(|row| row.ch.title)
        .collect();
    let mut prev: Option<(String, String)> = None;
    for attempt in 1..=3 {
        println!("Составляю задачу (попытка {attempt}/3)…");
        let req = if let Some(sources) = &sources {
            let titles: Vec<_> = done
                .iter()
                .take(12)
                .map(|title| ai_context::prefix(title, 128).to_owned())
                .collect();
            let previous = prev.as_ref().map(|(answer, error)| {
                (
                    ai_context::prefix(answer, 4096),
                    ai_context::prefix(error, 1024),
                )
            });
            display_context(ai_context::prepare(
                sources,
                &attachments,
                MATH_BYTES,
                MATH_OUTPUT,
                |context| whiteroom_core::challenge::ai_request(context, "math", &titles, previous),
            )?)
        } else {
            whiteroom_core::challenge::ai_request(
                &activity,
                kind,
                &done,
                prev.as_ref()
                    .map(|(answer, error)| (answer.as_str(), error.as_str())),
            )
        };
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
    let cli = std::env::current_exe().map_err(|x| x.to_string())?;
    if windows && !whiteroom_core::tracker_start(&cli, id) {
        println!("Трекер окон не запустился.");
    }
    if let Err(error) = whiteroom_core::overlay::start(&cli, id, now) {
        eprintln!("{error}");
    }
    println!(
        "В Chromium разрешены: {}",
        db.allowed_sites(topic)?.join(", ")
    );
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

fn review(hours: Option<i64>, files: &[&str], now: i64) -> Res<()> {
    let db = Db::open(&db_path()?)?;
    let sources = db.review_sources(hours, now)?;
    let attachments = ai_context::read_attachments(files)?;
    let request = ai_context::prepare(
        &sources,
        &attachments,
        REVIEW_BYTES,
        REVIEW_OUTPUT,
        ai_request,
    )?;
    println!("Составляю вопросы…");
    let Some(response) = confirmed_call(&display_context(request))? else {
        return Ok(());
    };
    let (summary, items) = ai_parse(&response, now)?;
    run_review(
        &db,
        &mut stdin().lock(),
        &mut stdout(),
        now,
        &summary,
        &items,
    )
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
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|x| x.to_string())?;
    let dir = std::env::var_os("CHROME_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").filter(|dir| !dir.is_empty()))
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        })
        .ok_or("не задан HOME")?
        .join("chromium/NativeMessagingHosts");
    std::fs::create_dir_all(&dir).map_err(|x| x.to_string())?;
    let manifest = serde_json::json!({
        "name": HOST, "description": "WHITE ROOM", "type": "stdio",
        "path": exe, "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")]
    });
    let file = dir.join(format!("{HOST}.json"));
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&manifest).map_err(|x| x.to_string())?,
    )
    .map_err(|x| x.to_string())?;
    println!(
        "Записал {}\nТеперь chromium://extensions → «Режим разработчика» → «Загрузить распакованное» → папка extension/.",
        file.display()
    );
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
        ["practice", "math", rest @ ..] => {
            let (hours, files) = context_options(rest)?;
            practice("math", hours, &files, now)?;
        }
        ["practice", "pwn"] => practice("pwn", None, &[], now)?,
        ["practice", "pwn", h] => practice("pwn", Some(h.parse().map_err(|_| USAGE)?), &[], now)?,
        ["pwn", "run", n] => pwn(id(n)?, false)?,
        ["pwn", "gdb", n] => pwn(id(n)?, true)?,
        ["review", rest @ ..] => {
            let (hours, files) = context_options(rest)?;
            review(hours, &files, now)?;
        }
        ["start", args @ ..] => {
            let topic = args.iter().copied().filter(|s| !matches!(*s,"--windows"|"--browser")).collect::<Vec<_>>().join(" ");
            start(if topic.is_empty() { "без темы" } else { &topic }, now, args.contains(&"--windows"), args.contains(&"--browser"))?;
        }
        ["stop"] => stop(now)?,
        ["allow", "add", topic, domain] => db()?.site_allow(topic, domain)?,
        ["allow", "rm", topic, domain] => db()?.site_remove(topic, domain)?,
        ["allow", topic] => println!("{}", db()?.allowed_sites(topic)?.join("\n")),
        ["tutor-overlay"] => {
            let cli = std::env::current_exe().map_err(|err| err.to_string())?;
            whiteroom_core::overlay::open(&db_path()?, &cli)?;
        }
        ["timer-watch", session] | ["tutor-stdio", session] => {
            let session = if *session == "idle" { None } else { Some(id(session)?) };
            let path = db_path()?;
            if a[0] == "timer-watch" {
                whiteroom_core::overlay::watch(&path, session)?;
            } else {
                whiteroom_core::overlay::tutor_stdio(&path, session)?;
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
        let sources = ai_context::SessionSources {
            topic: Some("уравнения".into()),
            windows: "Всего 4 мин\nРедактор уравнений".into(),
            pages: vec![],
        };
        let bodies = [
            ai_context::prepare(&sources, &[], REVIEW_BYTES, REVIEW_OUTPUT, ai_request)
                .unwrap()
                .body,
            ai_context::prepare(&sources, &[], MATH_BYTES, MATH_OUTPUT, |text| {
                whiteroom_core::challenge::ai_request(text, "math", &titles, None)
            })
            .unwrap()
            .body,
            ai_context::prepare(&sources, &[], MATH_BYTES, MATH_OUTPUT, |text| {
                whiteroom_core::challenge::ai_request(text, "math", &titles, Some(prior))
            })
            .unwrap()
            .body,
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
