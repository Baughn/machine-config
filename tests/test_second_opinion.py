import importlib.util
import os
from pathlib import Path
import tempfile
import time
from types import SimpleNamespace as NS
import unittest

source = Path(__file__).resolve().parents[1] / "machines/saya/second-opinion.py"
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
        self.assertAlmostEqual(so.call_cost("gemini-4-argon", usage, 0, "2026-10-15"), 3.0)
        self.assertAlmostEqual(so.call_cost("gemini-4-argon", usage, 0, "2026-12-01"), 6.0)
        self.assertAlmostEqual(so.call_cost("gemini-4-argon", usage, 10, "2026-10-15"), 3.14)

    def test_unknown_model_costs_most(self):
        usage = NS(prompt_token_count=1_000_000, candidates_token_count=None)
        self.assertAlmostEqual(so.call_cost("gemini-9", usage, 0, "2026-10-15"), 4.0)


class Budget(unittest.TestCase):
    def test_ledger_and_refusal(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = {"SO_DIRS": tmp, "SO_LEDGER": f"{tmp}/state/ledger.json", "SO_MONTHLY_USD": "20",
                   "SO_CALL_USD": "1"}
            ledger = so.Ledger(Path(env["SO_LEDGER"]))
            month = time.strftime("%Y-%m", time.gmtime())
            self.assertEqual(ledger.add(month, 19.5), 19.5)
            with self.assertRaisesRegex(so.Refused, "budget is used up"):
                so.answer({"question": "hi"}, env)
            with self.assertRaisesRegex(so.Refused, "dir must be inside"):
                so.answer({"question": "hi", "dir": "/etc"}, env)


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
            text, spent = so.converse(generate, lambda done: ("results", done), "q", files,
                                      lambda r: 0.25, 1.0, time.monotonic() + 60)
            self.assertEqual((text, spent), ("done", 0.5))
            self.assertEqual(sent[1][-1], ("results", [("read_file", "c1", "1\thello")]))

    def test_stops_at_ceiling(self):
        call = self.response(NS(function_call=NS(name="list_dir", id="x", args={})))
        text, spent = so.converse(lambda c: call, lambda d: d, "q", so.Files("/", ["/"]),
                                  lambda r: 0.6, 1.0, time.monotonic() + 60)
        self.assertIn("ceiling", text)
        self.assertAlmostEqual(spent, 1.2)


if __name__ == "__main__":
    unittest.main()
