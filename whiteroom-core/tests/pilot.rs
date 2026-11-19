use std::io::Cursor;
use std::process::Command;
use whiteroom_core::*;

const MODULE: &str = include_str!("../../modules/re-x86-64.toml");

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("whiteroom-test-{}-{name}", rand::random::<u128>()));
    std::fs::create_dir(&d).unwrap();
    d
}

#[test]
fn grading() {
    assert_eq!(parse_num("-4"), parse_num("0xfffffffffffffffc"));
    assert_eq!(parse_num(" 0X1_0 "), Some(16));
    assert_eq!(parse_num("abc"), None);
    let m = Module::parse(MODULE).unwrap();
    let by = |id: &str| m.item.iter().find(|i| i.id == id).unwrap();
    assert!(grade(by("abi-p1"), " %RDI "));
    assert!(!grade(by("abi-p1"), "rsi"));
    assert!(grade(by("trace-p4"), "0x34"));
    assert!(grade(by("trace-p8"), "0xfffffffffffffffc"));
    assert!(!grade(by("trace-p8"), ""));
}

#[test]
fn module_shape() {
    let m = Module::parse(MODULE).unwrap();
    for o in &m.objective {
        let n = |role: &str| m.item.iter().filter(|i| i.objective == o.id && i.role == role).count();
        assert_eq!((n("practice"), n("test7")), (8, 4), "{}", o.id);
    }
    assert!(Module::parse(&MODULE.replace("role = \"test7\"", "role = \"x\"")).is_err());
}

/// Ключи ответов к задачам с кодом проверяются настоящим запуском (нужен gcc).
#[test]
fn answer_keys_by_execution() {
    let m = Module::parse(MODULE).unwrap();
    let d = tmp("asm");
    std::fs::write(d.join("main.c"), "#include <stdio.h>\nunsigned long f(void);\nint main(){printf(\"%lu\\n\", f());}\n").unwrap();
    let mut checked = 0;
    for it in m.item.iter().filter(|i| i.run) {
        let s = format!(".intel_syntax noprefix\n.globl f\nf:\npush rbx\n{}\npop rbx\nret\n", it.code.as_ref().unwrap());
        std::fs::write(d.join("f.s"), s).unwrap();
        let exe = d.join("a.out");
        let cc = Command::new("gcc").current_dir(&d).args(["main.c", "f.s", "-o"]).arg(&exe).output().unwrap();
        assert!(cc.status.success(), "{}: {}", it.id, String::from_utf8_lossy(&cc.stderr));
        let got: u64 = String::from_utf8(Command::new(&exe).output().unwrap().stdout).unwrap().trim().parse().unwrap();
        assert_eq!(Some(got), parse_num(&it.answer[0]), "{}", it.id);
        checked += 1;
    }
    assert_eq!(checked, 16);
}

#[test]
fn pilot_end_to_end() {
    let db = Db::open(tmp("e2e").join("db.sqlite").to_str().unwrap()).unwrap();
    let m = Module::parse(MODULE).unwrap();
    let t0 = 1_800_000_000;
    let arms = db.load(&m, t0).unwrap();
    assert_eq!(arms.iter().filter(|a| a.1 == "wr").count(), 2);
    assert_eq!(db.load(&m, t0).unwrap(), arms, "повторная загрузка не перераспределяет");

    // Сессия: первый ответ неверный, остальные верные, ошибка исправляется в повторе.
    let items = db.new_items(4).unwrap();
    assert_eq!(items.len(), 4);
    assert_ne!(items[0].objective, items[1].objective, "цели чередуются");
    let mut input = String::from("3\nневерно\n");
    for it in &items[1..] {
        input += &format!("{}\n", it.answer[0]);
    }
    input += &format!("{}\n", items[0].answer[0]);
    let mut out = Vec::new();
    run_session(&db, &mut Cursor::new(input), &mut out, t0, 4).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("Итог: 3/4, прогноз был 3."), "{out}");

    // Ошибка возвращается раньше, чем верный ответ.
    assert_eq!(db.due_reviews(t0 + DAY).unwrap().iter().map(|i| i.id.clone()).collect::<Vec<_>>(), vec![items[0].id.clone()]);
    assert!(db.due_reviews(t0 + 30 * DAY).unwrap().len() == 4);

    // Проверка: только через 7 дней после done, по невиданным вопросам, для обеих групп.
    for (id, _) in &arms {
        db.done(id, 30, t0).unwrap();
    }
    assert!(db.done(&arms[0].0, 30, t0).is_err());
    assert!(db.new_items(4).unwrap().is_empty());
    assert!(db.tests_due(t0 + 7 * DAY - 1).unwrap().is_empty());
    let tests = db.tests_due(t0 + 7 * DAY).unwrap();
    assert_eq!(tests.len(), 16);
    assert!(tests.iter().all(|i| i.role == "test7"));
    let input: String = tests.iter().map(|i| format!("{}\n", i.answer[0])).collect();
    run_test(&db, &mut Cursor::new(input), &mut Vec::new(), t0 + 7 * DAY).unwrap();
    assert!(db.tests_due(t0 + 8 * DAY).unwrap().is_empty(), "проверка одноразовая");
    let st = db.status(t0 + 8 * DAY).unwrap();
    assert_eq!(st.matches("проверка: 4/4").count(), 4, "{st}");
    assert!(st.contains("WHITE ROOM: 8/8 (100%)") && st.contains("учишь сам : 8/8 (100%); время: 60 мин"), "{st}");
    assert!(st.contains("Калибровка за 1 сессий: прогноз 3, факт 3 — точно."), "{st}");
    let ex = db.export().unwrap();
    assert_eq!(ex["attempt"].as_array().unwrap().len(), 4 + 1 + 16);
}

