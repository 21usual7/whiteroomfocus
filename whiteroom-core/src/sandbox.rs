//! Песочница для кода от ИИ и пользователя: контракт — docs/research/technical-feasibility.md, раздел 8.1.
//! systemd-run --user --scope (cgroup: память, процессы, CPU, время на всё дерево) + bwrap (без сети,
//! минимальная ФС, пустое окружение). Обходного пути нет: если песочница недоступна — ошибка (R11).

use crate::Res;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const MAX_OUTPUT: u64 = 1024 * 1024;

#[derive(Clone, Copy)]
pub struct Limits {
    pub mem_mb: u32,
    pub tasks: u32,
    pub secs: u32,
}

/// R7: потолок, который задача не может превысить.
pub const DEFAULT: Limits = Limits {
    mem_mb: 256,
    tasks: 64,
    secs: 10,
};
const MAX_SECS: u32 = 60;
/// Интерактивный запуск (gdb, ручная отладка эксплойта) живёт дольше, но тоже не бесконечно.
pub const INTERACTIVE_MAX_SECS: u32 = 3600;

pub struct Out {
    /// Код выхода; 128+N — убит сигналом N (137 — OOM или таймаут, 143 — таймаут scope).
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Папка задания в tmpfs ($XDG_RUNTIME_DIR): записанное в ней засчитывается в MemoryMax песочницы,
/// поэтому код не может заполнить диск. `src/` монтируется только для чтения, `out/` — на запись.
pub struct Job {
    pub dir: PathBuf,
}

impl Job {
    pub fn new() -> Res<Job> {
        let base = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "нет XDG_RUNTIME_DIR")?;
        let dir = PathBuf::from(base).join("whiteroom/jobs").join(format!(
            "{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        for d in ["src", "out"] {
            std::fs::create_dir_all(dir.join(d)).map_err(|x| x.to_string())?;
        }
        Ok(Job { dir })
    }

    pub fn src(&self) -> PathBuf {
        self.dir.join("src")
    }

    pub fn out(&self) -> PathBuf {
        self.dir.join("out")
    }
}

impl Drop for Job {
    // R8: после запуска не остаётся файлов.
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn command(job: &Job, argv: &[&str], lim: Limits, cap: u32) -> Command {
    let secs = lim.secs.min(cap);
    let mut c = Command::new("systemd-run");
    c.args(["--user", "--scope", "--quiet", "--collect"]);
    for p in [
        format!("MemoryMax={}M", lim.mem_mb),
        "MemorySwapMax=0".into(),
        format!("TasksMax={}", lim.tasks),
        "CPUQuota=100%".into(),
        format!("RuntimeMaxSec={secs}"),
    ] {
        c.args(["-p", &p]);
    }
    c.args([
        "--",
        "env",
        "-i",
        "PATH=/usr/bin",
        "HOME=/work",
        "LANG=C.UTF-8",
    ]);
    c.args([
        "bwrap",
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
    ]);
    c.args([
        "--ro-bind",
        "/usr",
        "/usr",
        "--symlink",
        "usr/bin",
        "/bin",
        "--symlink",
        "usr/lib",
        "/lib",
    ]);
    c.args([
        "--symlink",
        "usr/lib",
        "/lib64",
        "--ro-bind",
        "/etc/ld.so.cache",
        "/etc/ld.so.cache",
    ]);
    c.args([
        "--proc", "/proc", "--dev", "/dev", "--size", "67108864", "--tmpfs", "/tmp",
    ]);
    c.arg("--ro-bind").arg(job.src()).arg("/work/src");
    c.arg("--bind").arg(job.out()).arg("/work/out");
    c.args(["--chdir", "/work"]);
    c.args(argv);
    // R6: наследуются только 0/1/2. Замыкание выполняется в дочернем процессе после dup2 stdio, перед exec.
    unsafe {
        c.pre_exec(|| {
            libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32);
            Ok(())
        });
    }
    c
}

fn code(s: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0))
}

/// Читает не больше MAX_OUTPUT байт, остальное выбрасывает, чтобы процесс не завис на полном канале (R9).
fn capped(r: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut r = r;
        let mut v = Vec::new();
        let _ = (&mut r).take(MAX_OUTPUT).read_to_end(&mut v);
        let _ = std::io::copy(&mut r, &mut std::io::sink());
        v
    })
}

/// Запуск с вводом `input` и перехватом вывода.
pub fn run(job: &Job, argv: &[&str], input: &[u8], lim: Limits) -> Res<Out> {
    let mut child = command(job, argv, lim, MAX_SECS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|x| format!("песочница не запустилась: {x}"))?;
    let (o, e) = (
        capped(child.stdout.take().unwrap()),
        capped(child.stderr.take().unwrap()),
    );
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let w = std::thread::spawn(move || {
        let _ = stdin.write_all(&input); // процесс может не читать ввод — это не ошибка
    });
    let status = child.wait().map_err(|x| x.to_string())?;
    let _ = w.join();
    Ok(Out {
        code: code(status),
        stdout: o.join().unwrap_or_default(),
        stderr: e.join().unwrap_or_default(),
    })
}

/// Интерактивный запуск: stdin/stdout/stderr — наши (терминал или pwntools `process`).
pub fn run_interactive(job: &Job, argv: &[&str], lim: Limits) -> Res<i32> {
    let s = command(job, argv, lim, INTERACTIVE_MAX_SECS)
        .status()
        .map_err(|x| format!("песочница не запустилась: {x}"))?;
    Ok(code(s))
}

/// R11: проверка перед первым запуском. Ошибка — запуск кода выключен.
pub fn check() -> Res<()> {
    let job = Job::new()?;
    let out = run(
        &job,
        &["sh", "-c", "echo ok"],
        b"",
        Limits { secs: 5, ..DEFAULT },
    )
    .map_err(|x| format!("{x}. Нужны bubblewrap и systemd --user."))?;
    if out.code != 0 || out.stdout != b"ok\n" {
        return Err(format!(
            "песочница недоступна (код {}): {}. Запуск кода выключен.",
            out.code,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(())
}

/// Копирует файлы задания в src/ (имена — только простые, без путей).
pub fn put(job: &Job, files: &[(&str, &[u8])]) -> Res<()> {
    for (name, data) in files {
        let ok = !name.is_empty()
            && Path::new(name).file_name().is_some_and(|f| f == *name)
            && !name.starts_with('.');
        if !ok {
            return Err(format!("недопустимое имя файла: {name}"));
        }
        std::fs::write(job.src().join(name), data).map_err(|x| x.to_string())?;
    }
    Ok(())
}
