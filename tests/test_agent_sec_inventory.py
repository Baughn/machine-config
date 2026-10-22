import importlib.util
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest import mock

source = Path(__file__).resolve().parents[1] / "machines/tsugumi/agent-sec-inventory.py"
spec = importlib.util.spec_from_file_location("agent_sec_inventory", source)
inventory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inventory)


class Argfiles(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(os.path.realpath(self.tmp.name)) / "minecraft"
        self.world = self.root / "incognito"
        self.world.mkdir(parents=True)
        self.outside = Path(self.tmp.name) / "secret"
        self.outside.write_text("hunter2\n")

    def tearDown(self):
        self.tmp.cleanup()

    def test_expands_relative_to_cwd(self):
        (self.world / "user_jvm_args.txt").write_text("# comment\n-Xmx8G\n-Dfoo=\"a b\"  # trailing\n")
        found = inventory.argfiles(["java", "@user_jvm_args.txt", "-jar", "x.jar"], str(self.world), self.root)
        self.assertEqual(found["@user_jvm_args.txt"]["args"], ["-Xmx8G", "-Dfoo=a b"])

    def test_symlink_out_of_root_is_not_read(self):
        (self.world / "evil.txt").symlink_to(self.outside)
        found = inventory.argfiles(["java", "@evil.txt", "@../../secret"], str(self.world), self.root)
        for entry in found.values():
            self.assertNotIn("args", entry)
            self.assertIn("outside", entry["error"])

    def test_skips_escaped_and_missing(self):
        found = inventory.argfiles(["java", "@@literal", "@missing.txt"], str(self.world), self.root)
        self.assertEqual(list(found), ["@missing.txt"])
        self.assertIn("error", found["@missing.txt"])


def finding(pname, version, cves):
    return {"name": f"{pname}-{version}", "pname": pname, "version": version, "affected_by": cves}


class VulnixDiff(unittest.TestCase):
    def test_no_baseline(self):
        self.assertEqual(inventory.vulnix_diff(None, []), {"baseline": None})

    def test_new_gone_changed(self):
        previous = {"generated": 1, "findings": [finding("curl", "8.1", ["CVE-1"]), finding("zlib", "1.3", ["CVE-2"])]}
        now = [finding("curl", "8.2", ["CVE-1", "CVE-3"]), finding("expat", "2.6", ["CVE-4"])]
        diff = inventory.vulnix_diff(previous, now)
        self.assertEqual(diff["baseline"], 1)
        self.assertEqual([p["pname"] for p in diff["new"]], ["expat"])
        self.assertEqual([p["pname"] for p in diff["gone"]], ["zlib"])
        self.assertEqual(diff["changed"], [{"pname": "curl", "versions": ["8.1"], "now": ["8.2"],
                                            "added": ["CVE-3"], "removed": []}])


class VulnixRetries(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.out = Path(self.tmp.name)
        self.env = mock.patch.dict(os.environ, {"VULNIX_CACHE": "/cache", "VULNIX_WHITELIST": ""})
        self.env.start()
        mock.patch.object(inventory, "packages", return_value=self.out / ".packages.json").start()
        mock.patch.object(inventory.time, "sleep").start()

    def tearDown(self):
        mock.patch.stopall()
        self.tmp.cleanup()

    def run_with(self, results):
        with mock.patch.object(inventory.subprocess, "run", side_effect=results) as run:
            try:
                return inventory.vulnix(self.out), run
            except RuntimeError as error:
                return error, run

    def test_retries_then_succeeds_and_diffs_against_last_good(self):
        fail = mock.Mock(returncode=1, stdout="", stderr="ReadTimeout")
        ok = mock.Mock(returncode=2, stdout=json.dumps([finding("curl", "8.1", ["CVE-1"])]), stderr="")
        data, run = self.run_with([fail, fail, ok])
        self.assertEqual(run.call_count, 3)
        self.assertEqual(len(data["failed_attempts"]), 2)
        self.assertEqual(data["diff"], {"baseline": None})
        data, _ = self.run_with([ok])
        self.assertEqual(data["diff"]["changed"], [])
        self.assertEqual(data["diff"]["new"], [])

    def test_gives_up_after_two_retries(self):
        fail = mock.Mock(returncode=1, stdout="", stderr="ReadTimeout")
        error, run = self.run_with([fail, fail, fail])
        self.assertIsInstance(error, RuntimeError)
        self.assertEqual(run.call_count, 3)
        self.assertIn("3 times", str(error))
        self.assertFalse((self.out / ".vulnix-last-good.json").exists())


class Daily(unittest.TestCase):
    def test_first_run_after_five_is_daily(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            at = lambda hour, day=22: time.strptime(f"2026-10-{day} {hour:02}:30", "%Y-%m-%d %H:%M")
            self.assertFalse(inventory.is_daily(out, at(1)))
            self.assertTrue(inventory.is_daily(out, at(5)))
            (out / ".last-daily").write_text("2026-10-22\n")
            self.assertFalse(inventory.is_daily(out, at(9)))
            self.assertFalse(inventory.is_daily(out, at(1, 23)))
            self.assertTrue(inventory.is_daily(out, at(9, 23)))  # 05:30 missed: the next run catches up


if __name__ == "__main__":
    unittest.main()