#[test]
fn eof_mid_session_is_safe() {
    let db = Db::open(tmp("eof").join("db.sqlite").to_str().unwrap()).unwrap();
    db.load(&Module::parse(MODULE).unwrap(), 0).unwrap();
    run_session(&db, &mut Cursor::new("2\n"), &mut Vec::new(), 0, 4).unwrap();
    run_session(&db, &mut Cursor::new(""), &mut Vec::new(), 0, 4).unwrap();
    assert_eq!(db.new_items(4).unwrap().len(), 4, "ничего не засчитано");
}

#[test]
fn focus_tracking() {
    let db = Db::open(tmp("focus").join("db.sqlite").to_str().unwrap()).unwrap();
    let id = db.study_start_with_sources("окна", 999, true, false).unwrap();
    db.focus_event(id, "activewindow>>ghidra,CodeBrowser: crackme1, main", 1000).unwrap();
    db.focus_event(id, "workspace>>2", 1100).unwrap();
    db.focus_tick(id, 1600).unwrap();
    db.focus_event(id, "activewindow>>firefox,x86 calling convention", 1600).unwrap();
    db.focus_tick(id, 1800).unwrap();
    db.focus_event(id, "activewindow>>ghidra,CodeBrowser: crackme1, main", 1800).unwrap();
    db.focus_tick(id, 2100).unwrap();
    let log = db.focus_log(0).unwrap();
    assert!(log.starts_with("Всего 18 мин"), "{log}");
    assert!(log.contains("  15 мин  ghidra") && log.contains("CodeBrowser: crackme1, main"), "{log}");
    assert!(log.contains("   3 мин  firefox"), "{log}");
    assert!(db.focus_log(1700).unwrap().starts_with("Всего 6 мин"), "окно обрезается по since");
    db.focus_event(id, &format!("activewindow>>{},", IDLE), 2100).unwrap();
    db.focus_tick(id, 2700).unwrap();
    db.focus_event(id, "activewindow>>ghidra,CodeBrowser: crackme1, main", 2700).unwrap();
    let log = db.focus_log(0).unwrap();
    assert!(log.starts_with("Всего 18 мин, простой 10 мин"), "простой не входит в «Всего»: {log}");
    assert!(!log.contains("  10 мин"), "{log}");
}

fn api_response(questions: serde_json::Value) -> serde_json::Value {
    let text = serde_json::json!({"summary": "45 мин: стек x86-64 в Ghidra.", "questions": questions}).to_string();
    serde_json::json!({"stop_reason": "end_turn", "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": text}]})
}

#[test]
fn ai_request_shape() {
    let r = ai_request("Всего 45 мин\n  45 мин  ghidra\n");
    assert_eq!(r["model"], "claude-haiku-5-5");
    assert_eq!(r["output_config"]["format"]["type"], "json_schema");
    assert!(r["messages"][0]["content"].as_str().unwrap().contains("<activity>\nВсего 45 мин"));
    assert!(r.get("thinking").is_none() && r.get("temperature").is_none());
}

