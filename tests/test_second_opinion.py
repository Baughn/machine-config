import importlib.util
import json
import os
import re
from pathlib import Path
import tempfile
import time
from types import SimpleNamespace as NS
import unittest
import unittest.mock

source = Path(__file__).resolve().parents[1] / "modules/second-opinion/second-opinion.py"
spec = importlib.util.spec_from_file_location("second_opinion", source)
so = importlib.util.module_from_spec(spec)
spec.loader.exec_module(so)


class Paths(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        base = Path(self.tmp.name)
        self.repo = base / "repo"
        (self.repo / "sub").mkdir(parents=True)
        (self.repo / "sub" / "a.txt").write_text("one\ntwo\nthree\n")
        (base / "secret").write_text("no")
        os.symlink(base / "secret", self.repo / "link")
        self.files = so.Files(str(self.repo), [os.path.realpath(self.repo)])

    def test_reads_inside(self):
        self.assertEqual(self.files.call("read_file", {"path": "sub/a.txt", "offset": 2, "limit": 1}),
                         "2\ttwo\n... 1 more lines")
        self.assertIn("sub/", self.files.call("list_dir", {}))

    def test_refuses_outside(self):
        for path in ("../secret", "/etc/passwd", "link", "sub/../../secret"):
            self.assertTrue(self.files.call("read_file", {"path": path}).startswith("error:"), path)

    def test_prefix_is_not_inside(self):
        self.assertIsNone(so.inside(str(self.repo) + "2", [os.path.realpath(self.repo)]))

    def test_unknown_tool_and_bad_args(self):
        self.assertTrue(self.files.call("run", {}).startswith("error:"))
        self.assertTrue(self.files.call("read_file", {"nope": 1}).startswith("error:"))


class Cost(unittest.TestCase):
    def test_prices_change_on_date(self):
        usage = NS(prompt_token_count=1_000_000, candidates_token_count=100_000, thoughts_token_count=0,
                   cached_content_token_count=0, tool_use_prompt_token_count=0)
        self.assertAlmostEqual(so.call_cost("gemini-4-carbon", "standard", usage, 0, "2026-11-15"), 3.0)
        self.assertAlmostEqual(so.call_cost("gemini-4-carbon", "standard", usage, 0, "2026-12-01"), 6.0)
        self.assertAlmostEqual(so.call_cost("gemini-4-carbon", "standard", usage, 10, "2026-11-15"), 3.14)
        self.assertAlmostEqual(so.call_cost("gemini-4-carbon", "flex", usage, 0, "2026-11-15"), 1.5)
        self.assertAlmostEqual(so.call_cost("gemini-4-carbon", "flex", usage, 0, "2026-12-01"), 3.0)

    def test_unknown_model_or_tier_costs_most(self):
        usage = NS(prompt_token_count=1_000_000, candidates_token_count=None)
        self.assertAlmostEqual(so.call_cost("gemini-9", "standard", usage, 0, "2026-10-15"), 4.0)
        self.assertAlmostEqual(so.call_cost("gemini-4-carbon", "priority", usage, 0, "2026-10-15"), 4.0)

    def test_billed_tier(self):
        self.assertEqual(so.billed_tier(NS(service_tier=NS(value="STANDARD")), "flex"), "standard")
        self.assertEqual(so.billed_tier(NS(service_tier=None), "flex"), "flex")
        self.assertEqual(so.billed_tier(NS(), "standard"), "standard")


class Tiers(unittest.TestCase):
    """tiered() on a fake clock: sleep advances it."""

    def setUp(self):
        self.clock = 0.0
        self.calls = []

    def run_tiered(self, outcomes, tier="flex", timeout=1140):
        def call(asked, left):
            self.calls.append((asked, round(left)))
            outcome = outcomes.pop(0)
            if isinstance(outcome, Exception):
                self.clock += 30
                raise outcome
            return outcome

        def sleep(seconds):
            self.clock += seconds
        return so.tiered(call, tier, timeout, lambda e: isinstance(e, TimeoutError), sleep, lambda: self.clock)

    def test_flex_first(self):
        self.assertEqual(self.run_tiered(["ok"]), "ok")
        self.assertEqual(self.calls, [("flex", 840)])

    def test_flex_retries_then_succeeds(self):
        self.assertEqual(self.run_tiered([TimeoutError(), "ok"]), "ok")
        self.assertEqual([c[0] for c in self.calls], ["flex", "flex"])

    def test_standard_when_flex_stays_busy(self):
        self.assertEqual(self.run_tiered([TimeoutError()] * 6 + ["ok"]), "ok")
        self.assertEqual(self.calls[-1][0], "standard")
        self.assertGreaterEqual(self.calls[-1][1], 60)
        self.assertTrue(all(c[0] == "flex" for c in self.calls[:-1]))

    def test_other_errors_are_raised(self):
        with self.assertRaises(ValueError):
            self.run_tiered([ValueError("bad request")])

    def test_standard_tier_goes_straight(self):
        self.assertEqual(self.run_tiered(["ok"], tier="standard"), "ok")
        self.assertEqual(self.calls, [("standard", 1140)])

    def test_no_time_left_for_flex(self):
        self.clock = 900
        self.assertEqual(self.run_tiered(["ok"]), "ok")
        self.assertEqual(self.calls, [("standard", 240)])

    def test_last_try_out_of_time(self):
        with self.assertRaises(so.OutOfTime):
            self.run_tiered([TimeoutError()] * 7)


class Budget(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        base = self.tmp.name
        os.makedirs(f"{base}/view/home/a/repo/sub")
        os.makedirs(f"{base}/view/home/b/repo")
        self.env = {"SO_VIEW": f"{base}/view", "SO_LEDGER": f"{base}/state/ledger.json",
                    "SO_CLIENTS": json.dumps({"a": ["/home/a/repo"], "b": ["/home/b/repo"]}),
                    "SO_MONTHLY_USD": "20", "SO_CALL_USD": "1", "SO_CONVERSATIONS": f"{base}/state/conversations",
                    "SO_USAGE": f"{base}/state/usage.jsonl"}
        self.ledger = so.Ledger(Path(self.env["SO_LEDGER"]))
        self.month = time.strftime("%Y-%m", time.gmtime())

    def test_each_caller_only_its_dirs(self):
        for caller, wanted in (("a", "/home/b/repo"), ("a", "/home/a/repo/../../b/repo"), ("b", "/etc")):
            with self.assertRaisesRegex(so.Refused, "dir must be inside"):
                so.answer({"question": "hi", "dir": wanted}, caller, self.env)
        with self.assertRaisesRegex(so.Refused, "no directories"):
            so.answer({"question": "hi"}, "c", self.env)

    def test_refuses_when_spent(self):
        self.assertEqual(self.ledger.add(self.month, 19.5), 19.5)
        with self.assertRaisesRegex(so.Refused, "budget is used up"):
            so.answer({"question": "hi", "dir": "/home/a/repo/sub"}, "a", self.env)

    def test_reservation_is_settled(self):
        def fake(question, history, cwd, roots, model, call_usd, today, env, spent, stats=None):
            self.assertEqual(cwd, os.path.realpath(self.env["SO_VIEW"] + "/home/a/repo/sub"))
            self.assertAlmostEqual(self.ledger.spent(self.month), 1.0)  # booked while it runs
            spent[0] += 0.25
            return "fine", history + [question]
        with unittest.mock.patch.object(so, "ask_gemini", fake):
            text = so.answer({"question": "hi", "dir": "/home/a/repo/sub"}, "a", self.env)
        self.assertIn("$0.250 for this question; $0.25 of $20.00", text)
        records = [json.loads(line) for line in Path(self.env["SO_USAGE"]).read_text().splitlines()]
        self.assertEqual([(r["caller"], r["outcome"], r["usd"]) for r in records], [("a", "answered", 0.25)])
        self.assertRegex(records[0]["conversation"], so.TAG)

    def test_failure_keeps_what_was_spent(self):
        def fake(question, history, cwd, roots, model, call_usd, today, env, spent, stats=None):
            spent[0] += 0.4
            raise RuntimeError("API down")
        with unittest.mock.patch.object(so, "ask_gemini", fake), self.assertRaises(RuntimeError):
            so.answer({"question": "hi"}, "a", self.env)
        self.assertAlmostEqual(self.ledger.spent(self.month), 0.4)
        record = json.loads(Path(self.env["SO_USAGE"]).read_text())
        self.assertEqual((record["outcome"], record["usd"]), ("failed", 0.4))

    def test_conversations(self):
        seen = []

        def fake(question, history, cwd, roots, model, call_usd, today, env, spent, stats=None):
            seen.append((list(history), cwd))
            return "ok", history + [question]
        with unittest.mock.patch.object(so, "ask_gemini", fake):
            first = so.answer({"question": "one", "dir": "/home/a/repo/sub"}, "a", self.env)
            tag = re.search(r"Conversation (c-\S+):", first).group(1)
            second = so.answer({"question": "two", "continue": tag}, "a", self.env)
            self.assertIn(f"Conversation {tag}:", second)
            # The history carries over, and so does the directory.
            self.assertEqual(seen[1], (["one"], os.path.realpath(self.env["SO_VIEW"] + "/home/a/repo/sub")))
            for caller, wanted in (("b", tag), ("a", "c-nonexistent0"), ("a", "../ledger")):
                with self.assertRaisesRegex(so.Refused, "no conversation"):
                    so.answer({"question": "x", "continue": wanted}, caller, self.env)
        path = Path(self.env["SO_CONVERSATIONS"], f"{tag}.json")
        old = time.time() - 8 * 86400
        os.utime(path, (old, old))
        with self.assertRaisesRegex(so.Refused, "no conversation"):
            so.answer({"question": "x", "continue": tag}, "a", self.env)
        self.assertFalse(path.exists())

    def test_import(self):
        text = so.answer({"import": {"question": "q", "answer": "a"}}, "a", self.env)
        tag = re.search(r"conversation (c-\S+)\.", text).group(1)
        data = json.loads(Path(self.env["SO_CONVERSATIONS"], f"{tag}.json").read_text())
        self.assertEqual(data["contents"][1], {"role": "model", "parts": [{"text": "a"}]})
        self.assertEqual(self.ledger.spent(self.month), 0.0)


class Loop(unittest.TestCase):
    def response(self, *parts, text=None):
        return NS(candidates=[NS(content=NS(parts=list(parts)))], text=text)

    def test_tool_round_then_answer(self):
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "f").write_text("hello\n")
            files = so.Files(tmp, [os.path.realpath(tmp)])
            replies = [self.response(NS(function_call=NS(name="read_file", id="c1", args={"path": "f"}))),
                       self.response(NS(function_call=None, text="ok"), text="done")]
            sent = []

            def generate(contents):
                sent.append(list(contents))
                return replies.pop(0)
            text, spent, done = so.converse(generate, lambda done: ("results", done), ["q"], files,
                                            lambda r: 0.25, 1.0, time.monotonic() + 60)
            self.assertEqual((text, spent, done), ("done", 0.5, True))
            self.assertEqual(sent[1][-1], ("results", [("read_file", "c1", "1\thello")]))

    def test_stops_at_ceiling(self):
        call = self.response(NS(function_call=NS(name="list_dir", id="x", args={})))
        text, spent, done = so.converse(lambda c: call, lambda d: d, ["q"], so.Files("/", ["/"]),
                                        lambda r: 0.6, 1.0, time.monotonic() + 60)
        self.assertFalse(done)
        self.assertIn("ceiling", text)
        self.assertAlmostEqual(spent, 1.2)

    def test_out_of_time_stops(self):
        def generate(contents):
            raise so.OutOfTime("standard: ReadTimeout: timed out")
        text, spent, done = so.converse(generate, lambda d: d, ["q"], so.Files("/", ["/"]),
                                        lambda r: 0.0, 1.0, time.monotonic() + 60)
        self.assertFalse(done)
        self.assertTrue(text.startswith("(stopped: Gemini didn't answer in time"))


if __name__ == "__main__":
    unittest.main()
