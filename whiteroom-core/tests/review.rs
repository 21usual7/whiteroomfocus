//! Разбор сессии (D17), просмотр вопросов Claude, подсказки в практике и маршрут по журналам.
//! Claude — транспорт с фиксированным ответом, сети нет.
use std::cell::Cell;
use std::collections::HashMap;
use whiteroom_core::ai_context::{self, DraftEdit, SOURCE_UNKNOWN};
use whiteroom_core::challenge::{Challenge, Ready};
use whiteroom_core::tutor::{self, Tutor};
use whiteroom_core::{DAY, Db, Module, ReviewDraft};

const LOGS: &str = include_str!("../../modules/log-analysis.toml");

fn reply(questions: serde_json::Value) -> serde_json::Value {
    let text =
        serde_json::json!({"summary": "Сессия: стек x86-64.", "questions": questions}).to_string();
    serde_json::json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": text}]})
}

fn q(prompt: &str, kind: &str, answer: &str, source: &str) -> serde_json::Value {
    serde_json::json!({"prompt": prompt, "kind": kind, "answers": [answer], "hint": "h", "explain": "e", "source": source})
}

/// Завершённая сессия с записью окон.
fn session(db: &Db, title: &str, start: i64) -> i64 {
    let id = db
        .focus_start_with_sources("ctf", "Форензика", "стек", start, true, false)
        .unwrap();
    db.focus_event(id, &format!("activewindow>>{title},main.c"), start + 10)
        .unwrap();
    db.focus_tick(id, start + 300).unwrap();
    db.stop_session(Some(id), start + 300).unwrap();
    id
}

#[test]
fn empty_claude_result_stages_nothing() {
    let db = Db::open(":memory:").unwrap();
    session(&db, "ghidra", 100);
    let draft = ReviewDraft::new();
    let shown = draft.prepare(&db, 500).unwrap();
    let result = draft
        .send(&db, &shown.preview_id, 500, |_| {
            Ok(reply(serde_json::json!([])))
        })
        .unwrap();
    assert_eq!(result.count, 0);
    assert!(result.summary.contains("стек"));
    assert!(db.ai_drafts(None).unwrap().is_empty());
    assert!(db.new_ai_items().unwrap().is_empty());
}