#[test]
fn ai_parse_validates() {
    let q = |kind: &str, ans: &str| serde_json::json!({"prompt": "?", "kind": kind, "answers": [ans], "hint": "", "explain": "e"});
    let resp = api_response(serde_json::json!([q("number", "0x8"), q("number", "восемь"), q("text", "rbp"), q("text", "a"), q("text", "b")]));
    let (summary, items) = ai_parse(&resp, 7).unwrap();
    assert!(summary.contains("Ghidra"));
    assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["ai-7-0", "ai-7-1", "ai-7-2"], "нечисловой ответ выброшен, максимум 3");
    assert!(items[0].hint.is_none());
    assert!(ai_parse(&serde_json::json!({"stop_reason": "refusal", "content": []}), 0).is_err());
    assert!(ai_parse(&serde_json::json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": "не json"}]}), 0).is_err());
}

#[test]
fn ai_review_flow() {
    let db = Db::open(tmp("ai").join("db.sqlite").to_str().unwrap()).unwrap();
    let q = |p: &str, ans: &str| serde_json::json!({"prompt": p, "kind": "number", "answers": [ans], "hint": "h", "explain": "e"});
    let (s, items) = ai_parse(&api_response(serde_json::json!([q("[rbp+?] адрес возврата", "8"), q("плохой", "1"), q("rsp%16 на входе", "8")])), 1000).unwrap();
    let mut out = Vec::new();
    run_review(&db, &mut Cursor::new("0x8\n!\n3\n"), &mut out, 1000, &s, &items).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("Убрал вопрос") && out.contains("Итог: 1/3"), "{out}");
    // Ошибочный вопрос возвращается в повторениях раньше, удалённый — никогда.
    let due: Vec<_> = db.due_reviews(1000 + 2 * DAY).unwrap().into_iter().map(|i| i.id).collect();
    assert_eq!(due, ["ai-1000-2"]);
    assert_eq!(db.due_reviews(1000 + 60 * DAY).unwrap().len(), 2);
    assert!(db.status(1000).unwrap().contains("вопросы ИИ по твоим сессиям: 2"));
}

#[test]
fn browser_pages() {
    let db = Db::open(tmp("pages").join("db.sqlite").to_str().unwrap()).unwrap();
    let id = db.study_start_with_sources("вкладки", 1, false, true).unwrap();
    let page = |d: &str, t: &str, x: Option<&str>| serde_json::json!({"type": "page", "capture": id, "domain": d, "title": t, "excerpt": x});
    assert!(db.page_event(&page("ir0nstone.gitbook.io", "ret2win", None), Some("ir0nstone.gitbook.io"), 10).is_ok());
    assert!(db.page_event(&page("ir0nstone.gitbook.io", "ret2win", None), Some("ir0nstone.gitbook.io"), 11).is_ok(), "дубль молча пропускается");
    db.page_event(&page("ir0nstone.gitbook.io", "ret2win", Some(&"x".repeat(9000))), Some("ir0nstone.gitbook.io"), 12).unwrap();
    db.page_event(&page("localhost:8080", "lab", Some("  ")), Some("ir0nstone.gitbook.io"), 13).unwrap();
    for bad in [page("evil.com/'; DROP", "t", None), page("", "t", None), serde_json::json!({"type": "exec", "domain": "a.b"})] {
        assert!(db.page_event(&bad, Some("ir0nstone.gitbook.io"), 14).is_err(), "{bad}");
    }
    let log = db.pages_log(0).unwrap();
    assert!(log.starts_with("── ir0nstone.gitbook.io — ret2win\n"), "{log}");
    assert_eq!(log.matches('x').count(), 4000, "выдержка обрезана");
    assert!(!log.contains("localhost"), "пустая выдержка не попадает в разбор");
    assert_eq!(db.export().unwrap()["page"].as_array().unwrap().len(), 3);
}

#[test]
fn native_framing() {
    let mut buf = Vec::new();
    write_native(&mut buf, &serde_json::json!({"ok": true})).unwrap();
    write_native(&mut buf, &serde_json::json!({"a": "б"})).unwrap();
    let mut r = Cursor::new(buf);
    assert_eq!(read_native(&mut r).unwrap().unwrap()["ok"], true);
    assert_eq!(read_native(&mut r).unwrap().unwrap()["a"], "б");
    assert!(read_native(&mut r).unwrap().is_none(), "конец потока");
    let huge = ((MAX_NATIVE_MSG + 1) as u32).to_ne_bytes();
    assert!(read_native(&mut Cursor::new(huge.to_vec())).is_err());
    assert!(read_native(&mut Cursor::new(vec![3, 0, 0, 0, b'{'])).is_err(), "обрыв посреди сообщения");
}

