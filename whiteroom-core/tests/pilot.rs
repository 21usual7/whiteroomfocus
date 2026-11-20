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
        let n = |role: &str| {
            m.item
                .iter()
                .filter(|i| i.objective == o.id && i.role == role)
                .count()
        };
        assert_eq!((n("practice"), n("test7")), (8, 4), "{}", o.id);
    }
    assert!(Module::parse(&MODULE.replace("role = \"test7\"", "role = \"x\"")).is_err());
}

/// Ключи ответов к задачам с кодом проверяются настоящим запуском (нужен gcc).
#[test]
fn answer_keys_by_execution() {
    let m = Module::parse(MODULE).unwrap();
    let d = tmp("asm");
    std::fs::write(
        d.join("main.c"),
        "#include <stdio.h>\nunsigned long f(void);\nint main(){printf(\"%lu\\n\", f());}\n",
    )
    .unwrap();
    let mut checked = 0;
    for it in m.item.iter().filter(|i| i.run) {
        let s = format!(
            ".intel_syntax noprefix\n.globl f\nf:\npush rbx\n{}\npop rbx\nret\n",
            it.code.as_ref().unwrap()
        );
        std::fs::write(d.join("f.s"), s).unwrap();
        let exe = d.join("a.out");
        let cc = Command::new("gcc")
            .current_dir(&d)
            .args(["main.c", "f.s", "-o"])
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            cc.status.success(),
            "{}: {}",
            it.id,
            String::from_utf8_lossy(&cc.stderr)
        );
        let got: u64 = String::from_utf8(Command::new(&exe).output().unwrap().stdout)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
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
    assert_eq!(
        db.load(&m, t0).unwrap(),
        arms,
        "повторная загрузка не перераспределяет"
    );

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
    assert_eq!(
        db.due_reviews(t0 + DAY)
            .unwrap()
            .iter()
            .map(|i| i.id.clone())
            .collect::<Vec<_>>(),
        vec![items[0].id.clone()]
    );
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
    assert!(
        db.tests_due(t0 + 8 * DAY).unwrap().is_empty(),
        "проверка одноразовая"
    );
    let st = db.status(t0 + 8 * DAY).unwrap();
    assert_eq!(st.matches("проверка: 4/4").count(), 4, "{st}");
    assert!(
        st.contains("WHITE ROOM: 8/8 (100%)")
            && st.contains("учишь сам : 8/8 (100%); время: 60 мин"),
        "{st}"
    );
    assert!(
        st.contains("Калибровка за 1 сессий: прогноз 3, факт 3 — точно."),
        "{st}"
    );
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
    let id = db
        .study_start_with_sources("окна", 999, true, false)
        .unwrap();
    db.focus_event(id, "activewindow>>ghidra,CodeBrowser: crackme1, main", 1000)
        .unwrap();
    db.focus_event(id, "workspace>>2", 1100).unwrap();
    db.focus_tick(id, 1600).unwrap();
    db.focus_event(id, "activewindow>>firefox,x86 calling convention", 1600)
        .unwrap();
    db.focus_tick(id, 1800).unwrap();
    db.focus_event(id, "activewindow>>ghidra,CodeBrowser: crackme1, main", 1800)
        .unwrap();
    db.focus_tick(id, 2100).unwrap();
    let log = db.focus_log(0).unwrap();
    assert!(log.starts_with("Всего 18 мин"), "{log}");
    assert!(
        log.contains("  15 мин  ghidra") && log.contains("CodeBrowser: crackme1, main"),
        "{log}"
    );
    assert!(log.contains("   3 мин  firefox"), "{log}");
    assert!(
        db.focus_log(1700).unwrap().starts_with("Всего 6 мин"),
        "окно обрезается по since"
    );
    db.focus_event(id, &format!("activewindow>>{},", IDLE), 2100)
        .unwrap();
    db.focus_tick(id, 2700).unwrap();
    db.focus_event(id, "activewindow>>ghidra,CodeBrowser: crackme1, main", 2700)
        .unwrap();
    let log = db.focus_log(0).unwrap();
    assert!(
        log.starts_with("Всего 18 мин, простой 10 мин"),
        "простой не входит в «Всего»: {log}"
    );
    assert!(!log.contains("  10 мин"), "{log}");
}

fn api_response(questions: serde_json::Value) -> serde_json::Value {
    let text =
        serde_json::json!({"summary": "45 мин: стек x86-64 в Ghidra.", "questions": questions})
            .to_string();
    serde_json::json!({"stop_reason": "end_turn", "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": text}]})
}

