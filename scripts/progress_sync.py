#!/usr/bin/env python3
"""Sync the approved WHITE ROOM data to Cloudflare D1."""

import argparse
from collections import Counter
from contextlib import closing
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import sys
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, Request, build_opener


TABLES = {
    "objective": "id title arm assigned_at done_at outside_min",
    "item": "id v ord objective role",
    "card": "item_id stability difficulty stability_fast last due",
    "session": "id started ended planned predicted correct",
    "attempt": "id session_id item_id item_v role ts answer correct hint ms",
    "study": "id started ended topic",
    "study_focus": "study_id mode direction",
    "capture": "study_id windows browser",
    "focus": "id start end class title study_id",
    "page": "id ts domain title excerpt study_id",
    "challenge": "id created kind title topic difficulty hints solved_at",
    "challenge_attempt": "id challenge_id ts answer correct",
}
DDL = [
    """CREATE TABLE IF NOT EXISTS whiteroom_progress (
        source TEXT NOT NULL, collection TEXT NOT NULL, record_id TEXT NOT NULL,
        digest TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload)),
        PRIMARY KEY(source, collection, record_id)
    )""",
    """CREATE TABLE IF NOT EXISTS whiteroom_sync (
        source TEXT PRIMARY KEY, digest TEXT NOT NULL, record_count INTEGER NOT NULL,
        synced_at TEXT NOT NULL
    )""",
]
UPSERT = """INSERT INTO whiteroom_progress(source, collection, record_id, digest, payload)
            VALUES(?,?,?,?,?) ON CONFLICT(source, collection, record_id)
            DO UPDATE SET digest=excluded.digest,payload=excluded.payload"""


class SyncError(Exception):
    pass


def encode(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False)


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def read_records(path):
    uri = path.resolve(strict=True).as_uri() + "?mode=ro"
    with closing(sqlite3.connect(uri, uri=True, timeout=5)) as db:
        db.row_factory = sqlite3.Row
        db.execute("BEGIN")
        tables = {row[0] for row in db.execute("SELECT name FROM sqlite_schema WHERE type='table'")}
        if not {"objective", "item", "card", "session", "attempt", "study"} <= tables:
            raise SyncError("Выбранный файл не похож на базу WHITE ROOM.")
        nonmath = set()
        if "challenge" in tables:
            nonmath = {row[0] for row in db.execute("SELECT id FROM challenge WHERE kind <> 'math'")}
        records = []
        for table, columns in TABLES.items():
            if table not in tables:
                continue
            available = {row[1] for row in db.execute(f"PRAGMA table_info({table})")}
            fields = columns.split()
            optional = {"study_id"} if table in {"focus", "page"} else set()
            if set(fields) - available - optional:
                raise SyncError(f"Схема таблицы {table} не поддерживается.")
            key = fields[0]
            fields_sql = ",".join(field if field in available else f"NULL AS {field}" for field in fields)
            for row in db.execute(f"SELECT {fields_sql} FROM {table} ORDER BY {key}"):
                record = dict(row)
                if table == "challenge_attempt" and record["challenge_id"] in nonmath:
                    record["answer"] = None
                payload = encode(record)
                records.append((table, str(record[key]), sha(payload), payload))
        return records


class RejectRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class D1:
    def __init__(self, account, database, token):
        if not re.fullmatch(r"[a-fA-F0-9]{32}", account):
            raise SyncError("Нужен CLOUDFLARE_ACCOUNT_ID.")
        if not re.fullmatch(r"[a-fA-F0-9]{8}(?:-[a-fA-F0-9]{4}){3}-[a-fA-F0-9]{12}", database):
            raise SyncError("Нужен WHITEROOM_D1_DATABASE_ID.")
        if not token:
            raise SyncError("Нужен токен Cloudflare D1.")
        self.url = f"https://api.cloudflare.com/client/v4/accounts/{account}/d1/database/{database}/query"
        self.token = token
        self.opener = build_opener(RejectRedirect())

    def batch(self, statements):
        body = {"batch": [{"sql": sql, "params": [str(param) for param in params]}
                          for sql, params in statements]}
        request = Request(self.url, data=encode(body).encode(), headers={
            "Authorization": f"Bearer {self.token}", "Content-Type": "application/json",
        })
        try:
            with self.opener.open(request, timeout=30) as response:
                result = json.load(response)
        except HTTPError as error:
            raise SyncError(f"Cloudflare D1 вернул HTTP {error.code}.") from None
        except (URLError, TimeoutError, ValueError):
            raise SyncError("Cloudflare D1 недоступен или вернул неожиданный ответ.") from None
        if not isinstance(result, dict) or not result.get("success"):
            raise SyncError("Cloudflare D1 отклонил пакет запросов.")
        items = result.get("result")
        if not isinstance(items, list) or len(items) != len(statements):
            raise SyncError("Cloudflare D1 вернул неполный ответ.")
        if not all(isinstance(item, dict) and item.get("success") for item in items):
            raise SyncError("Cloudflare D1 не подтвердил выполнение всех запросов.")
        return [item.get("results", []) for item in items]

    def query(self, sql, *params):
        return self.batch([(sql, params)])[0]


