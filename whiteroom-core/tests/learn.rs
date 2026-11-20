//! WHITE ROOM 1.0: цель → план → блок → возобновление → итог → проверка; миграция базы MVP.
use whiteroom_core::learn::{self, GoalEdit, Material, Step};
use whiteroom_core::{DAY, Db, Module, SCHEMA};

const T0: i64 = 1_800_000_000;

fn tmp() -> (tempdir::Dir, String) {
    let d = tempdir::Dir::new();
    let p = d.0.join("w.db").to_string_lossy().to_string();
    (d, p)
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Dir {
        pub fn new() -> Dir {
            let p = std::env::temp_dir().join(format!("wr-learn-{}-{}", std::process::id(), rand_id()));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn rand_id() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::SeqCst)
    }
}

fn logs() -> Module {
    Module::parse(&std::fs::read_to_string("../modules/log-analysis.toml").unwrap()).unwrap()
}

fn key(db: &Db, item: &str) -> String {
    db.item(item).unwrap().answer[0].clone()
}

#[test]
fn clean_db_goal_plan_block_resume_and_close() {
    let (_d, p) = tmp();
    let db = Db::open(&p).unwrap();
    let t = db.today(T0, 0).unwrap();
    assert_eq!(t["empty"], true);
    let g = db.goal_create(&logs(), T0).unwrap();
    // Пилотные группы не трогаются: цели примера — в группе goal.
    assert!(db.pilot_report(T0).unwrap()["groups"].as_array().unwrap().iter().all(|g| g["arm"] != "wr" && g["arm"] != "usual"));
    let plan = db.plan_build(g, T0).unwrap();
    assert_eq!(plan, db.plan_build(g, T0).unwrap(), "план воспроизводим");
    assert_eq!(plan[0].kind, "diag");
    assert!(plan.iter().all(|s| !s.why.is_empty()));
    let b = db.block_start(g, Some(3), T0).unwrap();
    assert_eq!(db.block_start(g, None, T0).unwrap(), b, "второй клиент получает тот же блок");
    // Диагностика: auth верно, web неверно → для auth разобранный пример уходит из плана.
    let r = db.block_answer(b, 0, &key(&db, "logs-auth-d1"), None, 1000, T0).unwrap();
    assert_eq!(r["ok"], true);
    assert!(db.block_hint(b, 1).is_err(), "в диагностике подсказок нет");
    db.block_answer(b, 1, "0", None, 1000, T0).unwrap();
    let st = db.block_state(b).unwrap();
    let items: Vec<&str> = st["steps"].as_array().unwrap().iter().filter_map(|s| s["item"].as_str()).collect();
    assert!(!items.contains(&"logs-auth-p1") && !items.contains(&"logs-auth-p2"), "{items:?}");
    assert!(items.contains(&"logs-web-p1"), "новичку в web — разобранный пример");
    assert!(items.iter().position(|i| *i == "logs-method-p1") > items.iter().position(|i| *i == "logs-web-p1"), "предпосылки раньше");
    // Двойной клик и второй клиент: одна попытка.
    let step = st["step"].as_i64().unwrap();
    let item = st["item"]["id"].as_str().unwrap().to_string();
    let db2 = Db::open(&p).unwrap();
    let a1 = db.block_answer(b, step, &key(&db, &item), None, 500, T0 + 60).unwrap();
    let a2 = db2.block_answer(b, step, "чужой ответ", None, 500, T0 + 61).unwrap();
    assert_eq!(a1["ok"], a2["ok"]);
    assert_eq!(db.export().unwrap()["attempt"].as_array().unwrap().iter().filter(|a| a["item_id"] == item.as_str()).count(), 1);
    // Подсказка запоминается в блоке: ответ после неё — не самостоятельный, в любом клиенте.
    let st = db2.block_state(b).unwrap();
    let step = st["step"].as_i64().unwrap();
    let item = st["item"]["id"].as_str().unwrap().to_string();
    if st["item"]["has_hint"] == true {
        db2.block_hint(b, step).unwrap();
        let r = db.block_answer(b, step, &key(&db, &item), None, 500, T0 + 70).unwrap();
        assert_eq!(r["helped"], true);
    } else {
        db.block_skip(b, step, T0 + 70).unwrap();
    }
    db.block_note(b, "остановился на веб-журнале", T0 + 80).unwrap();
    db.block_thought(b, "проверить права на /var/log", T0 + 81).unwrap();
    // «Закрыли окно и перезапустили»: новое соединение видит тот же шаг, заметку и мысль.
    drop(db);
    drop(db2);
    let db = Db::open(&p).unwrap();
    let t = db.today(T0 + 3600, 0).unwrap();
    assert_eq!(t["actions"][0]["kind"], "block");
    assert!(t["actions"][0]["reason"].as_str().unwrap().contains("веб-журнале"));
    let st = db.block_state(b).unwrap();
    assert_eq!(st["note"], "остановился на веб-журнале");
    assert_eq!(st["thoughts"][0]["text"], "проверить права на /var/log");
    // Правка плана: пройденное неизменно, перерыв можно убрать, порядок — поменять.
    let cur = st["step"].as_i64().unwrap() as usize;
    let mut plan: Vec<Step> = serde_json::from_value(st["steps"].clone()).unwrap();
    plan.retain(|s| s.kind != "break");
    let tail = plan.split_off(cur);
    let mut bad = plan.clone();
    bad.reverse();
    bad.extend(tail.clone());
    assert!(db.block_plan_set(b, &bad, T0).is_err() || cur <= 1);
    let mut tail = tail;
    tail.reverse();
    plan.extend(tail);
    db.block_plan_set(b, &plan, T0 + 3600).unwrap();
    // Завершить в любой момент; итог показывает ответы, помощь, мысли и сроки возврата.
    let s = db.block_close(b, Some(3), Some("журнал веба сложнее"), T0 + 3700).unwrap();
    assert_eq!(s["closed"], true);
    assert!(s["answers"].as_array().unwrap().len() >= 3);
    assert!(s["answers"].as_array().unwrap().iter().any(|a| a["due"].is_i64()));
    assert_eq!(db.block_close(b, None, None, T0 + 9999).unwrap()["energy"], 3, "повтор закрытия безопасен");
    assert!(db.block_open().unwrap().is_none());
    assert!(db.block_answer(b, 99, "x", None, 0, T0).is_err());
}