#[test]
fn claude_questions_are_reviewed_edited_rejected_and_disputed_with_history() {
    let db = Db::open(":memory:").unwrap();
    let study = session(&db, "ghidra", 100);
    let draft = ReviewDraft::new();
    let selected =
        ai_context::selected_text("Адрес возврата лежит по [rbp+8] после пролога.").unwrap();
    let shown = draft.prepare_for(&db, None, &[selected]).unwrap();
    assert_eq!(shown.session_id, study);
    assert!(shown.request_json.contains("выделенный текст"));
    let result = draft
        .send(&db, &shown.preview_id, 500, |_| {
            Ok(reply(serde_json::json!([
                q(
                    "Смещение адреса возврата от rbp?",
                    "number",
                    "16",
                    "лежит по [rbp+8] после"
                ),
                q(
                    "Регистр первого аргумента?",
                    "text",
                    "rdi",
                    "цитата, которой не было в запросе"
                ),
                q("Что делает leave?", "text", "mov rsp, rbp; pop rbp", "")
            ])))
        })
        .unwrap();
    assert_eq!((result.count, result.session_id), (3, study));
    assert!(
        db.new_ai_items().unwrap().is_empty(),
        "до принятия в повторения ничего не попадает"
    );

    let drafts = db.ai_drafts(None).unwrap();
    assert_eq!(drafts[0].source.as_deref(), Some("лежит по [rbp+8] после"));
    assert_eq!(
        drafts[1].source_label, SOURCE_UNKNOWN,
        "цитаты нет в отправленном тексте"
    );
    assert_eq!(drafts[2].source_label, SOURCE_UNKNOWN);
    let json = serde_json::to_value(&drafts[0]).unwrap();
    for key in [
        "id",
        "sessionId",
        "status",
        "prompt",
        "answers",
        "explain",
        "sourceLabel",
    ] {
        assert!(json.get(key).is_some(), "{key}");
    }

    // Спорный ключ: исправить до добавления. Число проверяется, как у ответа Claude.
    let mut edit = DraftEdit {
        prompt: drafts[0].prompt.clone(),
        kind: "number".into(),
        answers: vec!["восемь".into()],
        hint: None,
        explain: "Над сохранённым rbp лежит адрес возврата.".into(),
    };
    assert!(db.ai_draft_edit(drafts[0].id, &edit).is_err());
    edit.answers = vec!["8".into()];
    db.ai_draft_edit(drafts[0].id, &edit).unwrap();
    let item = db.ai_draft_accept(drafts[0].id, 600).unwrap();
    assert!(
        db.ai_draft_edit(drafts[0].id, &edit).is_err(),
        "после принятия правка — только через «ошибку»"
    );

    db.ai_draft_reject(drafts[1].id).unwrap();
    assert!(db.ai_draft_accept(drafts[1].id, 600).is_err());
    assert!(db.ai_draft_reject(drafts[1].id).is_err());

    // Третий остаётся на просмотре в терминале: EOF не принимает его молча.
    let mut out = Vec::new();
    ai_context::run_drafts(&db, &mut std::io::Cursor::new(""), &mut out, 600).unwrap();
    assert!(
        String::from_utf8(out)
            .unwrap()
            .contains("связь с источником неизвестна")
    );
    assert_eq!(db.ai_drafts(None).unwrap().len(), 1);

    let queued = db.new_ai_items().unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(
        (
            queued[0].id.as_str(),
            queued[0].role.as_str(),
            queued[0].objective.as_str()
        ),
        (item.as_str(), "practice", "ai")
    );
    assert_eq!(queued[0].answer, ["8"]);
    db.record(None, &queued[0], "new", "16", false, false, 900, 700)
        .unwrap();
    assert_eq!(db.due_reviews(700 + 2 * DAY).unwrap().len(), 1);

    db.ai_item_dispute(&item, "в x86-64 смещение 8", 800)
        .unwrap();
    assert!(db.due_reviews(700 + 60 * DAY).unwrap().is_empty());
    assert!(db.new_ai_items().unwrap().is_empty());
    let export = db.export().unwrap();
    assert_eq!(
        export["attempt"].as_array().unwrap().len(),
        1,
        "история попыток сохранена"
    );
    assert!(db.status(800).unwrap().contains("исключены из оценки: 1"));
    let session_drafts = db.ai_drafts(Some(study)).unwrap();
    assert_eq!(
        session_drafts
            .iter()
            .map(|d| d.status.as_str())
            .collect::<Vec<_>>(),
        ["disputed", "rejected", "pending"]
    );
    assert!(
        db.ai_item_dispute("abi-p1", "", 800).is_err(),
        "только вопросы Claude"
    );
}

#[test]
fn preview_is_bound_to_its_finished_session_snapshot_and_sent_once() {
    let db = Db::open(":memory:").unwrap();
    let draft = ReviewDraft::new();
    let calls = Cell::new(0);
    let fixture = |body: &serde_json::Value| {
        calls.set(calls.get() + 1);
        assert!(body.to_string().contains("ghidra"));
        Ok(reply(serde_json::json!([q(
            "Регистр первого аргумента?",
            "text",
            "rdi",
            ""
        )])))
    };

    let running = db
        .focus_start_with_sources("ctf", "Форензика", "стек", 50, true, false)
        .unwrap();
    assert!(
        draft.prepare(&db, 60).is_err(),
        "идущая сессия не разбирается"
    );
    db.stop_session(Some(running), 60).unwrap();
    let first = session(&db, "ghidra", 100);
    let shown = draft.prepare(&db, 500).unwrap();
    assert_eq!(shown.session_id, first);

    // Отмена: ни одного вызова транспорта.
    draft.cancel().unwrap();
    assert!(draft.send(&db, &shown.preview_id, 500, fixture).is_err());
    assert_eq!(calls.get(), 0);

    // Смена сессии между предпросмотром и отправкой: уходит снимок первой, вопросы — к первой.
    let shown = draft.prepare(&db, 500).unwrap();
    let second = session(&db, "firefox", 1000);
    let result = draft.send(&db, &shown.preview_id, 1400, fixture).unwrap();
    assert_eq!((result.session_id, calls.get()), (first, 1));
    assert_eq!(db.ai_drafts(Some(first)).unwrap().len(), 1);
    assert!(db.ai_drafts(Some(second)).unwrap().is_empty());

    // Повторная отправка того же предпросмотра — ошибка без вызова.
    assert!(draft.send(&db, &shown.preview_id, 1400, fixture).is_err());
    assert_eq!(calls.get(), 1);

    // Ошибка транспорта тоже расходует предпросмотр: повтор — только после нового.
    let failed = draft.prepare_for(&db, Some(first), &[]).unwrap();
    assert!(
        draft
            .send(&db, &failed.preview_id, 1400, |_| Err("сеть".into()))
            .is_err()
    );
    assert!(draft.send(&db, &failed.preview_id, 1400, fixture).is_err());
    assert_eq!(calls.get(), 1);
    assert!(
        !draft
            .prepare(&db, 1500)
            .unwrap()
            .request_json
            .contains("ghidra"),
        "по умолчанию — последняя завершённая"
    );
}

