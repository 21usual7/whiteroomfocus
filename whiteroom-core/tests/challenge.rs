use whiteroom_core::Db;
use whiteroom_core::challenge::Challenge;
use whiteroom_core::sandbox::{DEFAULT, run};

fn load(name: &str) -> Challenge {
    let p = format!("{}/../challenges/examples/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn db(name: &str) -> Db {
    let d = std::env::temp_dir().join(format!("whiteroom-ch-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    Db::open(d.join("db.sqlite").to_str().unwrap()).unwrap()
}

#[test]
fn examples_validate() {
    for name in ["pwn-ret2win", "pwn-admin-flag", "math-telescope", "math-x-root"] {
        if let Err(x) = load(name).validate() {
            panic!("{name}: {x}");
        }
    }
}

#[test]
fn broken_challenges_rejected() {
    let mut c = load("pwn-admin-flag");
    c.exploit = Some(c.exploit.unwrap().replace("b'A' * 16", "b'A' * 12"));
    assert!(c.validate().unwrap_err().contains("не получил флаг"));

    let mut c = load("pwn-admin-flag");
    c.source = Some(c.source.unwrap().replace("if (u.is_admin == 0x1337)", "if (1)"));
    assert!(c.validate().unwrap_err().contains("без эксплойта"));

    let mut c = load("pwn-ret2win");
    c.cflags.push("@/etc/passwd".into());
    assert!(c.validate().unwrap_err().contains("не разрешён"));

    let mut c = load("pwn-ret2win");
    c.source = Some("int main( {".into());
    assert!(c.validate().unwrap_err().contains("не компилируется"));

    let mut c = load("math-telescope");
    c.verify = Some(c.verify.unwrap().replace("Rational(1, 4)", "Rational(1, 3)"));
    assert!(c.validate().unwrap_err().contains("не подтвердил"));

    let mut c = load("math-telescope");
    c.hints.pop();
    assert!(c.validate().is_err());
}

#[test]
fn pwn_learner_flow() {
    let db = db("pwn");
    let id = db.add_challenge(&load("pwn-admin-flag").validate().unwrap(), 100).unwrap();
    // Подсказки по порядку; решение — только после попытки.
    for lvl in 1..=3 {
        assert_eq!(db.next_hint(id).unwrap().0, lvl);
    }
    assert!(db.next_hint(id).unwrap_err().contains("после твоей попытки"));
    assert!(!db.solve(id, "WR{guess}", 101).unwrap());
    assert_eq!(db.next_hint(id).unwrap().0, 4);

    // Как учащийся: запускает бинарь в песочнице со своим payload и сдаёт полученный флаг.
    let job = db.pwn_job(id).unwrap();
    let mut payload = b"A".repeat(16);
    payload.extend(0x1337u32.to_le_bytes());
    let o = run(&job, &["sh", "-c", "cd /work/out && exec ./chal"], &payload, DEFAULT).unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    let flag = out.lines().find(|l| l.starts_with("WR{")).expect(&out).to_string();
    assert!(db.solve(id, &flag, 102).unwrap());
    let r = db.challenge(id).unwrap();
    assert!(r.solved && r.attempts == 2);

    // Файлы для анализа: бинарь без права на запуск.
    let dir = std::env::temp_dir().join(format!("whiteroom-files-{}", std::process::id()));
    db.export_files(id, &dir).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(dir.join("chal")).unwrap().permissions().mode() & 0o111, 0);
    assert!(dir.join("chal.c").exists() && std::fs::read_to_string(dir.join("README.txt")).unwrap().contains("pwn run"));
    assert!(!std::fs::read_to_string(dir.join("README.txt")).unwrap().contains("WR{"), "флага в файлах нет");
}

#[test]
fn math_answers_checked_symbolically() {
    let db = db("math");
    let id = db.add_challenge(&load("math-x-root").validate().unwrap(), 100).unwrap();
    assert!(!db.solve(id, "1.4447", 101).unwrap(), "приближение не засчитывается");
    assert!(!db.solve(id, "e", 102).unwrap(), "e — это число, а не ответ");
    assert!(db.solve(id, "e**(1/e)", 102).unwrap(), "e пишется как число Эйлера");
    assert!(!db.solve(id, "x + 1", 102).unwrap(), "переменная в ответе — неверно, а не ошибка");
    assert!(db.solve(id, "exp(exp(-1))", 103).unwrap(), "другая запись того же числа");
    assert!(db.solve(id, "E**(1/E)", 104).unwrap());
    // Ответ пользователя разбирается только в песочнице: попытка выполнить код ничего не даёт.
    let r = db.solve(id, "__import__('os').system('touch /tmp/pwned')", 105);
    assert!(!std::path::Path::new("/tmp/pwned").exists());
    assert!(matches!(r, Ok(false) | Err(_)));
}

#[test]
fn ai_challenge_round_trip() {
    use whiteroom_core::challenge::{ai_request, from_response};
    let r = ai_request("Всего 60 мин\n  60 мин  ghidra\n", "pwn", &["Звонок без приглашения".into()], None);
    let schema = &r["output_config"]["format"]["schema"];
    assert_eq!(schema["required"].as_array().unwrap().len(), 10);
    assert!(r["system"].as_str().unwrap().contains("\"exploit\""), "образец формата в промпте");
    assert!(r["messages"][0]["content"].as_str().unwrap().contains("не повторяйся"));
    // Ответ модели в формате схемы — это наш образец; он же проходит проверку в песочнице.
    let example = std::fs::read_to_string(format!("{}/../challenges/examples/math-x-root.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let resp = serde_json::json!({"stop_reason": "end_turn", "content": [{"type": "text", "text": example}]});
    let (text, ch) = from_response(&resp).unwrap();
    assert!(ch.validate().is_ok());
    // Повторная попытка несёт прошлый ответ и ошибку проверки.
    let r2 = ai_request("a", "math", &[], Some((&text, "проверочный код не подтвердил ответ")));
    assert_eq!(r2["messages"].as_array().unwrap().len(), 3);
    assert!(r2["output_config"]["format"]["schema"]["required"].as_array().unwrap().iter().any(|k| k == "verify"));
    assert!(from_response(&serde_json::json!({"stop_reason": "refusal", "content": []})).is_err());
}