#[test]
fn self_graded_card_uses_hard_and_never_enters_measurement() {
    let (_d, p) = tmp();
    let db = Db::open(&p).unwrap();
    let m = learn::anki_import("#separator:tab\nчто такое ASLR\tрандомизация адресов\n", "anki-sec", "ASLR").unwrap();
    let g = db.goal_create(&m, T0).unwrap();
    let b = db.block_start(g, None, T0).unwrap();
    assert!(db.block_answer(b, 0, "что-то", None, 0, T0).is_err(), "без самооценки не записывается");
    assert_eq!(db.block_reveal(b, 0).unwrap()["answer"][0], "рандомизация адресов");
    let r = db.block_answer(b, 0, "рандомизация", Some("hard"), 0, T0).unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(r["graded"], "self");
    let p = db.progress(T0).unwrap();
    assert!(p["skills"].as_array().unwrap().is_empty(), "самооценка не входит в успех на новых задачах");
    // Повторный экспорт в Anki и импорт обратно дают те же поля.
    let tsv = learn::anki_export(&m);
    let back = learn::anki_import(&tsv, "anki-sec", "ASLR").unwrap();
    assert_eq!(back.item[0].prompt, m.item[0].prompt);
    assert_eq!(back.item[0].answer, m.item[0].answer);
}

