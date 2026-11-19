"""Fixed process, workspace and validation boundaries for the local supervisor."""

import difflib
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import tempfile
import time

MAX_OUTPUT = 8 * 1024 * 1024
MAX_FILE = 256 * 1024
MAX_TREE = 32 * 1024 * 1024
SECRET = re.compile(
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----|"
    r"\b(?:sk-(?:proj-|ant-)?[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9]{30,}|"
    r"github_pat_[A-Za-z0-9_]{25,}|AKIA[A-Z0-9]{16})\b|"
    r"\b(?:api[_-]?key|access[_-]?token|password|secret)\s*[:=]\s*[\"'][^\"'\r\n]{12,}[\"']",
    re.IGNORECASE,
)

SCHEMA = {
    "type": "object",
    "properties": {
        "verdict": {"type": "string", "enum": ["ready", "accept", "revise", "stop"]},
        "summary": {"type": "string"},
        "criteria": {"type": "array", "items": {"type": "string"}},
    },
    "required": ["verdict", "summary", "criteria"],
    "additionalProperties": False,
}


class Stop(Exception):
    """A boundary that needs owner attention."""


class Unavailable(Stop):
    """A local prerequisite that can be restored and retried."""


class Limit(Exception):
    """A run limit that can be resumed."""


def run(args, cwd, seconds, *, data=None, env=None, max_output=MAX_OUTPUT):
    if seconds <= 0:
        raise Limit("Время запуска истекло.")
    deadline = time.monotonic() + seconds
    inputs = memoryview(data or b"")
    outputs = {"stdout": bytearray(), "stderr": bytearray()}
    with subprocess.Popen(
        args, cwd=cwd, env=env, stdin=subprocess.PIPE if data else subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True,
    ) as process, selectors.DefaultSelector() as handles:
        for stream in outputs:
            handles.register(getattr(process, stream), selectors.EVENT_READ, stream)
        if data:
            handles.register(process.stdin, selectors.EVENT_WRITE, "stdin")
        try:
            while handles.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(args, seconds)
                for key, _ in handles.select(remaining):
                    if key.data == "stdin":
                        try:
                            inputs = inputs[os.write(key.fd, inputs[:4096]):]
                        except BrokenPipeError:
                            inputs = memoryview(b"")
                        if inputs:
                            continue
                    else:
                        chunk = os.read(key.fd, 65536)
                        if chunk:
                            output = outputs[key.data]
                            if len(output) + len(chunk) > max_output:
                                return 125, b"", b"Output limit exceeded"
                            output.extend(chunk)
                            continue
                    handles.unregister(key.fileobj)
                    key.fileobj.close()
            code = process.wait(timeout=max(0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            code = 124
        finally:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
    return code, bytes(outputs["stdout"]), bytes(outputs["stderr"][-65536:])


def git(repo, *args, data=None, index=None, seconds=30):
    env = {name: value for name, value in os.environ.items() if not name.startswith("GIT_")}
    env.update(GIT_TERMINAL_PROMPT="0", GIT_OPTIONAL_LOCKS="0")
    if index is not None:
        env["GIT_INDEX_FILE"] = str(index)
    code, output, error = run(
        ["git", *args], repo, seconds, data=data, env=env, max_output=MAX_TREE * 2,
    )
    if code:
        raise Stop(f"Git {args[0]} ({code}): {error.decode(errors='replace').strip()[:400]}")
    return output


def atomic_json(path, value):
    fd, temporary = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(value, handle, ensure_ascii=False, indent=2)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(temporary).unlink(missing_ok=True)


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def snapshot(root):
    result = {}
    total = 0
    count = 0
    for directory, subdirs, files in os.walk(root, followlinks=False):
        for name in sorted(subdirs + files):
            path = Path(directory) / name
            info = path.lstat()
            count += 1
            if count > 8000 or path.is_symlink():
                raise Stop("Песочница содержит слишком много файлов или символическую ссылку.")
            if stat.S_ISDIR(info.st_mode):
                continue
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise Stop("Песочница содержит специальный файл или жёсткую ссылку.")
            total += info.st_size
            if total > MAX_TREE:
                raise Stop("Снимок рабочего дерева превысил лимит.")
            result[path.relative_to(root).as_posix()] = (info.st_mode & 0o777, path.read_bytes())
    return result


def changes(before, after, allowed):
    delta = {}
    patch = []
    for name in sorted(before.keys() | after.keys()):
        old, new = before.get(name), after.get(name)
        if old == new:
            continue
        mode = old[0] if old else 0o644
        if name not in allowed or new is None or new[0] not in (mode, mode & 0o700):
            raise Stop(f"Изменение вне разрешённого объёма: {name!r}.")
        if old and old[1] == new[1]:
            continue
        if len(new[1]) > MAX_FILE or b"\0" in new[1]:
            raise Stop(f"Недопустимый размер или формат: {name!r}.")
        try:
            old_text = old[1].decode() if old else ""
            new_text = new[1].decode()
        except UnicodeDecodeError as error:
            raise Stop(f"Файл должен быть текстом UTF-8: {name!r}.") from error
        lines = list(difflib.unified_diff(
            old_text.splitlines(), new_text.splitlines(),
            fromfile=f"a/{name}" if old else "/dev/null", tofile=f"b/{name}", lineterm="",
        ))
        if SECRET.search("\n".join(line[1:] for line in lines if line.startswith("+"))):
            raise Stop(f"В изменении {name!r} обнаружен возможный секрет.")
        if not lines:
            lines = [f"Изменены байты {name}; проверьте завершающий перевод строки."]
        patch.extend(lines)
        delta[name] = (mode, new[1])
    diff = "\n".join(patch)
    if len(diff.encode()) > MAX_FILE:
        raise Stop("Diff превысил лимит независимого ревью.")
    return delta, diff


def fingerprint(delta):
    return hashlib.sha256(json.dumps([
        (name, mode, hashlib.sha256(data).hexdigest())
        for name, (mode, data) in delta.items()
    ]).encode()).hexdigest()


def codex(role, root, prompt, seconds, schema):
    args = [
        "codex", "--ask-for-approval", "never", "--disable", "hooks", "--disable", "apps",
        "--disable", "plugins", "--disable", "multi_agent", "--disable", "multi_agent_v2",
        "-c", "mcp_servers={}", "-c", 'web_search="disabled"',
        "-c", 'shell_environment_policy.inherit="core"',
        "-c", 'shell_environment_policy.exclude=["*KEY*","*TOKEN*","*SECRET*","*PASSWORD*"]',
        "-c", "sandbox_workspace_write.network_access=false",
        "-c", "sandbox_workspace_write.writable_roots=[]",
        "-c", "sandbox_workspace_write.exclude_slash_tmp=true",
        "-c", "sandbox_workspace_write.exclude_tmpdir_env_var=true",
        "exec", "--ephemeral", "--skip-git-repo-check", "--json", "--color", "never",
        "--sandbox", "workspace-write" if role == "builder" else "read-only",
        "--output-schema", str(schema), "-",
    ]
    code, output, _ = run(args, root, seconds, data=prompt.encode())
    usage = 0
    measured = False
    complete = None
    for line in output.splitlines():
        try:
            event = json.loads(line)
        except (ValueError, UnicodeError):
            continue
        if not isinstance(event, dict):
            continue
        if event.get("type") == "turn.completed":
            counts = event.get("usage")
            if not isinstance(counts, dict):
                return code or 1, None, None
            tokens = [counts.get(key) for key in ("input_tokens", "output_tokens")]
            if any(type(number) is not int or number < 0 for number in tokens):
                return code or 1, None, None
            usage += sum(tokens)
            measured = True
        item = event.get("item")
        if (event.get("type") == "item.completed" and isinstance(item, dict)
                and item.get("type") == "agent_message"):
            complete = item.get("text")
    try:
        answer = json.loads(complete)
    except (TypeError, ValueError):
        answer = None
    if (not isinstance(answer, dict) or set(answer) != SCHEMA["properties"].keys()
            or answer["verdict"] not in SCHEMA["properties"]["verdict"]["enum"]
            or not isinstance(answer["summary"], str) or len(answer["summary"]) > 1200
            or not isinstance(answer["criteria"], list)
            or not all(isinstance(criterion, str) for criterion in answer["criteria"])):
        answer = None
    return code, usage if measured else None, answer


def tauri_mounts(root):
    source = root / "whiteroom-app"
    app = "/work/whiteroom-app"
    permissions = f"{app}/permissions"
    mounts = ["--tmpfs", app, "--tmpfs", permissions]
    for directory, destination, excluded in (
        (source, app, {"gen", "permissions", ".cargo-home"}),
        (source / "permissions", permissions, {"autogenerated"}),
    ):
        if directory.is_symlink():
            raise Unavailable(f"Каталог сборки Tauri является символической ссылкой: {directory}.")
        if directory.is_dir():
            for entry in sorted(directory.iterdir()):
                if entry.name in excluded:
                    continue
                if entry.is_symlink():
                    raise Unavailable(f"Файл сборки Tauri является символической ссылкой: {entry}.")
                mounts.extend(("--ro-bind", str(entry), f"{destination}/{entry.name}"))
    # Tauri writes these paths beside the manifest even with CARGO_TARGET_DIR set.
    return mounts + [
        "--tmpfs", f"{app}/gen", "--tmpfs", f"{permissions}/autogenerated",
        "--remount-ro", permissions, "--remount-ro", app,
    ]


def validation_command(root, repo, seconds, targets):
    for executable in ("mise", "node", "bwrap"):
        if not shutil.which(executable):
            raise Unavailable(f"Для проверок требуется {executable}.")
    code, output, _ = run(["mise", "exec", "--", "rustc", "--print", "sysroot"], repo, min(seconds, 15))
    if code == 124:
        raise Limit("Не хватило времени на поиск toolchain Rust.")
    if code:
        raise Unavailable("Mise не смог найти установленный Rust.")
    toolchain = Path(output.decode().strip()).resolve()
    registry = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "registry"
    if any(not (toolchain / "bin" / name).is_file() for name in ("cargo", "cargo-clippy", "rustfmt")) or not registry.is_dir():
        raise Unavailable("Нужны установленные Clippy, rustfmt и локальный offline-кеш Cargo.")
    node = Path(shutil.which("node")).resolve()
    return [
        "bwrap", "--unshare-all", "--die-with-parent", "--new-session", "--clearenv",
        "--ro-bind", "/usr", "/usr", "--symlink", "usr/bin", "/bin",
        "--symlink", "usr/lib", "/lib", "--symlink", "usr/lib", "/lib64",
        "--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp",
        "--ro-bind", str(toolchain), "/toolchain",
        "--ro-bind", str(registry), "/tmp/cargo/registry",
        "--ro-bind", str(node), "/tools/node",
        "--ro-bind", str(root), "/work", *tauri_mounts(root),
        "--ro-bind", str(repo / "scripts/supervisor-checks"), "/checks",
        "--setenv", "HOME", "/tmp", "--setenv", "TMPDIR", "/tmp",
        "--setenv", "CARGO_HOME", "/tmp/cargo", "--setenv", "CARGO_TARGET_DIR", "/tmp/target",
        "--setenv", "CARGO_NET_OFFLINE", "true", "--setenv", "RUST_BACKTRACE", "0",
        "--setenv", "CARGO_BUILD_JOBS", "2", "--setenv", "CARGO_INCREMENTAL", "0",
        "--setenv", "WHITEROOM_DB", "/tmp/whiteroom.db", "--setenv", "LC_ALL", "C",
        "--setenv", "PATH", "/toolchain/bin:/tools:/usr/bin", "--chdir", "/work",
        "/bin/bash", "/checks", *targets,
    ]


def validate(root, repo, seconds, targets=()):
    started = time.monotonic()
    command = validation_command(root, repo, seconds, targets)
    code, output, error = run(command, repo, seconds - (time.monotonic() - started))
    return code, (output[-32000:] + b"\n" + error[-32000:]).decode(errors="replace")