#[test]
fn session_allows_google_and_the_current_topics_sites() {
    let path = tmp("study").join("db.sqlite");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    db.site_allow("Математика", "www.Docs.Example.org").unwrap();
    db.site_allow("Сети", "network.example").unwrap();
    for domain in ["youtube", "a b.com", "x.com/'", "a..com", "-a.com", ""] {
        assert!(db.site_allow("Математика", domain).is_err(), "{domain}");
    }
    assert!(db.site_allow(" ", "example.com").is_err());
    assert!(db.site_remove("Математика", "google.com").is_err());
    assert_eq!(
        db.allowed_sites(" математика ").unwrap(),
        ["google.com", "docs.example.org"]
    );
    assert_eq!(
        db.browser_state().unwrap(),
        serde_json::json!({ "session": false, "allowed": [], "capture": null })
    );
    db.study_start("Математика", 100).unwrap();
    assert!(db.study_start("ещё", 101).is_err());
    drop(db);
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        db.browser_state().unwrap(),
        serde_json::json!({
            "session": true, "allowed": ["google.com", "docs.example.org"], "capture": null
        })
    );
    assert_eq!(db.study_last_start().unwrap(), Some(100));
    assert_eq!(db.study_stop(200).unwrap(), (100, "Математика".to_string()));
    assert!(db.study_stop(201).is_err());
    db.study_start("Сети", 300).unwrap();
    assert_eq!(
        db.browser_state().unwrap()["allowed"],
        serde_json::json!(["google.com", "network.example"])
    );
    db.site_remove("сети", "WWW.NETWORK.EXAMPLE").unwrap();
    assert_eq!(
        db.browser_state().unwrap()["allowed"],
        serde_json::json!(["google.com"])
    );
    db.study_stop(400).unwrap();
    db.focus_start("study", "Сети и протоколы", "", 500)
        .unwrap();
    assert_eq!(
        db.browser_state().unwrap()["allowed"],
        serde_json::json!(["google.com"])
    );
}

#[test]
fn wayland_tutor_uses_the_session_chat_without_credentials() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let path = tmp("wayland-tutor").join("db.sqlite");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let id = db
        .focus_start_with_sources("hunt", "Анализ журналов", "", 100, false, false)
        .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_whiteroom"))
        .args(["tutor-stdio", &id.to_string()])
        .env("WHITEROOM_DB", &path)
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |request: serde_json::Value| -> serde_json::Value {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut response = String::new();
        output.read_line(&mut response).unwrap();
        serde_json::from_str(&response).unwrap()
    };
    assert_eq!(
        exchange(serde_json::json!({ "method": "context" }))["value"]["suggestedConceptId"],
        "logs"
    );
    let preview = exchange(serde_json::json!({
        "method": "prepare", "question": "Что такое корреляция?", "conceptId": null
    }));
    assert_eq!(preview["value"]["local"], true);
    let answer = exchange(serde_json::json!({
        "method": "send", "previewId": preview["value"]["previewId"]
    }));
    assert_eq!(answer["value"]["sessionId"], id);
    let history = tutor::context(&db).unwrap();
    assert_eq!(history.messages.len(), 2);
    assert!(history.messages[1].text.contains("наблюдения меняются вместе"));

    db.stop_session(Some(id), 101).unwrap();
    assert!(
        exchange(serde_json::json!({ "method": "context" }))["error"]
            .as_str()
            .unwrap()
            .contains("Сессия изменилась")
    );
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn cyber_focus_survives_reopen_and_validates_direction() {
    let path = tmp("cyber-focus").join("db.sqlite");
    let open = || Db::open(path.to_str().unwrap()).unwrap();
    let db = open();
    db.study_start("Старая сессия", 1).unwrap();
    let id = db.study_active().unwrap().unwrap().0;
    assert_eq!(db.focus_selection(id).unwrap(), None);
    db.study_stop(2).unwrap();
    assert!(db.focus_start("hunt", "Криптография", "", 3).is_err());
    assert!(db.study_active().unwrap().is_none());

    for (i, mode) in focus::MODES.iter().enumerate() {
        let now = 10 + i as i64 * 100;
        let direction = mode.options[1];
        db.focus_start(mode.id, direction, "", now).unwrap();
        let reopened = open();
        let (id, started, topic) = reopened.study_active().unwrap().unwrap();
        assert_eq!((started, topic.as_str()), (now, direction));
        assert_eq!(reopened.focus_selection(id).unwrap().unwrap(), (mode.id.into(), direction.into()));
        assert!(reopened.focus_start(mode.id, direction, "", now).is_err());
        reopened.study_stop(now + 60).unwrap();
    }
    let history = db.focus_history().unwrap();
    assert_eq!(history.len(), 5);
    assert_eq!(history[0].3, "research");
    assert_eq!(db.export().unwrap()["study_focus"].as_array().unwrap().len(), 6);
}