#[test]
fn goal_edit_validates_and_reorders() {
    let (_d, p) = tmp();
    let db = Db::open(&p).unwrap();
    let g = db.goal_create(&logs(), T0).unwrap();
    let edit = |skills: Vec<&str>, src: &str| GoalEdit {
        title: "Журналы к пятнице".into(),
        skills: skills.into_iter().map(String::from).collect(),
        materials: vec![Material { label: "Конспект".into(), src: src.into(), scope: "раздел про sshd".into() }],
        deadline: Some(T0 + 5 * DAY),
        block_min: 40,
    };
    assert!(db.goal_update(g, &edit(vec!["logs-web", "nope"], "/tmp/x.md"), T0).is_err());
    assert!(db.goal_update(g, &edit(vec!["logs-web"], "x.md"), T0).is_err(), "у материала понятный источник");
    db.goal_update(g, &edit(vec!["logs-web", "logs-auth"], "https://example.org/sshd"), T0).unwrap();
    let gs = db.goals().unwrap();
    assert_eq!(gs[0]["skills"][0]["id"], "logs-web");
    assert!(db.prereq_set("logs-auth", &["logs-method".into()]).is_err(), "цикл через logs-method");
    let t = db.today(T0, 0).unwrap();
    assert!(t["actions"][0]["reason"].as_str().unwrap().contains("до срока 5 дн."));
}

#[test]
fn backlog_is_neutral_and_can_be_spread() {
    let (_d, p) = tmp();
    let db = Db::open(&p).unwrap();
    let g = db.goal_create(&logs(), T0).unwrap();
    let b = db.block_start(g, None, T0).unwrap();
    while let Some(step) = db.block_state(b).unwrap()["step"].as_i64() {
        let st = db.block_state(b).unwrap();
        match st["item"]["id"].as_str() {
            Some(i) if st["item"]["grading"] == "auto" => {
                db.block_answer(b, step, "нет", None, 0, T0).unwrap();
                let _ = i;
            }
            _ => db.block_skip(b, step, T0).unwrap(),
        }
    }
    db.block_close(b, None, None, T0).unwrap();
    let later = T0 + 20 * DAY;
    let t = db.today(later, 0).unwrap();
    assert_eq!(t["gap_days"], 19);
    assert!(t["gap_note"].as_str().unwrap().contains("Ничего не потеряно"));
    let n = t["actions"].as_array().unwrap().iter().find(|a| a["kind"] == "reviews").unwrap()["count"].as_i64().unwrap();
    assert_eq!(db.spread_reviews(3, later).unwrap(), n);
    let due_now = db.today(later, 0).unwrap()["actions"].as_array().unwrap().iter().find(|a| a["kind"] == "reviews").map_or(0, |a| a["count"].as_i64().unwrap());
    assert_eq!(due_now, (n + 2) / 3);
    // Часовой пояс: та же активность, другой локальный день.
    assert_eq!(db.today(T0 + DAY + 3600, 0).unwrap()["gap_days"], 0);
}

#[test]
fn edited_item_gets_new_version_and_old_result_stays() {
    let (_d, p) = tmp();
    let db = Db::open(&p).unwrap();
    let mut m = logs();
    db.goal_create(&m, T0).unwrap();
    let it = db.item("logs-auth-p3").unwrap();
    db.record(None, &it, "new", &it.answer[0], true, false, 0, T0).unwrap();
    let k = m.item.iter().position(|i| i.id == "logs-auth-p3").unwrap();
    m.item[k].answer = vec!["192.0.2.81".into()];
    let pv = db.module_preview(&m).unwrap();
    assert_eq!(pv["changed"][0]["id"], "logs-auth-p3");
    assert_eq!(pv["changed"][0]["attempts"], 1);
    db.module_put(&m, T0 + 1).unwrap();
    assert_eq!(db.item("logs-auth-p3").unwrap().v, it.v + 1);
    let ex = db.export().unwrap();
    let a = ex["attempt"].as_array().unwrap().iter().find(|a| a["item_id"] == "logs-auth-p3").unwrap().clone();
    assert_eq!(a["correct"], 1, "изменение эталона не переписывает результат");
    assert_eq!(a["item_v"], it.v);
    // Отмена возвращает прежнюю редакцию как новую версию; история не теряется.
    db.module_undo("log-analysis", T0 + 2).unwrap();
    assert_eq!(db.item("logs-auth-p3").unwrap().answer, it.answer);
    assert_eq!(db.item("logs-auth-p3").unwrap().v, it.v + 2);
}