def remote_index(remote, source):
    result = {}
    collection, record_id = "", ""
    while True:
        rows = remote.query(
            """SELECT collection, record_id, digest FROM whiteroom_progress
               WHERE source=? AND (collection, record_id)>(?,?)
               ORDER BY collection, record_id LIMIT 3000""",
            source, collection, record_id,
        )
        for row in rows:
            collection, record_id = row["collection"], row["record_id"]
            result[(collection, record_id)] = row["digest"]
        if len(rows) < 3000:
            return result


def write_pending(remote, source, changed):
    batch, size = [], 0
    for row in changed:
        params = (source, *row)
        row_size = len(encode(params).encode())
        if batch and (len(batch) >= 50 or size + row_size > 400_000):
            remote.batch(batch)
            batch, size = [], 0
        batch.append((UPSERT, params))
        size += row_size
    if batch:
        remote.batch(batch)


def push(remote, source, records):
    remote.batch([(sql, ()) for sql in DDL])
    expected = {(table, key): digest for table, key, digest, _payload in records}
    digest = sha(encode(sorted((*key, value) for key, value in expected.items())))
    state = remote.query("SELECT digest FROM whiteroom_sync WHERE source=?", source)
    if state and state[0]["digest"] == digest:
        return 0
    actual = remote_index(remote, source)
    changed = [row for row in records if actual.get(row[:2]) != row[2]]
    write_pending(remote, source, changed)
    if changed:
        actual = remote_index(remote, source)
    if any(actual.get(key) != expected_digest for key, expected_digest in expected.items()):
        raise SyncError("Cloudflare D1 не подтвердил все записи. Повторная отправка безопасна.")
    result = remote.query(
        """INSERT INTO whiteroom_sync(source, digest, record_count, synced_at)
           VALUES(?,?,?,datetime('now'))
           ON CONFLICT(source) DO UPDATE SET digest=excluded.digest,
           record_count=excluded.record_count, synced_at=excluded.synced_at
           RETURNING record_count""",
        source, digest, len(records),
    )
    if not result or result[0]["record_count"] != len(records):
        raise SyncError("Cloudflare D1 не подтвердил итог синхронизации.")
    return len(changed)


def default_database():
    data = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share")
    return Path(os.environ.get("WHITEROOM_DB") or data / "whiteroom/whiteroom.db")


def main():
    parser = argparse.ArgumentParser(description="Копирование прогресса WHITE ROOM в Cloudflare D1")
    parser.add_argument("action", choices=("preview", "push"))
    parser.add_argument("--db", type=Path, default=default_database())
    args = parser.parse_args()
    try:
        if args.action == "preview":
            counts = Counter(record[0] for record in read_records(args.db))
            print(f"Записей в локальной базе: {counts.total()}")
            for table in TABLES:
                print(f"{table}: {counts[table]}")
            return 0
        token = os.environ.get("CLOUDFLARE_API_TOKEN", "").strip()
        if not token and os.environ.get("CREDENTIALS_DIRECTORY"):
            token = (Path(os.environ["CREDENTIALS_DIRECTORY"]) / "cloudflare-token").read_text().strip()
        source = os.environ.get("WHITEROOM_D1_SOURCE", "desktop")
        if not re.fullmatch(r"[a-zA-Z0-9_-]{1,64}", source):
            raise SyncError("Некорректное имя источника WHITEROOM_D1_SOURCE.")
        remote = D1(os.environ.get("CLOUDFLARE_ACCOUNT_ID", ""), os.environ.get("WHITEROOM_D1_DATABASE_ID", ""), token)
        state_dir = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "whiteroom"
        state_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        fd = os.open(state_dir / "d1-sync.lock", os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            records = read_records(args.db)
            uploaded = push(remote, source, records)
        print(f"D1: проверено {len(records)} записей, отправлено изменений: {uploaded}.")
        return 0
    except SyncError as error:
        print(error, file=sys.stderr)
    except (OSError, sqlite3.Error, ValueError):
        print("Не удалось прочитать локальную базу или конфигурацию синхронизации.", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
