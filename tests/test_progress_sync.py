from collections import Counter
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import sys
from tempfile import TemporaryDirectory
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from progress_sync import SyncError, TABLES, encode, push, read_records, sha


class SQLiteD1:
    def __init__(self):
        self.db = sqlite3.connect(":memory:")
        self.db.row_factory = sqlite3.Row
        self.fail_after_write = False

    def batch(self, statements):
        with self.db:
            rows = [[dict(row) for row in self.db.execute(sql, params)] for sql, params in statements]
        if self.fail_after_write and "INSERT INTO whiteroom_progress" in statements[0][0]:
            raise SyncError("simulated lost response")
        return rows

    def query(self, sql, *params):
        return self.batch([(sql, params)])[0]


class ProgressSyncTests(unittest.TestCase):
    def test_copies_consented_fields_without_task_assets_and_accepts_legacy_page(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / "test.db"
            with closing(sqlite3.connect(path)) as db:
                for table, names in TABLES.items():
                    fields = names.split()
                    if table == "page":
                        fields.remove("study_id")
                    if table == "challenge":
                        fields.extend(["binary", "flag", "data"])
                    if table == "item":
                        fields.append("data")
                    db.execute(f"CREATE TABLE {table} ({','.join(fields)})")
                db.execute("""INSERT INTO page(id, ts, domain, title, excerpt)
                              VALUES(1, 10, 'study.example', 'Public material', 'Theory')""")
                db.execute("""INSERT INTO challenge(id, kind, title, binary, flag, data)
                              VALUES(3, 'pwn', 'Example', x'00', 'fixture-flag', 'fixture-code')""")
                db.execute("INSERT INTO challenge_attempt VALUES(1,3,10,'fixture-flag',1)")
                db.execute("INSERT INTO item(id,data) VALUES('i1','fixture-answer-key')")
                db.commit()
            rows = {name: json.loads(payload) for name, _, _, payload in read_records(path)}
        self.assertEqual(rows["page"]["excerpt"], "Theory")
        self.assertIsNone(rows["page"]["study_id"])
        self.assertEqual(rows["challenge_attempt"]["correct"], 1)
        self.assertIsNone(rows["challenge_attempt"]["answer"])
        self.assertFalse({"data", "flag", "binary"} & rows["challenge"].keys())
        self.assertNotIn("data", rows["item"])

    def test_push_verifies_retries_and_preserves_existing_history(self):
        remote = SQLiteD1()
        self.addCleanup(remote.db.close)
        def record(key, payload):
            text = encode(payload)
            return ("study", key, sha(text), text)
        original = record("1", {"ended": None})
        remote.fail_after_write = True
        with self.assertRaises(SyncError):
            push(remote, "desktop", [original])
        self.assertEqual(remote.query("SELECT * FROM whiteroom_sync"), [])
        remote.fail_after_write = False
        self.assertEqual(push(remote, "desktop", [original]), 0)
        changed = record("1", {"ended": 500})
        extra = record("2", {"ended": None})
        self.assertEqual(push(remote, "desktop", [changed, extra]), 2)
        self.assertEqual(push(remote, "desktop", [changed, extra]), 0)
        self.assertEqual(push(remote, "desktop", [changed]), 0)
        rows = remote.query("SELECT collection, payload FROM whiteroom_progress ORDER BY record_id")
        self.assertEqual(Counter(row["collection"] for row in rows), {"study": 2})
        self.assertEqual(json.loads(rows[0]["payload"]), {"ended": 500})


if __name__ == "__main__":
    unittest.main()