#[test]
fn mvp_database_migrates_with_backup_and_weekly_check_still_works() {
    let (d, p) = tmp();
    std::fs::copy("tests/fixtures/mvp-v1.db", &p).unwrap();
    let db = Db::open(&p).unwrap();
    assert!(std::path::Path::new(&format!("{p}.pre-v{SCHEMA}.bak")).exists(), "копия до миграции");
    // Пилот и его группы не изменились, срок проверки — 7 дней.
    let before = db.pilot_report(T0).unwrap();
    assert!(before["groups"].as_array().unwrap().iter().any(|g| g["arm"] == "wr"));
    let later = 1_795_196_277 + 8 * DAY;
    let t = db.today(later, 0).unwrap();
    assert!(t["actions"].as_array().unwrap().iter().any(|a| a["kind"] == "test"), "{t}");
    let q = db.test_current(later).unwrap();
    assert!(q["item"]["id"].is_string());
    // Восстановление из копии до миграции: текущая база сохраняется рядом.
    drop(db);
    let saved = learn::restore(&format!("{p}.pre-v{SCHEMA}.bak"), &p, later).unwrap();
    assert!(std::path::Path::new(&saved).exists());
    let db = Db::open(&p).unwrap();
    assert!(db.test_current(later).unwrap()["item"]["id"].is_string());
    // Неверная копия отклоняется, база не трогается.
    std::fs::write(d.0.join("junk.db"), b"not sqlite").unwrap();
    assert!(learn::restore(&d.0.join("junk.db").to_string_lossy(), &p, later).is_err());
    assert!(Db::open(&p).is_ok());
}

#[test]
fn newer_schema_is_refused_with_clear_error() {
    let (_d, p) = tmp();
    drop(Db::open(&p).unwrap());
    rusqlite_set_version(&p, SCHEMA + 1);
    let err = Db::open(&p).err().unwrap();
    assert!(err.contains("более новой версией"), "{err}");
}

fn rusqlite_set_version(p: &str, v: i64) {
    std::process::Command::new("sqlite3").arg(p).arg(format!("PRAGMA user_version={v}")).status().unwrap();
}

#[test]
fn thirty_day_form_is_a_separate_objective_group() {
    let src = std::fs::read_to_string("../modules/log-analysis.toml").unwrap();
    let bad = src.replacen("role = \"test7\"\nkind", "role = \"test30\"\nkind", 1);
    assert!(Module::parse(&bad).is_err(), "одна цель — один срок");
    let (_d, p) = tmp();
    let db = Db::open(&p).unwrap();
    let m = Module::parse(
        r#"id = "h30"
title = "Тридцать дней"
domain = "custom"
[[objective]]
id = "h30-a"
title = "А"
[[item]]
id = "h30-p"
objective = "h30-a"
role = "practice"
kind = "number"
prompt = "2+2"
answer = ["4"]
explain = ""
[[item]]
id = "h30-t"
objective = "h30-a"
role = "test30"
kind = "number"
prompt = "3+3"
answer = ["6"]
explain = ""
"#,
    )
    .unwrap();
    db.goal_create(&m, T0).unwrap();
    db.done("h30-a", 0, T0).unwrap();
    assert!(db.test_current(T0 + 8 * DAY).unwrap().is_null(), "через 7 дней ещё рано");
    let q = db.test_current(T0 + 30 * DAY).unwrap();
    assert_eq!(q["item"]["id"], "h30-t");
    let r = db.test_submit(q["block"].as_i64().unwrap(), "h30-t", "6", 0, T0 + 30 * DAY).unwrap();
    assert_eq!(r["closed"], true);
    let pr = db.progress(T0 + 31 * DAY).unwrap();
    assert_eq!(pr["retention"][0]["horizon"], 30);
    assert_eq!(pr["retention"][0]["domain"], "custom");
    assert_eq!(pr["retention"][0]["num"], 1);
    assert_eq!(pr["retention"][0]["low"], true);
}