#[test]
fn review_without_capture_uses_only_explicit_text_or_files() {
    let db = Db::open(":memory:").unwrap();
    let id = db
        .focus_start_with_sources("ctf", "Форензика", "стек", 100, false, false)
        .unwrap();
    let _ = db.focus_event(id, "activewindow>>ghidra,main.c", 110); // запись окон выключена
    db.stop_session(Some(id), 400).unwrap();
    let draft = ReviewDraft::new();
    assert!(
        draft.prepare(&db, 500).is_err(),
        "без записи и вложений отправлять нечего"
    );

    let dir = std::env::temp_dir().join(format!("whiteroom-review-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("конспект.md");
    std::fs::write(&file, "Пролог: push rbp; mov rbp, rsp.").unwrap();
    let files = ai_context::read_attachments(&[file.to_str().unwrap()]).unwrap();
    let shown = draft.prepare_for(&db, Some(id), &files).unwrap();
    assert!(
        shown.request_json.contains("Файл конспект.md") && shown.request_json.contains("push rbp")
    );
    assert!(!shown.request_json.contains("ghidra"));
    assert!(ai_context::selected_text("  ").is_err());
    assert!(
        ai_context::read_attachments(&["/etc/hostname"]).is_err(),
        "только .txt и .md"
    );
}

fn pwn(db: &Db, flag: &str) -> i64 {
    let ch: Challenge = serde_json::from_str(include_str!(
        "../../challenges/examples/pwn-admin-flag.json"
    ))
    .unwrap();
    let ready = Ready {
        ch,
        binary: Some(Vec::new()),
        flag: Some(flag.into()),
    };
    db.add_challenge(&ready, 100).unwrap()
}

#[test]
fn hint_level_is_kept_per_attempt_and_assisted_results_are_not_independent() {
    let db = Db::open(":memory:").unwrap();
    let (miss, helped, clean) = (pwn(&db, "WR{a}"), pwn(&db, "WR{b}"), pwn(&db, "WR{c}"));
    assert!(!db.solve(miss, "WR{?}", 101).unwrap());
    db.next_hint(miss).unwrap();
    assert!(db.solve(miss, "WR{a}", 102).unwrap());
    db.next_hint(helped).unwrap();
    db.next_hint(helped).unwrap();
    assert!(db.solve(helped, "WR{b}", 103).unwrap());
    assert!(db.solve(clean, "WR{c}", 104).unwrap());

    let levels: Vec<_> = db.export().unwrap()["challenge_attempt"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["hints"].as_i64().unwrap())
        .collect();
    assert_eq!(levels, [0, 1, 2, 0]);
    let independent: Vec<_> = [miss, helped, clean]
        .iter()
        .map(|&id| db.challenge(id).unwrap().independent)
        .collect();
    // Промах без помощи остаётся промахом, решение после подсказки — не самостоятельное.
    assert_eq!(independent, [Some(false), None, Some(true)]);
}

#[test]
fn self_test_block_closes_tutor_hints_keys_and_review() {
    let db = Db::open(":memory:").unwrap();
    db.load(&Module::parse(LOGS).unwrap(), 0).unwrap();
    for objective in ["logs-auth", "logs-method", "logs-web"] {
        db.done(objective, 0, 0).unwrap();
    }
    session(&db, "ghidra", 100);
    let challenge = pwn(&db, "WR{x}");
    let tutor = Tutor::new();
    let now = 8 * DAY;
    let shown = db.test_current(now).unwrap();
    assert!(db.self_test_active().unwrap());

    assert!(tutor::context(&db).is_err());
    assert!(tutor.prepare(&db, None, None, Some("logs")).is_err());
    assert!(tutor.prepare_claude(&db, None, "Что такое хеш?").is_err());
    assert!(db.next_hint(challenge).is_err());
    assert!(ReviewDraft::new().prepare(&db, now).is_err());
    assert!(
        db.ai_drafts(None)
            .unwrap_err()
            .contains("самостоятельная проверка")
    );
    // Wayland popup: окно не открывается, а его tutor-stdio идёт через те же функции.
    let path = std::env::temp_dir().join(format!("whiteroom-overlay-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let disk = Db::open(path.to_str().unwrap()).unwrap();
    disk.load(&Module::parse(LOGS).unwrap(), 0).unwrap();
    disk.done("logs-auth", 0, 0).unwrap();
    disk.test_current(now).unwrap();
    let error =
        whiteroom_core::overlay::open(path.to_str().unwrap(), std::path::Path::new("/nonexistent"));
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        assert!(error.unwrap_err().contains("самостоятельная проверка"));
    }

    // После последнего ответа блока помощь снова доступна.
    let (mut block, mut item) = (
        shown["block"].as_i64().unwrap(),
        shown["item"]["id"].as_str().unwrap().to_string(),
    );
    loop {
        let r = db.test_submit(block, &item, "1", 1000, now).unwrap();
        if r["closed"] == true {
            break;
        }
        let next = db.test_current(now).unwrap();
        (block, item) = (
            next["block"].as_i64().unwrap(),
            next["item"]["id"].as_str().unwrap().into(),
        );
    }
    assert!(!db.self_test_active().unwrap());
    assert!(tutor::context(&db).is_ok() && db.next_hint(challenge).is_ok());
}

/// Ключи маршрута сверяются с журналами: число — строки с признаком и адресом из условия,
/// адрес — единственный лидер по числу строк с признаком.
#[test]
fn log_route_keys_match_the_logs_and_steps_are_ordered() {
    let m = Module::parse(LOGS).unwrap();
    for objective in ["logs-auth", "logs-web"] {
        let route: Vec<_> = m
            .item
            .iter()
            .filter(|i| i.objective == objective && i.role == "practice")
            .map(|i| i.id.rsplit('-').next().unwrap())
            .collect();
        assert_eq!(route, ["p1", "p2", "p3", "p4", "p5"], "{objective}");
    }
    for o in &m.objective {
        assert!(
            m.item
                .iter()
                .any(|i| i.objective == o.id && i.role == "test7"),
            "{}",
            o.id
        );
    }
    let mut checked = 0;
    for it in m.item.iter().filter(|i| i.code.is_some()) {
        let code = it.code.as_deref().unwrap();
        let marker = if it.prompt.contains("парол") || it.prompt.contains("вход") {
            "Failed password"
        } else {
            "\" 404 "
        };
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for line in code.lines().filter(|l| l.contains(marker)) {
            let ip = if marker == "Failed password" {
                line.rsplit(' ').next()
            } else {
                line.split(' ').next()
            };
            *counts.entry(ip.unwrap()).or_default() += 1;
        }
        if it.kind == "number" {
            let ip = it
                .prompt
                .split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .find(|w| w.matches('.').count() == 3)
                .expect(&it.id);
            assert_eq!(counts[ip].to_string(), it.answer[0], "{}", it.id);
            checked += 1;
        } else if it.answer[0].matches('.').count() == 3 {
            let best = counts[it.answer[0].as_str()];
            assert!(
                counts
                    .iter()
                    .all(|(ip, &n)| *ip == it.answer[0] || n < best),
                "{}",
                it.id
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 13);
}
