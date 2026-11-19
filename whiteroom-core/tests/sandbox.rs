//! Приёмочные тесты контракта песочницы (technical-feasibility.md, 8.1). Без них запуск кода не включается.
use std::os::fd::AsRawFd;
use whiteroom_core::sandbox::{DEFAULT, Job, Limits, check, put, run};

fn sh(script: &str) -> (i32, String, String) {
    let job = Job::new().unwrap();
    let o = run(&job, &["sh", "-c", script], b"", DEFAULT).unwrap();
    (o.code, String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into())
}

#[test]
fn available() {
    check().unwrap();
}

#[test]
fn no_user_files_no_desktop_no_network() {
    let home = std::env::var("HOME").unwrap();
    let (_, out, _) = sh(&format!(
        "ls {home} >/dev/null 2>&1 && echo HOME; ls /run/user >/dev/null 2>&1 && echo RUN; \
         ls /etc/passwd >/dev/null 2>&1 && echo ETC; \
         python3 -c 'import socket; socket.create_connection((\"1.1.1.1\", 53), 2)' 2>/dev/null && echo NET; \
         touch /work/src/x 2>/dev/null && echo SRC; touch /usr/x 2>/dev/null && echo USR; env | grep -vE '^(SHLVL|_)=' | sort | tr '\\n' ' '"
    ));
    assert_eq!(out, "HOME=/work LANG=C.UTF-8 PATH=/usr/bin PWD=/work ", "утечка: {out}");
}

#[test]
fn inherited_fd_not_visible() {
    let f = std::fs::File::open("/etc/hostname").unwrap();
    let fd = f.as_raw_fd();
    unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }; // снимаем CLOEXEC, как сделала бы неаккуратная библиотека
    let (_, out, _) = sh(&format!("ls /proc/self/fd | tr '\\n' ' '; cat <&{fd} 2>/dev/null && echo LEAK"));
    assert_eq!(out, "0 1 2 3 ", "fd {fd} утёк: {out}"); // 3 — сам ls открывает /proc/self/fd
    drop(f);
}

#[test]
fn limits_apply_to_whole_tree() {
    let (_, out, err) = sh("for i in $(seq 100); do sleep 2 & done; wait; echo done");
    assert!(err.contains("fork") || err.contains("Resource temporarily unavailable"), "TasksMax не сработал: {out} {err}");

    // Суть — выделить 600 МБ нельзя. Код выхода зависит от того, кого выберет OOM killer (python или sh).
    let (code, out, _) = sh("python3 -c 'a = bytearray(600 * 1024 * 1024); print(\"ALLOC\")'");
    assert!(code != 0 && !out.contains("ALLOC"), "MemoryMax не сработал: {code} {out}");

    // tmpfs src/out засчитывается в память: диск заполнить нельзя.
    let (code, _, _) = sh("head -c 400M /dev/zero > /work/out/big; echo $?");
    assert_ne!(code, 0, "запись 400 МБ должна упереться в лимит");

    let t = std::time::Instant::now();
    let job = Job::new().unwrap();
    let o = run(&job, &["sh", "-c", "setsid sleep 100 & sleep 100"], b"", Limits { secs: 3, ..DEFAULT }).unwrap();
    assert!(t.elapsed().as_secs() < 10 && o.code >= 128, "таймаут: {} за {:?}", o.code, t.elapsed());
    let left = std::process::Command::new("pgrep").args(["-f", "^sleep 100$"]).output().unwrap();
    assert!(left.stdout.is_empty(), "остались процессы");
}

#[test]
fn output_capped_and_job_cleaned() {
    let job = Job::new().unwrap();
    let dir = job.dir.clone();
    let o = run(&job, &["sh", "-c", "head -c 5M /dev/zero"], b"", DEFAULT).unwrap();
    assert_eq!(o.stdout.len() as u64, whiteroom_core::sandbox::MAX_OUTPUT);
    drop(job);
    assert!(!dir.exists(), "папка задания не удалена");
}

#[test]
fn bad_file_names_rejected() {
    let job = Job::new().unwrap();
    for bad in ["../x", "/etc/x", "a/b", ".hidden", ""] {
        assert!(put(&job, &[(bad, b"x")]).is_err(), "{bad}");
    }
}

#[test]
fn compile_and_run_c_with_stdin() {
    let job = Job::new().unwrap();
    put(&job, &[("v.c", b"#include <stdio.h>\nint main(){char b[16];if(fgets(b,16,stdin))printf(\"got %s\",b);}\n")]).unwrap();
    let c = run(&job, &["gcc", "-o", "/work/out/v", "/work/src/v.c"], b"", Limits { secs: 30, mem_mb: 512, ..DEFAULT }).unwrap();
    assert_eq!(c.code, 0, "{}", String::from_utf8_lossy(&c.stderr));
    let o = run(&job, &["/work/out/v"], b"hello\n", DEFAULT).unwrap();
    assert_eq!(String::from_utf8_lossy(&o.stdout), "got hello\n");
}