#[test]
fn capture_is_scoped_to_the_consented_session_and_site() {
    let db = Db::open(":memory:").unwrap();
    let page = |id, title| {
        serde_json::json!({"type":"page","capture":id,"domain":"study.example",
        "title":title,"excerpt":title})
    };
    assert!(db.page_event(&page(1, "before"), Some("study.example"), 1).is_err());
    let first = db.study_start_with_sources("first", 2, true, true).unwrap();
    db.page_event(&page(first, "metadata only"), None, 3).unwrap();
    db.page_event(&page(first, "first excerpt"), Some("study.example"), 4)
        .unwrap();
    db.focus_event(first, "activewindow>>editor,first window", 4).unwrap();
    db.focus_tick(first, 140).unwrap();
    assert!(db.review_activity(None, 140).unwrap().contains("first window"));

    db.revoke_capture(first, "windows").unwrap();
    db.focus_tick(first, 180).unwrap();
    db.focus_event(first, "activewindow>>editor,revoked", 181).unwrap();
    db.study_stop(190).unwrap();
    assert!(db.page_event(&page(first, "after stop"), Some("study.example"), 190).is_err());
    let second = db.study_start_with_sources("second", 190, false, true).unwrap();
    assert!(db.page_event(&page(first, "stale"), Some("study.example"), 191).is_err());
    db.focus_event(first, "activewindow>>editor,old worker", 191).unwrap();
    db.page_event(&page(second, "second excerpt"), Some("study.example"), 192)
        .unwrap();
    db.revoke_capture(second, "browser").unwrap();
    assert!(db.page_event(&page(second, "revoked"), Some("study.example"), 193).is_err());
    db.study_stop(194).unwrap();
    let latest = db.review_activity(None, 999).unwrap();
    assert!(latest.contains("second excerpt") && !latest.contains("first"));
    let data = db.export().unwrap();
    assert_eq!(data["page"].as_array().unwrap().len(), 3);
    assert!(data["page"][0]["excerpt"].is_null());
    assert_eq!(data["focus"].as_array().unwrap().len(), 1);
    assert_eq!(data["focus"][0]["end"], 140);
    db.study_start("private", 200).unwrap();
    assert!(db.review_activity(None, 201).is_err());
    assert!(db.browser_state().unwrap()["capture"].is_null());
}

#[test]
fn upgrade_preserves_old_rows_and_starts_with_capture_off() {
    let path = tmp("upgrade").join("legacy.sqlite");
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(
        "CREATE TABLE study(id INTEGER PRIMARY KEY,started INT NOT NULL,ended INT,topic TEXT NOT NULL);
        CREATE TABLE page(id INTEGER PRIMARY KEY,ts INT NOT NULL,domain TEXT NOT NULL,title TEXT NOT NULL,excerpt TEXT);
        CREATE TABLE focus(id INTEGER PRIMARY KEY,start INT NOT NULL,end INT NOT NULL,class TEXT NOT NULL,title TEXT NOT NULL);
        INSERT INTO study VALUES(1,10,NULL,'legacy');
        INSERT INTO focus VALUES(1,9,12,'legacy','legacy title');
        INSERT INTO page VALUES(1,11,'old.example','before upgrade','legacy excerpt');",
    )
    .unwrap();
    drop(old);
    for _ in 0..2 {
        let db = Db::open(path.to_str().unwrap()).unwrap();
        let stale = rusqlite::Connection::open(&path).unwrap();
        assert!(
            stale
                .execute("INSERT INTO page(ts,domain,title) VALUES(12,'stale.example','stale')", [])
                .is_err()
        );
        assert!(
            stale
                .execute("INSERT INTO focus(start,end,class,title) VALUES(12,12,'stale','stale')", [])
                .is_err()
        );
        assert!(stale.execute("UPDATE focus SET end=20 WHERE id=1", []).is_err());
        assert_eq!(db.active_focus().unwrap().unwrap().3, (1, false, false));
        assert_eq!(db.capture_state().unwrap(), Some((1, false, false)));
        assert_eq!(db.export().unwrap()["page"][0]["excerpt"], "legacy excerpt");
        assert!(db.review_activity(None, 20).is_err());
    }
}

