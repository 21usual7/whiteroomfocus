import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import supervisor
import supervisor_runtime as runtime


TARGET = "whiteroom-app/ui/style.css"


def task(task_id="first", kind="routine"):
    return {"id": task_id, "title": task_id, "kind": kind,
            "paths": [TARGET] if kind == "routine" else [], "criteria": [f"criterion {task_id}"]}


def answer(verdict, criteria=(), summary=""):
    return 0, 7, {"verdict": verdict, "criteria": list(criteria), "summary": summary}


class SupervisorTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.repo = Path(temp.name)
        self.git("init", "-q")
        self.git("config", "user.name", "Supervisor Test")
        self.git("config", "user.email", "supervisor@example.test")
        self.write(TARGET, "base\n")
        for name in supervisor.PROTECTED:
            self.write(name, "/.supervisor/\n" if name == ".gitignore" else "protected\n")
        self.write(supervisor.APPROVAL, json.dumps({
            "status": "approved_for_implementation", "phase": "test", "approval_record": supervisor.DECISIONS,
        }))
        self.write(supervisor.DECISIONS, "| D1 | Approved by owner | Testing |\n")
        self.storage = self.repo / ".supervisor" / "test"
        self.storage.mkdir(parents=True)
        self.storage.parent.chmod(0o700)
        mocks = (
            patch.object(supervisor, "validate", return_value=(0, "all checks passed")),
            patch.object(supervisor, "codex", side_effect=self.model),
            patch.object(supervisor.shutil, "which", return_value="/mock/codex"),
        )
        self.validate, self.codex, _ = [mock.start() for mock in mocks]
        for mock in mocks:
            self.addCleanup(mock.stop)
        self.review = "accept"
        self.roles = []

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True).strip()

    def write(self, name, value):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)

    def supervisor(self, tasks, **limits):
        config = {
            "version": 1, "milestone": "test", "phase": "test", "decision": "D1",
            "limits": {"minutes": 1, "iterations": 10, "attempts": 3,
                       "executions": 10, "tokens": 100, **limits},
            "tasks": tasks,
        }
        self.write(supervisor.PLAN, json.dumps(config))
        self.git("add", ".")
        self.git("commit", "-qm", "Fixture")
        self.plan, self.identity = supervisor.load_policy(self.repo)
        self.head = supervisor.clean_head(self.repo)
        return self.resume()

    def resume(self):
        return supervisor.Supervisor(self.repo, self.plan, self.identity, self.storage, self.head)

    def model(self, role, workspace, prompt, seconds, schema):
        self.roles.append((role, workspace))
        self.assertFalse((workspace / ".git").exists())
        self.assertTrue(schema.is_file())
        spec = json.loads(prompt.split("\nTask:\n")[1].split("\nEvidence:\n")[0])
        if role == "builder":
            with (workspace / TARGET).open("a") as handle:
                handle.write(spec["id"] + "\n")
            return answer("ready", summary="builder-only-claim")
        self.assertNotIn("builder-only-claim", prompt)
        self.assertIn("+" + spec["id"], prompt)
        return answer(self.review, spec["criteria"] if self.review == "accept" else (), "review")

    def test_continues_and_checkpoints_only_on_separate_branch(self):
        runner = self.supervisor([task(), task("second")])
        runner.execute()
        commits = [entry["commit"] for entry in runner.state["completed"]]
        self.assertEqual(self.git("rev-parse", runner.ref), commits[1])
        self.assertEqual(self.git("rev-parse", commits[1] + "^"), commits[0])
        self.assertEqual(self.git("rev-parse", "HEAD"), self.head)
        self.assertEqual(self.git("show", f"{commits[1]}:{TARGET}"), "base\nfirst\nsecond")
        self.assertEqual(self.git("status", "--porcelain"), "")
        self.assertEqual([role for role, _ in self.roles], ["builder", "reviewer"] * 2)
        self.assertNotEqual(self.roles[0][1], self.roles[1][1])
        self.assertEqual(self.validate.call_count, 3)
        self.resume().execute()
        self.assertEqual(self.codex.call_count, 4)

    def test_resumes_candidate_after_check_timeout_without_rebuilding(self):
        runner = self.supervisor([task()], attempts=1)
        self.validate.side_effect = [(0, "baseline"), (124, "deadline"), (0, "candidate")]
        with self.assertRaises(runtime.Limit):
            runner.execute()
        self.assertIsNone(runner.ref_tip())
        self.assertEqual(runner.state["active"]["candidate"]["phase"], "validation")
        restored = self.resume()
        restored.execute()
        self.assertEqual([role for role, _ in self.roles], ["builder", "reviewer"])
        self.assertEqual(len(restored.state["completed"]), 1)

    def test_retry_limit_and_identical_rejection(self):
        runner = self.supervisor([task()])
        self.review = "revise"
        with self.assertRaisesRegex(runtime.Stop, "повторил"):
            runner.execute()
        self.assertEqual([role for role, _ in self.roles], ["builder", "reviewer", "builder"])
        self.assertIsNone(runner.ref_tip())

    def test_failing_checks_prevent_review_and_checkpoint(self):
        runner = self.supervisor([task()], attempts=1)
        self.validate.side_effect = [(0, "baseline"), (5, "real check failed")]
        with self.assertRaisesRegex(runtime.Stop, "real check failed"):
            runner.execute()
        self.assertEqual([role for role, _ in self.roles], ["builder"])
        self.assertIsNone(runner.ref_tip())

    def test_baseline_failure_prevents_codex_calls(self):
        runner = self.supervisor([task()])
        self.validate.return_value = (1, "baseline failed")
        with self.assertRaisesRegex(runtime.Stop, "Исходное дерево"):
            runner.execute()
        self.codex.assert_not_called()

    def test_missing_usage_blocks_resumption(self):
        runner = self.supervisor([task()])
        self.codex.side_effect = None
        self.codex.return_value = (1, None, None)
        with self.assertRaisesRegex(runtime.Stop, "статистики API"):
            runner.execute()
        with self.assertRaisesRegex(runtime.Stop, "статистики API"):
            self.resume().execute()
        self.assertEqual(self.codex.call_count, 1)
        self.assertIsNone(runner.ref_tip())

    def test_calls_and_reported_token_caps(self):
        runner = self.supervisor([task()], executions=1)
        with self.assertRaisesRegex(runtime.Stop, "Лимит выполнений"):
            runner.execute()
        self.assertEqual(self.codex.call_count, 1)
        self.assertIsNone(runner.ref_tip())
        runner.limits["executions"] = 10
        runner.limits["tokens"] = 12
        with self.assertRaisesRegex(runtime.Stop, "токенный лимит"):
            runner.execute()
        self.assertEqual(runner.state["tokens"], 14)
        self.assertIsNone(runner.ref_tip())

    def test_owner_gate_runs_no_agents_or_checks(self):
        runner = self.supervisor([task("human", "owner"), task()])
        with self.assertRaisesRegex(runtime.Stop, "подтверждение владельца"):
            runner.execute()
        self.codex.assert_not_called()
        self.validate.assert_not_called()

    def test_trusted_acceptance_fixture_runs_on_candidate_only(self):
        test_path = "tests/supervisor-acceptance/example.test.mjs"
        self.write(test_path, "import 'node:test';\n")
        spec = {**task(), "acceptance": [test_path]}
        self.supervisor([spec]).execute()
        baseline, candidate = self.validate.call_args_list
        self.assertFalse(baseline.args[3])
        self.assertEqual(candidate.args[3], [test_path])

    def test_launch_reports_an_owner_gate_in_a_clean_checkout(self):
        root = Path(supervisor.__file__).resolve().parents[1]
        for name in supervisor.PROTECTED:
            if name.startswith("scripts/"):
                self.write(name, (root / name).read_text())
        self.supervisor([task("human", "owner")])
        for arguments, exit_code in ((["--status"], 1), (["run"], 3), (["report"], 3)):
            result = subprocess.run(
                ["bash", "scripts/supervise", *arguments], cwd=self.repo, text=True,
                capture_output=True, check=False,
            )
            self.assertEqual(result.returncode, exit_code, result.stdout + result.stderr)
            self.assertIn("подтверждение владельца", result.stdout + result.stderr)

    def test_closed_milestone_is_reported_while_next_milestone_awaits_approval(self):
        root = Path(supervisor.__file__).resolve().parents[1]
        for name in supervisor.PROTECTED:
            if name.startswith("scripts/"):
                self.write(name, (root / name).read_text())
        self.supervisor([task()])
        self.write(supervisor.APPROVAL, json.dumps({
            "status": "pending_owner_approval",
            "completed_milestone": "M1",
            "git_checkpoint": self.head,
        }))
        self.git("add", supervisor.APPROVAL)
        self.git("commit", "-qm", "Close milestone")
        for arguments, exit_code in ((["--status"], 3), (["run"], 3), (["report"], 0)):
            result = subprocess.run(
                ["bash", "scripts/supervise", *arguments], cwd=self.repo, text=True,
                capture_output=True, check=False,
            )
            self.assertEqual(result.returncode, exit_code, result.stdout + result.stderr)
            self.assertIn("Завершённый этап: M1", result.stdout)
            self.assertIn(self.head[:12], result.stdout)
            self.assertIn("Следующий этап ждёт одобрения владельца", result.stdout)

    def test_policy_edit_by_builder_is_blocked(self):
        runner = self.supervisor([task()])

        def modify_policy(_role, workspace, *_args):
            (workspace / supervisor.PLAN).write_text("{}\n")
            return answer("ready")

        self.codex.side_effect = modify_policy
        with self.assertRaisesRegex(runtime.Stop, "вне разрешённого"):
            runner.execute()
        self.assertIsNone(runner.ref_tip())

    def test_reconciles_checkpoint_after_ref_update_before_state_save(self):
        runner = self.supervisor([task()])
        save = runner.save

        def interrupt_after_publish():
            if runner.state["completed"]:
                raise KeyboardInterrupt
            save()

        with patch.object(runner, "save", side_effect=interrupt_after_publish):
            with self.assertRaises(KeyboardInterrupt):
                runner.execute()
        self.assertIsNotNone(runner.ref_tip())
        restored = self.resume()
        restored.execute()
        self.assertEqual(len(restored.state["completed"]), 1)
        self.assertEqual(self.codex.call_count, 2)

    def test_iteration_limit_resumes_from_checkpoint(self):
        runner = self.supervisor([task(), task("second")], iterations=1)
        with self.assertRaises(runtime.Limit):
            runner.execute()
        self.resume().execute()
        self.assertEqual(len(self.resume().state["completed"]), 2)
        self.assertEqual(self.codex.call_count, 4)