#[test]
fn seen_screen_reports_capture_off_without_session() {
    let (_d, p) = tmp();
    let db = Db::open(&p).unwrap();
    let s = db.seen(&p).unwrap();
    assert_eq!(s["capture"]["windows"], false);
    assert_eq!(s["capture"]["browser"], false);
    assert!(s["stored"]["bytes"].as_u64().unwrap() > 0);
    db.backup(&format!("{p}.manual.bak")).unwrap();
    assert!(db.backup(&format!("{p}.manual.bak")).is_err(), "копия не перезаписывается");
    assert_eq!(db.seen(&p).unwrap()["stored"]["copies"].as_array().unwrap().len(), 1);
}

/// Каждый модуль домена: ключи проходят собственную оценку (SymPy и тесты кода — в песочнице),
/// а траектория цель → блок → изучено → проверка через срок → «Прогресс» по домену проходит офлайн.
#[test]
fn every_domain_module_runs_from_goal_to_delayed_check() {
    let mut domains = std::collections::BTreeSet::new();
    for f in std::fs::read_dir("../modules").unwrap().flatten() {
        let path = f.path();
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let m = Module::parse(&std::fs::read_to_string(&path).unwrap()).unwrap_or_else(|x| panic!("{path:?}: {x}"));
        for it in m.item.iter().filter(|i| whiteroom_core::grading(&i.kind) == "auto") {
            assert!(whiteroom_core::grade(it, &it.answer[0]), "{}: ключ не проходит свою оценку", it.id);
            if it.kind != "code" {
                assert!(!whiteroom_core::grade(it, "заведомо неверно 42"), "{}: неверный ответ засчитан", it.id);
            }
        }
        if m.id == "re-x86-64" || m.id == "logs-defense" {
            continue; // пилотные модули MVP (без diag и stage): ключи проверены выше, путь — в pilot.rs
        }
        let (_d, p) = tmp();
        let db = Db::open(&p).unwrap();
        let g = db.goal_create(&m, T0).unwrap();
        let b = db.block_start(g, None, T0).unwrap();
        let mut answered = 0;
        while let Some(step) = db.block_state(b).unwrap()["step"].as_i64() {
            let st = db.block_state(b).unwrap();
            match st["item"]["id"].as_str() {
                None => db.block_skip(b, step, T0).unwrap(),
                Some(i) => {
                    let rating = (st["item"]["grading"] == "self").then_some("good");
                    let r = db.block_answer(b, step, &key(&db, i), rating, 0, T0).unwrap();
                    assert_eq!(r["ok"], true, "{i}");
                    answered += 1;
                }
            }
        }
        assert!(answered >= 3, "{}: в блоке {answered} заданий", m.id);
        db.block_close(b, None, None, T0).unwrap();
        for o in &m.objective {
            db.done(&o.id, 0, T0).unwrap();
        }
        let at = T0 + 7 * DAY + 3600;
        while let Some(q) = db.test_current(at).unwrap().as_object().cloned() {
            let id = q["item"]["id"].as_str().unwrap();
            db.test_submit(q["block"].as_i64().unwrap(), id, &key(&db, id), 0, at).unwrap();
        }
        let pr = db.progress(at).unwrap();
        let r = pr["retention"].as_array().unwrap().iter().find(|r| r["domain"] == m.domain.as_str()).unwrap_or_else(|| panic!("{}: {pr}", m.id));
        assert_eq!(r["num"], r["den"], "{}", m.id);
        assert!(r["den"].as_i64().unwrap() >= 1);
        domains.insert(m.domain.clone());
    }
    assert_eq!(domains.into_iter().collect::<Vec<_>>(), ["code", "custom", "cyber", "german", "math", "research"]);
}

#[test]
fn expression_equivalence_respects_domain() {
    use whiteroom_core::challenge::expr_equal;
    assert!(expr_equal("sqrt(x^2)", "x", "positive").unwrap());
    assert!(!expr_equal("sqrt(x^2)", "x", "real").unwrap());
    assert!(expr_equal("2*x/(x^2+1)", "(2*x)/(1+x**2)", "real").unwrap());
}