#[test]
fn ai_request_shape() {
    let r = ai_request("Всего 45 мин\n  45 мин  ghidra\n");
    assert_eq!(r["model"], "claude-haiku-5-5");
    assert_eq!(r["output_config"]["format"]["type"], "json_schema");
    assert!(
        r["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("<activity>\nВсего 45 мин")
    );
    assert!(r.get("thinking").is_none() && r.get("temperature").is_none());
}

#[test]
fn ai_parse_validates() {
    let q = |kind: &str, ans: &str| serde_json::json!({"prompt": "?", "kind": kind, "answers": [ans], "hint": "", "explain": "e"});
    let resp = api_response(serde_json::json!([
        q("number", "0x8"),
        q("number", "восемь"),
        q("text", "rbp"),
        q("text", "a"),
        q("text", "b")
    ]));
    let (summary, items) = ai_parse(&resp, 7).unwrap();
    assert!(summary.contains("Ghidra"));
    assert_eq!(
        items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["ai-7-0", "ai-7-1", "ai-7-2"],
        "нечисловой ответ выброшен, максимум 3"
    );
    assert!(items[0].hint.is_none());
    assert!(
        ai_parse(
            &serde_json::json!({"stop_reason": "refusal", "content": []}),
            0
        )
        .is_err()
    );
    assert!(ai_parse(&serde_json::json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": "не json"}]}), 0).is_err());
}

#[test]
fn ai_review_flow() {
    let db = Db::open(tmp("ai").join("db.sqlite").to_str().unwrap()).unwrap();
    let q = |p: &str, ans: &str| serde_json::json!({"prompt": p, "kind": "number", "answers": [ans], "hint": "h", "explain": "e"});
    let (s, items) = ai_parse(
        &api_response(serde_json::json!([
            q("[rbp+?] адрес возврата", "8"),
            q("плохой", "1"),
            q("rsp%16 на входе", "8")
        ])),
        1000,
    )
    .unwrap();
    let mut out = Vec::new();
    run_review(
        &db,
        &mut Cursor::new("0x8\n!\n3\n"),
        &mut out,
        1000,
        &s,
        &items,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.contains("Убрал вопрос") && out.contains("Итог: 1/3"),
        "{out}"
    );
    // Ошибочный вопрос возвращается в повторениях раньше, удалённый — никогда.
    let due: Vec<_> = db
        .due_reviews(1000 + 2 * DAY)
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(due, ["ai-1000-2"]);
    assert_eq!(db.due_reviews(1000 + 60 * DAY).unwrap().len(), 2);
    assert!(
        db.status(1000)
            .unwrap()
            .contains("вопросы ИИ по твоим сессиям: 2")
    );
}

#[test]
fn browser_pages() {
    let db = Db::open(tmp("pages").join("db.sqlite").to_str().unwrap()).unwrap();
    let id = db
        .study_start_with_sources("вкладки", 1, false, true)
        .unwrap();
    let page = |d: &str, t: &str, x: Option<&str>| serde_json::json!({"type": "page", "capture": id, "domain": d, "title": t, "excerpt": x});
    assert!(
        db.page_event(
            &page("ir0nstone.gitbook.io", "ret2win", None),
            Some("ir0nstone.gitbook.io"),
            10
        )
        .is_ok()
    );
    assert!(
        db.page_event(
            &page("ir0nstone.gitbook.io", "ret2win", None),
            Some("ir0nstone.gitbook.io"),
            11
        )
        .is_ok(),
        "дубль молча пропускается"
    );
    db.page_event(
        &page("ir0nstone.gitbook.io", "ret2win", Some(&"x".repeat(9000))),
        Some("ir0nstone.gitbook.io"),
        12,
    )
    .unwrap();
    db.page_event(
        &page("localhost:8080", "lab", Some("  ")),
        Some("ir0nstone.gitbook.io"),
        13,
    )
    .unwrap();
    for bad in [
        page("evil.com/'; DROP", "t", None),
        page("", "t", None),
        serde_json::json!({"type": "exec", "domain": "a.b"}),
    ] {
        assert!(
            db.page_event(&bad, Some("ir0nstone.gitbook.io"), 14)
                .is_err(),
            "{bad}"
        );
    }
    let log = db.pages_log(0).unwrap();
    assert!(
        log.starts_with("── ir0nstone.gitbook.io — ret2win\n"),
        "{log}"
    );
    assert_eq!(log.matches('x').count(), 4000, "выдержка обрезана");
    assert!(
        !log.contains("localhost"),
        "пустая выдержка не попадает в разбор"
    );
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
    assert!(
        read_native(&mut Cursor::new(vec![3, 0, 0, 0, b'{'])).is_err(),
        "обрыв посреди сообщения"
    );
}

#[test]
fn session_blocks_only_listed_sites() {
    let path = tmp("study").join("db.sqlite");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    db.site_block("www.Youtube.com").unwrap();
    db.site_block("reddit.com").unwrap();
    for domain in ["youtube", "a b.com", "x.com/'", "a..com", "-a.com", ""] {
        assert!(db.site_block(domain).is_err(), "{domain}");
    }
    assert_eq!(db.blocked_sites().unwrap(), ["reddit.com", "youtube.com"]);
    assert_eq!(
        db.browser_state().unwrap(),
        serde_json::json!({ "session": false, "blocked": [], "capture": null })
    );
    db.study_start("Математика", 100).unwrap();
    assert!(db.study_start("ещё", 101).is_err());
    drop(db);
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(
        db.browser_state().unwrap(),
        serde_json::json!({
            "session": true, "blocked": ["reddit.com", "youtube.com"], "capture": null
        })
    );
    assert_eq!(db.study_last_start().unwrap(), Some(100));
    assert_eq!(db.study_stop(200).unwrap(), (100, "Математика".to_string()));
    assert!(db.study_stop(201).is_err());
    db.site_unblock("WWW.REDDIT.COM").unwrap();
    db.study_start("Сети", 300).unwrap();
    assert_eq!(
        db.browser_state().unwrap()["blocked"],
        serde_json::json!(["youtube.com"])
    );
    db.study_stop(400).unwrap();
}

#[test]
fn block_lists_are_locked_during_a_session() {
    let path = tmp("lock").join("db.sqlite");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    db.app_block(" Discord ").unwrap();
    for name in ["a", "a b", "x/y", ""] {
        assert!(db.app_block(name).is_err(), "{name}");
    }
    assert_eq!(db.blocked_apps().unwrap(), ["discord"]);
    db.study_start("Сети", 100).unwrap();
    assert!(db.app_block("telegram").is_err());
    assert!(db.app_unblock("discord").is_err());
    assert!(db.site_block("youtube.com").is_err());
    assert!(db.site_unblock("youtube.com").is_err());
    assert_eq!(db.blocked_apps().unwrap(), ["discord"]);
    db.study_stop(200).unwrap();
    db.app_unblock("discord").unwrap();
    assert!(db.blocked_apps().unwrap().is_empty());
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
    assert!(
        history.messages[1]
            .text
            .contains("наблюдения меняются вместе")
    );

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
        assert_eq!(
            reopened.focus_selection(id).unwrap().unwrap(),
            (mode.id.into(), direction.into())
        );
        assert!(reopened.focus_start(mode.id, direction, "", now).is_err());
        reopened.study_stop(now + 60).unwrap();
    }
    let history = db.focus_history().unwrap();
    assert_eq!(history.len(), 5);
    assert_eq!(history[0].3, "research");
    assert_eq!(
        db.export().unwrap()["study_focus"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
}

#[test]
fn capture_is_scoped_to_the_consented_session_and_site() {
    let db = Db::open(":memory:").unwrap();
    let page = |id, title| {
        serde_json::json!({"type":"page","capture":id,"domain":"study.example",
        "title":title,"excerpt":title})
    };
    assert!(
        db.page_event(&page(1, "before"), Some("study.example"), 1)
            .is_err()
    );
    let first = db.study_start_with_sources("first", 2, true, true).unwrap();
    db.page_event(&page(first, "metadata only"), None, 3)
        .unwrap();
    db.page_event(&page(first, "first excerpt"), Some("study.example"), 4)
        .unwrap();
    db.focus_event(first, "activewindow>>editor,first window", 4)
        .unwrap();
    db.focus_tick(first, 140).unwrap();
    assert!(
        db.review_activity(None, 140)
            .unwrap()
            .contains("first window")
    );

    db.revoke_capture(first, "windows").unwrap();
    db.focus_tick(first, 180).unwrap();
    db.focus_event(first, "activewindow>>editor,revoked", 181)
        .unwrap();
    db.study_stop(190).unwrap();
    assert!(
        db.page_event(&page(first, "after stop"), Some("study.example"), 190)
            .is_err()
    );
    let second = db
        .study_start_with_sources("second", 190, false, true)
        .unwrap();
    assert!(
        db.page_event(&page(first, "stale"), Some("study.example"), 191)
            .is_err()
    );
    db.focus_event(first, "activewindow>>editor,old worker", 191)
        .unwrap();
    db.page_event(&page(second, "second excerpt"), Some("study.example"), 192)
        .unwrap();
    db.revoke_capture(second, "browser").unwrap();
    assert!(
        db.page_event(&page(second, "revoked"), Some("study.example"), 193)
            .is_err()
    );
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
                .execute(
                    "INSERT INTO page(ts,domain,title) VALUES(12,'stale.example','stale')",
                    []
                )
                .is_err()
        );
        assert!(
            stale
                .execute(
                    "INSERT INTO focus(start,end,class,title) VALUES(12,12,'stale','stale')",
                    []
                )
                .is_err()
        );
        assert!(
            stale
                .execute("UPDATE focus SET end=20 WHERE id=1", [])
                .is_err()
        );
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
    let grant =
        |id| serde_json::json!({"type":"site_permission","capture":id,"domain":"example.invalid"});
    assert!(exchange(state.clone())["capture"].is_null());
    assert_eq!(exchange(page(1, "before"))["ok"], false);
    let id = db
        .study_start_with_sources("first", 1, false, true)
        .unwrap();
    let response = exchange(state.clone());
    assert_eq!(response["request"], 123);
    assert_eq!(response["capture"], id);
    exchange(grant(id));
    assert_eq!(exchange(page(id, "marked"))["ok"], true);
    exchange(page(id, "unmarked"));
    exchange(grant(id));
    db.study_stop(10).unwrap();
    let next = db
        .study_start_with_sources("second", 10, false, true)
        .unwrap();
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
    assert!(
        db.briefing(0, 0).unwrap()["last"].is_null(),
        "пустая база — без выдуманных данных"
    );

    let t0 = 1_800_000_000; // 08:00 UTC
    db.load(&Module::parse(MODULE).unwrap(), t0).unwrap();
    let id = db
        .focus_start_with_sources("hunt", "Анализ журналов", "Логи nginx", t0, false, false)
        .unwrap();
    db.stop_session(Some(id), t0 + 1800).unwrap();
    let it = &db.new_items(1).unwrap()[0];
    run_session(
        &db,
        &mut Cursor::new(format!("1\n{}\n", it.answer[0])),
        &mut Vec::new(),
        t0 + 2000,
        1,
    )
    .unwrap();

    // Через три дня брифинг показывает тот же день, а не пустое «вчера».
    let b = db.briefing(t0 + 3 * DAY, 0).unwrap();
    let last = &b["last"];
    assert_eq!(last["start"], t0 - 28_800);
    assert_eq!(
        last["studies"][0],
        serde_json::json!(["Логи nginx", "hunt", "Анализ журналов", 30])
    );
    assert_eq!(
        (last["answered"].as_i64(), last["correct"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(last["facts"][0][1], serde_json::json!(it.explain));
    assert_eq!(b["week"]["minutes"], 30);
    assert!(
        b["plan"]["due"].as_i64().unwrap() <= 1 && b["upcoming"]["week"].as_i64().unwrap() >= 1,
        "{b}"
    );

    // Сегодняшние занятия во «вчера» не попадают; смещение часового пояса сдвигает границу дня.
    assert!(db.briefing(t0 + 3600, 0).unwrap()["last"].is_null());
    assert_eq!(
        db.briefing(t0 + 3600, -9 * 3600).unwrap()["last"]["start"],
        t0 - 28_800 + 9 * 3600 - DAY
    );
}

#[test]
fn app_review_sends_shown_request_once_and_queues_questions() {
    let db = Db::open(":memory:").unwrap();
    let draft = ReviewDraft::new();
    assert!(draft.prepare(&db, 10).is_err());
    let id = db
        .focus_start_with_sources("ctf", "Форензика", "стек", 100, true, false)
        .unwrap();
    db.focus_event(id, "activewindow>>ghidra,main.c", 110)
        .unwrap();
    db.focus_tick(id, 400).unwrap();
    db.stop_session(Some(id), 400).unwrap();

    let shown = draft.prepare(&db, 500).unwrap();
    assert!(shown.request_json.contains("ghidra"));
    let sent = std::cell::Cell::new(false);
    let result = draft
        .send(&db, &shown.preview_id, 500, |body| {
            assert_eq!(serde_json::to_string_pretty(body).unwrap(), shown.request_json);
            sent.set(true);
            Ok(api_response(serde_json::json!([{"prompt": "[rbp+?] адрес возврата", "kind": "number", "answers": ["8"], "hint": "h", "explain": "e"}])))
        })
        .unwrap();
    assert!(sent.get());
    assert_eq!(result.count, 1);
    // Вопросы Claude сначала на просмотре; в повторения — только после принятия.
    assert!(db.new_ai_items().unwrap().is_empty());
    db.ai_draft_accept(db.ai_drafts(None).unwrap()[0].id, 500)
        .unwrap();
    assert_eq!(db.new_ai_items().unwrap().len(), 1);
    assert!(
        draft
            .send(&db, &shown.preview_id, 500, |_| panic!(
                "повторная отправка"
            ))
            .is_err()
    );

    let cancelled = draft.prepare(&db, 500).unwrap();
    draft.cancel().unwrap();
    assert!(
        draft
            .send(&db, &cancelled.preview_id, 500, |_| panic!(
                "отменённый запрос"
            ))
            .is_err()
    );
}

#[test]
fn month_activity_covers_every_day_and_counts_sessions() {
    let db = Db::open(":memory:").unwrap();
    // 2026-11-11 12:00 UTC: середина дня в любом часовом поясе от UTC-11 до UTC+11.
    let noon = 1_794_398_400;
    let id = db
        .focus_start_with_sources("ctf", "Форензика", "", noon, false, false)
        .unwrap();
    db.stop_session(Some(id), noon + 45 * 60).unwrap();
    db.focus_start_with_sources("ctf", "Форензика", "", noon + 3600, false, false)
        .unwrap();
    let days = db.month_activity(noon + 3600 + 10 * 60).unwrap();
    assert_eq!(days.len(), 30);
    assert_eq!(days[0].0, "2026-11-01");
    assert_eq!(days[10], ("2026-11-11".into(), 55, 2, 0));
    assert_eq!(days.iter().map(|d| d.2).sum::<i64>(), 2);
}

#[test]
fn estimate_against_fact() {
    let db = Db::open(tmp("estimate").join("db.sqlite").to_str().unwrap()).unwrap();
    let id = db.study_start_with_sources("тема", 0, false, false).unwrap();
    db.study_estimate_set(id, 30).unwrap();
    assert_eq!(
        db.estimate_line(id, 0, 45 * 60).unwrap(),
        "Прогноз времени: 30 мин, факт: 45 мин (+15).\n"
    );
    assert_eq!(db.estimate_line(id + 1, 0, 60).unwrap(), "");
}

/// База с одной выполненной целью: проверка наступает через 7 дней после `t0`.
fn weekly_db(name: &str) -> (Db, String, i64) {
    let db = Db::open(tmp(name).join("db.sqlite").to_str().unwrap()).unwrap();
    let t0 = 1_800_000_000;
    let arms = db.load(&Module::parse(MODULE).unwrap(), t0).unwrap();
    db.done(&arms[0].0, 30, t0).unwrap();
    (db, arms[0].0.clone(), t0)
}

#[test]
fn weekly_exit_mid_block_resumes_without_early_feedback() {
    let (db, objective, t0) = weekly_db("weekly-exit");
    let now = t0 + 7 * DAY;
    let items = db.tests_due(now).unwrap();
    assert_eq!(items.len(), 4);
    // Один верный ответ, затем закрытие ввода на втором вопросе.
    let mut out = Vec::new();
    run_test(&db, &mut Cursor::new(format!("{}\n", items[0].answer[0])), &mut out, now).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(!out.contains("══ Разбор") && !out.contains("Итог"), "ранний разбор: {out}");
    assert!(out.contains("Блок не закончен: осталось 3 из 4"), "{out}");
    // Вопрос, показанный без ответа, возвращается первым, а не пропадает.
    let left = db.tests_due(now + 60).unwrap();
    assert_eq!(left.iter().map(|i| &i.id).collect::<Vec<_>>(), items[1..].iter().map(|i| &i.id).collect::<Vec<_>>());
    // Брифинг напоминает о частичном блоке.
    let brief = db.briefing(now + 60, 0).unwrap().to_string();
    assert!(brief.contains(&db_title(&db, &objective)), "{brief}");
    // До закрытия блока ни статус, ни брифинг не выдают правильность и пояснения.
    let st = db.status(now + 60).unwrap();
    assert!(st.contains("проверка начата, осталось 3") && !st.contains("проверка: 1/1") && !st.contains("1/1 (100%)"), "{st}");
    assert!(!brief.contains(&items[0].explain), "{brief}");
    let report = db.pilot_report(now + 60).unwrap();
    let g = group(&report, &db_arm(&db, &objective));
    assert_eq!((g["first_correct"].as_i64(), g["pending_open_block"].as_i64()), (Some(0), Some(1)), "{g}");
    let ex = db.pilot_export(now + 60).unwrap();
    assert!(ex["checks"].as_array().unwrap().iter().all(|c| c["correct"].is_null()), "{ex}");
}

fn db_title(_db: &Db, objective: &str) -> String {
    MODULE_TITLES.iter().find(|(id, _)| *id == objective).map_or(objective, |t| t.1).to_string()
}
const MODULE_TITLES: [(&str, &str); 4] = [
    ("abi", "Соглашение о вызовах System V и регистры"),
    ("stack", "Стек: push/pop/call/ret и кадр функции"),
    ("trace", "Трассировка: арифметика, флаги, переходы"),
    ("overflow", "Переполнение буфера на стеке: смещения и байты"),
];

fn run_weekly(db: &Db, input: &str, now: i64) -> String {
    let mut out = Vec::new();
    run_test(db, &mut Cursor::new(input.to_owned()), &mut out, now).unwrap();
    String::from_utf8(out).unwrap()
}

fn group<'a>(report: &'a serde_json::Value, arm: &str) -> &'a serde_json::Value {
    report["groups"].as_array().unwrap().iter().find(|g| g["arm"] == arm).unwrap()
}

#[test]
fn weekly_resume_keeps_old_form_and_counts_each_answer_once() {
    let (db, objective, t0) = weekly_db("weekly-resume");
    let due = t0 + 7 * DAY;
    assert!(db.tests_due(due - 1).unwrap().is_empty(), "до срока блока нет");
    assert_eq!(db.tests_due(due).unwrap().len(), 4, "в момент срока блок доступен");
    let items = db.tests_due(due).unwrap();
    let answers: Vec<String> = items.iter().map(|i| i.answer[0].clone()).collect();
    run_weekly(&db, &format!("{}\n", answers[0]), due);
    // Модуль обновили посреди блока: у ещё не отвеченного вопроса другой ключ и версия.
    let mut m = Module::parse(MODULE).unwrap();
    let changed = m.item.iter_mut().find(|i| i.id == items[1].id).unwrap();
    changed.answer = vec!["изменён".into()];
    changed.v += 1;
    db.load(&m, due + 10).unwrap();
    db.load(&m, due + 10).unwrap();
    let out = run_weekly(&db, &answers[1..].iter().map(|a| format!("{a}\n")).collect::<String>(), due + 60);
    assert!(out.contains("Продолжаем начатый блок: осталось 3 из 4"), "{out}");
    assert!(out.contains("Итог: 4/4."), "старая форма и ключ сохранены: {out}");
    assert_eq!(out.matches("══ Разбор").count(), 1, "{out}");
    assert!(run_weekly(&db, "x\n", due + 120).contains("Проверок пока нет"), "блок закрыт, повторный запуск ничего не пишет");

    let report = db.pilot_report(due + 120).unwrap();
    let g = group(&report, &db_arm(&db, &objective));
    assert_eq!((g["first_answers"].as_i64(), g["first_correct"].as_i64()), (Some(3), Some(3)));
    assert_eq!((g["repeat_answers"].as_i64(), g["reshows"].as_i64()), (Some(1), Some(1)));
    assert_eq!((g["late"].as_i64(), g["missed"].as_i64(), g["shown_unanswered"].as_i64()), (Some(0), Some(0), Some(0)));
}

fn db_arm(db: &Db, objective: &str) -> String {
    let report = db.pilot_export(0).unwrap();
    report["checks"].as_array().unwrap().iter().find(|c| c["objective"] == objective).unwrap()["arm"].as_str().unwrap().to_owned()
}

#[test]
fn weekly_late_answer_and_missed_check_are_counted_separately() {
    let (db, objective, t0) = weekly_db("weekly-late");
    let other = ["abi", "stack", "trace", "overflow"].into_iter().find(|id| *id != objective).unwrap();
    db.done(other, 20, t0).unwrap();
    let after_window = t0 + 7 * DAY + 3 * DAY;
    // Блок первой цели открыт после окна допуска: ответы засчитываются как опоздания.
    let items = db.tests_due(after_window).unwrap();
    let first_objective = items[0].objective.clone();
    let mine: String = items.iter().filter(|i| i.objective == first_objective).map(|i| format!("{}\n", i.answer[0])).collect();
    let out = run_weekly(&db, &mine, after_window);
    assert!(out.contains("Итог: 4/4."), "{out}");
    // Вторая цель осталась не открытой: после окна это пропуск, а не ответ.
    let report = db.pilot_report(after_window).unwrap();
    let total = |key: &str| report["groups"].as_array().unwrap().iter().map(|g| g[key].as_i64().unwrap()).sum::<i64>();
    assert_eq!((total("first_answers"), total("late"), total("missed")), (0, 4, 4));
    assert!(db.tests_due(after_window).unwrap().len() == 4, "пропущенная цель всё ещё доступна для ответа вне зачёта");
}

#[test]
fn weekly_second_client_cannot_double_count_and_export_is_anonymous() {
    let dir = tmp("weekly-two");
    let path = dir.join("db.sqlite");
    let a = Db::open(path.to_str().unwrap()).unwrap();
    let t0 = 1_800_000_000;
    let arms = a.load(&Module::parse(MODULE).unwrap(), t0).unwrap();
    a.done(&arms[0].0, 30, t0).unwrap();
    let due = t0 + 7 * DAY;
    let answers: String = a.tests_due(due).unwrap().iter().map(|i| format!("{}\n", i.answer[0])).collect();
    let b = Db::open(path.to_str().unwrap()).unwrap();
    // Клиент A открыл блок и вышел после первого ответа; клиент B продолжает тот же блок.
    run_weekly(&a, answers.lines().next().unwrap(), due);
    let rest: String = answers.lines().skip(1).map(|l| format!("{l}\n")).collect();
    assert!(run_weekly(&b, &rest, due + 5).contains("Итог: 4/4."));
    assert!(run_weekly(&a, &answers, due + 6).contains("Проверок пока нет"));
    let export = a.pilot_export(due + 6).unwrap();
    assert_eq!(export["checks"].as_array().unwrap().len(), 4, "по одной записи на вопрос, без дублей");
    let text = export.to_string();
    for forbidden in ["form", "answer\"", "prompt", "explain", "hint", "rdi"] {
        assert!(!text.contains(forbidden), "в выгрузке лишнее поле {forbidden}: {text}");
    }
    let report = a.pilot_text(due + 6).unwrap();
    assert!(report.contains("первые ответы в окне: 3/3 верно") || report.contains("4/4"), "{report}");
}

const LOGS: &str = include_str!("../../modules/logs-defense.toml");

/// Синтетическая выборка на модуле журналов; группы заданы явно: auth → wr, web → usual.
/// Ожидаемые числа посчитаны вручную, сценарий и пересчёт — в комментариях.
#[test]
fn pilot_report_matches_hand_count() {
    let dir = tmp("pilot-hand");
    let path = dir.join("db.sqlite");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let t0 = 1_800_000_000;
    let s = t0 - 10 * DAY;
    db.load(&Module::parse(LOGS).unwrap(), s).unwrap();
    let sql = rusqlite::Connection::open(&path).unwrap();
    sql.execute_batch("UPDATE objective SET arm='wr' WHERE id='auth'; UPDATE objective SET arm='usual' WHERE id='web';")
        .unwrap();

    // Сессия 1 (s): p1 — верно самостоятельно, p2 — «?» и верно (облегчённый ответ).
    let items = db.new_items(2).unwrap();
    let (p1, p2) = (&items[0], &items[1]);
    assert_eq!((p1.id.as_str(), p2.id.as_str()), ("auth-p1", "auth-p2"));
    let out = run_session_text(&db, &format!("1\n{}\n?\n{}\n", p1.answer[0], p2.answer[0]), s, 2);
    assert!(out.contains("Итог: 1/2, прогноз был 1.") && out.contains("С подсказкой: 1"), "{out}");
    // FSRS: ответ после подсказки — практика, срок как у промаха (Again): завтра вернётся только p2.
    assert_eq!(ids(&db.due_reviews(s + DAY).unwrap()), ["auth-p2"]);

    // Сессия 2 (s+5д): повторения p2 (раньше срок), затем p1. p2 — верно, p1 — промах, повтор p1 верен.
    let s2 = s + 5 * DAY;
    assert_eq!(ids(&db.due_reviews(s2).unwrap()), ["auth-p2", "auth-p1"]);
    let out = run_session_text(&db, &format!("1\n{}\nневерно\n{}\n", p2.answer[0], p1.answer[0]), s2, 0);
    assert!(out.contains("Итог: 1/2, прогноз был 1."), "{out}");
    // Самостоятельный промах остаётся промахом, хотя повтор верен: p1 (Again) вернётся раньше p2 (Good).
    assert_eq!(ids(&db.due_reviews(s2 + DAY).unwrap())[0], "auth-p1");

    db.done("auth", 30, t0).unwrap();
    // web завершена на 2 ч позже: её блок не откроется во время второго запуска auth.
    db.done("web", 45, t0 + 7200).unwrap();
    let due = t0 + 7 * DAY;
    let window = 2 * DAY;
    let key = |id: &str| db.item(id).unwrap().answer[0].clone();

    // auth (wr), срок due: t1 верно, t2 неверно, на t3 выход (показан). Через час t3 верно после повторного показа.
    run_weekly(&db, &format!("{}\nневерно\n", key("auth-t1")), due);
    let out = run_weekly(&db, &format!("{}\n", key("auth-t3")), due + 3600);
    assert!(out.contains("Итог: 2/3."), "{out}");
    // web (usual), срок wdue: t1 верно ровно на границе окна (в окне); t2 — выход (показан).
    // Через секунду после окна: t2 верно (опоздание), на t3 выход — показан, без ответа, окно прошло.
    let wdue = due + 7200;
    run_weekly(&db, &format!("{}\n", key("web-t1")), wdue + window);
    run_weekly(&db, &format!("{}\n", key("web-t2")), wdue + window + 1);

    let report = db.pilot_report(due + 3 * DAY).unwrap();
    let wr = group(&report, "wr");
    // wr: первые ответы в окне — t1 (верно), t2 (неверно) → 1/2; t3 после повторного показа → repeat 1,
    // reshows 1 (t3 показан дважды); задержка (0 + 0 + 3600 с)/3 = 1200 с = 0,33 ч → 0.3.
    // Вспоминание: review p2 верно, review p1 неверно → 1/2; retry и new не входят; с подсказкой — 1 (new p2).
    for (k, v) in [
        ("goals", 1), ("goals_done", 1), ("planned_checks", 3), ("first_answers", 2), ("first_correct", 1),
        ("late", 0), ("repeat_answers", 1), ("reshows", 1), ("shown_unanswered", 0), ("missed", 0),
        ("recall_independent", 2), ("recall_correct", 1), ("helped_answers", 1), ("study_minutes_outside", 30),
        ("incomplete_legacy_answers", 0),
    ] {
        assert_eq!(wr[k].as_i64(), Some(v), "wr {k}: {wr}");
    }
    assert_eq!(wr["avg_delay_hours"].as_f64(), Some(0.3));
    let us = group(&report, "usual");
    // usual: t1 на границе (wdue+2д) → первый в окне, 1/1; t2 через секунду после окна → late 1 (не repeat);
    // reshows: t2 показан дважды → 1 (t3 показан один раз); t3 без ответа после окна → missed 1, shown_unanswered 1.
    // Задержка (172800 + 172801)/2 с = 48,0001 ч → 48.
    for (k, v) in [
        ("planned_checks", 3), ("first_answers", 1), ("first_correct", 1), ("late", 1), ("repeat_answers", 0),
        ("reshows", 1), ("shown_unanswered", 1), ("missed", 1), ("recall_independent", 0), ("helped_answers", 0),
        ("study_minutes_outside", 45),
    ] {
        assert_eq!(us[k].as_i64(), Some(v), "usual {k}: {us}");
    }
    assert_eq!(us["avg_delay_hours"].as_f64(), Some(48.0));
    let text = db.pilot_text(due + 3 * DAY).unwrap();
    assert!(text.contains("первые ответы в окне: 1/2 верно") && text.contains("вспоминание (самостоятельное): 1/2"), "{text}");

    // Калибровка: прогноз 1+1, самостоятельно верных 1+1, набор 4 вопроса, с подсказкой 1.
    let st = db.status(due + 3 * DAY).unwrap();
    assert!(st.contains("прогноз 2, факт 2 — точно. Вопросов 4, с подсказкой 1") && st.contains("90%"), "{st}");
    assert!(st.contains("проверка начата, осталось 1"), "у web незакрытый блок: {st}");

    // Выгрузка: 6 строк проверок, 2 цели, 5 практических попыток, 2 сессии; без ключей и текстов.
    let ex = db.pilot_export(due + 3 * DAY).unwrap();
    let len = |k: &str| ex[k].as_array().unwrap().len();
    assert_eq!((len("checks"), len("objectives"), len("practice"), len("sessions")), (6, 2, 5, 2));
    let answered = ex["checks"].as_array().unwrap().iter().filter(|c| !c["answered_at"].is_null()).count();
    assert_eq!(answered, 5, "все ответы, кроме web-t3");
    let raw = ex.to_string();
    for bad in ["203.0.113", "198.51.100", "backup", "обход", "Failed", "prompt", "explain", "form\"", "answer\""] {
        assert!(!raw.contains(bad), "в выгрузке лишнее «{bad}»: {raw}");
    }
}

fn run_session_text(db: &Db, input: &str, now: i64, new_n: usize) -> String {
    let mut out = Vec::new();
    run_session(db, &mut Cursor::new(input.to_owned()), &mut out, now, new_n).unwrap();
    String::from_utf8(out).unwrap()
}

fn ids(items: &[Item]) -> Vec<String> {
    items.iter().map(|i| i.id.clone()).collect()
}

/// API для GUI: показ до выдачи, ключ только после закрытия блока, дубль отправки со второго клиента отклоняется.
#[test]
fn weekly_api_hides_keys_and_two_clients_record_one_answer() {
    let dir = tmp("weekly-api");
    let path = dir.join("db.sqlite");
    let a = Db::open(path.to_str().unwrap()).unwrap();
    let t0 = 1_800_000_000;
    a.load(&Module::parse(LOGS).unwrap(), t0).unwrap();
    a.done("auth", 10, t0).unwrap();
    let due = t0 + 7 * DAY;
    let b = Db::open(path.to_str().unwrap()).unwrap();
    assert!(a.test_current(due - 1).unwrap().is_null(), "до срока вопросов нет");
    let q = a.test_current(due).unwrap();
    let (block, id) = (q["block"].as_i64().unwrap(), q["item"]["id"].as_str().unwrap().to_owned());
    assert_eq!((q["left"].as_i64(), q["total"].as_i64(), q["resumed"].as_bool()), (Some(3), Some(3), Some(false)));
    let shown = q.to_string();
    assert!(!shown.contains("explain") && !shown.contains("answer") && !shown.contains("hint"), "{shown}");
    assert!(a.test_submit(block, "auth-t3", "backup", 0, due).is_err(), "не показанный вопрос не принимается");

    let key = a.item(&id).unwrap().answer[0].clone();
    let r = a.test_submit(block, &id, &key, 1000, due + 1).unwrap();
    assert_eq!(r, serde_json::json!({"closed": false, "left": 2, "total": 3}), "до закрытия правильность не видна");
    // Второй клиент со старым экраном отправляет тот же ответ — ошибка, запись одна.
    assert!(b.test_submit(block, &id, &key, 1000, due + 2).is_err());
    assert_eq!(a.export().unwrap()["attempt"].as_array().unwrap().len(), 1);

    // Клиент B продолжает блок; закрытие выдаёт разбор с ключами.
    let mut last = serde_json::Value::Null;
    while let Some(q) = Some(b.test_current(due + 10).unwrap()).filter(|q| !q.is_null()) {
        assert_eq!(q["resumed"].as_bool(), Some(true));
        let id = q["item"]["id"].as_str().unwrap();
        last = b.test_submit(q["block"].as_i64().unwrap(), id, "неверно", 0, due + 11).unwrap();
    }
    assert_eq!((last["closed"].as_bool(), last["correct"].as_i64(), last["total"].as_i64()), (Some(true), Some(1), Some(3)));
    assert!(last["review"].as_array().unwrap().iter().all(|r| r["key"].is_string() && r["explain"].is_string()));
    assert_eq!(a.export().unwrap()["attempt"].as_array().unwrap().len(), 3);
}

/// Ответы старой версии (test7 без записи показа) помечены неполными, не задаются повторно, не теряются.
#[test]
fn weekly_legacy_answers_are_incomplete_and_kept() {
    let path = tmp("weekly-legacy").join("db.sqlite");
    let db = Db::open(path.to_str().unwrap()).unwrap();
    let t0 = 1_800_000_000;
    let objective = db.load(&Module::parse(MODULE).unwrap(), t0).unwrap()[0].0.clone();
    db.done(&objective, 30, t0).unwrap();
    let due = t0 + 7 * DAY;
    // Так писала старая `run_test`: попытка test7 без test_exposure, а затем EOF.
    let old = db.tests_due(due).unwrap()[0].clone();
    db.record(None, &old, "test7", &old.answer[0], true, false, 0, due).unwrap();
    let left = db.tests_due(due).unwrap();
    assert_eq!(left.len(), 3);
    assert!(left.iter().all(|i| i.id != old.id), "отвеченный в старой версии вопрос не задаётся снова");
    let input: String = left.iter().map(|i| format!("{}\n", i.answer[0])).collect();
    assert!(run_weekly(&db, &input, due).contains("Итог: 3/3."));
    let report = db.pilot_report(due).unwrap();
    let g = group(&report, &db_arm(&db, &objective));
    assert_eq!((g["incomplete_legacy_answers"].as_i64(), g["first_answers"].as_i64()), (Some(1), Some(3)));
    // Повторное открытие базы (миграция) сохраняет все попытки.
    drop(db);
    let db = Db::open(path.to_str().unwrap()).unwrap();
    assert_eq!(db.export().unwrap()["attempt"].as_array().unwrap().len(), 4);
    assert_eq!(group(&db.pilot_report(due).unwrap(), &db_arm(&db, &objective))["incomplete_legacy_answers"].as_i64(), Some(1));
}

/// Пока недельный блок открыт, сессия повторений и очередь с подсказками недоступны.
#[test]
fn review_session_is_refused_while_weekly_block_is_open() {
    let (db, _, t0) = weekly_db("weekly-help");
    let due = t0 + 7 * DAY;
    let items = db.tests_due(due).unwrap();
    run_weekly(&db, &format!("{}\n", items[0].answer[0]), due);
    let err = run_session(&db, &mut Cursor::new("1\n"), &mut Vec::new(), due, 4).unwrap_err();
    assert!(err.contains("Идёт самостоятельная проверка"), "{err}");
    assert!(db.new_ai_items().is_err() && db.session_begin(due, 1, 1).is_err(), "очередь для Tauri тоже закрыта");
    let rest: String = items[1..].iter().map(|i| format!("{}\n", i.answer[0])).collect();
    assert!(run_weekly(&db, &rest, due).contains("Итог: 4/4."));
    assert!(run_session(&db, &mut Cursor::new(""), &mut Vec::new(), due, 4).is_ok());
    assert!(db.new_ai_items().is_ok() && db.session_begin(due, 1, 1).is_ok());
}