class RuntimeTests(unittest.TestCase):
    def test_process_uses_real_exit_code_and_times_out(self):
        cwd = Path.cwd()
        self.assertEqual(runtime.run([sys.executable, "-c", "raise SystemExit(7)"], cwd, 2)[0], 7)
        self.assertEqual(runtime.run([sys.executable, "-c", "import time; time.sleep(10)"], cwd, .1)[0], 124)

    def test_reviewer_uses_read_only_and_actual_jsonl_usage(self):
        events = [
            {"type": "item.completed", "item": {
                "type": "agent_message", "text": json.dumps(answer("accept", ["proof"])[2]),
            }},
            {"type": "turn.completed", "usage": {"input_tokens": 100, "cached_input_tokens": 90, "output_tokens": 10}},
        ]
        with patch.object(runtime, "run", return_value=(0, "\n".join(map(json.dumps, events)).encode(), b"")) as run:
            self.assertEqual(runtime.codex("reviewer", Path.cwd(), "review", 1, Path("/schema"))[:2], (0, 110))
        argv = run.call_args.args[0]
        self.assertEqual(argv[argv.index("--sandbox") + 1], "read-only")
        self.assertEqual(argv[argv.index("--ask-for-approval") + 1], "never")
        self.assertIn("--ephemeral", argv)

    def test_diff_rejects_common_secret(self):
        data = ("key = 'sk-" + "a" * 30 + "'\n").encode()
        with self.assertRaisesRegex(runtime.Stop, "секрет"):
            runtime.changes({}, {TARGET: (0o644, data)}, [TARGET])


if __name__ == "__main__":
    unittest.main()