#[test]
fn native_host_discards_stale_events_and_consumes_site_permission_once() {
    use std::io::{BufReader, Write};
    use std::process::Stdio;
    let path = tmp("native-consent").join("db.sqlite");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_whiteroom"))
        .arg("chrome-extension://loeenilmfolklcifabeaeaedhnoffdca/")
        .env("WHITEROOM_DB", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut exchange = |msg| {
        write_native(&mut input, &msg).unwrap();
        input.flush().unwrap();
        read_native(&mut output).unwrap().unwrap()
    };
    let state = serde_json::json!({"type":"state","request":123});
    let page = |id, title| {
        serde_json::json!({"type":"page","capture":id,"domain":"example.invalid",
        "title":title,"excerpt":"private fixture"})
    };
    let grant = |id| serde_json::json!({"type":"site_permission","capture":id,"domain":"example.invalid"});
    assert!(exchange(state.clone())["capture"].is_null());
    assert_eq!(exchange(page(1, "before"))["ok"], false);
    let id = db.study_start_with_sources("first", 1, false, true).unwrap();
    let response = exchange(state.clone());
    assert_eq!(response["request"], 123);
    assert_eq!(response["capture"], id);
    exchange(grant(id));
    assert_eq!(exchange(page(id, "marked"))["ok"], true);
    exchange(page(id, "unmarked"));
    exchange(grant(id));
    db.study_stop(10).unwrap();
    let next = db.study_start_with_sources("second", 10, false, true).unwrap();
    assert_eq!(exchange(page(id, "stale"))["ok"], false);
    exchange(page(next, "new session"));
    db.revoke_capture(next, "browser").unwrap();
    assert_eq!(exchange(page(next, "revoked"))["ok"], false);
    drop(input);
    assert!(child.wait().unwrap().success());
    let export = db.export().unwrap();
    let pages = export["page"].as_array().unwrap();
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0]["excerpt"], "private fixture");
    assert!(pages[1]["excerpt"].is_null() && pages[2]["excerpt"].is_null());
}

#[test]
fn briefing_shows_last_active_day() {
    let db = Db::open(tmp("brief").join("db.sqlite").to_str().unwrap()).unwrap();
    assert!(db.briefing(0, 0).unwrap()["last"].is_null(), "пустая база — без выдуманных данных");

    let t0 = 1_800_000_000; // 08:00 UTC
    db.load(&Module::parse(MODULE).unwrap(), t0).unwrap();
    let id = db.focus_start_with_sources("hunt", "Анализ журналов", "Логи nginx", t0, false, false).unwrap();
    db.stop_session(Some(id), t0 + 1800).unwrap();
    let it = &db.new_items(1).unwrap()[0];
    run_session(&db, &mut Cursor::new(format!("1\n{}\n", it.answer[0])), &mut Vec::new(), t0 + 2000, 1).unwrap();

    // Через три дня брифинг показывает тот же день, а не пустое «вчера».
    let b = db.briefing(t0 + 3 * DAY, 0).unwrap();
    let last = &b["last"];
    assert_eq!(last["start"], t0 - 28_800);
    assert_eq!(last["studies"][0], serde_json::json!(["Логи nginx", "hunt", "Анализ журналов", 30]));
    assert_eq!((last["answered"].as_i64(), last["correct"].as_i64()), (Some(1), Some(1)));
    assert_eq!(last["facts"][0][1], serde_json::json!(it.explain));
    assert_eq!(b["week"]["minutes"], 30);
    assert!(b["plan"]["due"].as_i64().unwrap() <= 1 && b["upcoming"]["week"].as_i64().unwrap() >= 1, "{b}");

    // Сегодняшние занятия во «вчера» не попадают; смещение часового пояса сдвигает границу дня.
    assert!(db.briefing(t0 + 3600, 0).unwrap()["last"].is_null());
    assert_eq!(db.briefing(t0 + 3600, -9 * 3600).unwrap()["last"]["start"], t0 - 28_800 + 9 * 3600 - DAY);
}
