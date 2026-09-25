import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

source = Path(__file__).resolve().parents[1] / "machines/tsugumi/minecraft-watch.py"
spec = importlib.util.spec_from_file_location("minecraft_watch", source)
watch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(watch)

NOW = 2_000_000_000
SETTINGS = dict(snapshotAge=2700, replicaAge=7200, leaseAge=600, bootGrace=2700,
                loopRestarts=3, loopWindow=3600, confirm=2)
FILTERS = json.dumps({"rpool<": True, "rpool/minecraft/testing<": False})


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        for directory in ("state", "leases", "storage"):
            (root / directory).mkdir()
        self.root = root
        self.snapshots = {"rpool/minecraft/erisia": NOW - 600,
                          "stash/zrepl/rpool/rpool/minecraft/erisia": NOW - 1800,
                          "rpool/minecraft/testing": NOW - 600}
        self.units = {"zrepl.service": {"ActiveState": "active"},
                      "minecraft-save-recovery.service": {"ActiveState": "inactive"},
                      "minecraft@erisia.service": {"ActiveState": "active", "SubState": "running",
                                                   "NRestarts": "0"}}
        self.posts = []
        self.fail_post = False

        def post(content):
            if self.fail_post:
                raise OSError("network down")
            self.posts.append(content)

        for name, value in {
            "STATE": root / "state", "LEASES": root / "leases", "STORAGE": root / "storage",
            "FILTERS": FILTERS, "AUTOSTART": json.dumps(["erisia"]), "OWNER": "42",
            "MENTION": "<@42> ", "SETTINGS": dict(SETTINGS),
            "worlds": lambda: ["rpool/minecraft/erisia", "rpool/minecraft/testing"],
            "exists": lambda dataset: dataset in self.snapshots,
            "newest": lambda dataset: self.snapshots.get(dataset),
            "unit": lambda name, *props: self.units.get(name, {"ActiveState": "inactive",
                                                               "SubState": "dead"}),
            "uptime": lambda: 86400.0,
            "post": post,
        }.items():
            patcher = patch.object(watch, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.addCleanup(self.tmp.cleanup)

    def run_watch(self, times=1, now=NOW):
        for _ in range(times):
            code = watch.watch(now)
        return code

    def failing(self):
        results, _ = watch.evaluate(NOW, {})
        return {key: result.detail for key, result in results.items() if not result.ok}


class CheckTests(Fixture):
    def test_healthy_system_passes_everything(self):
        self.assertEqual(self.failing(), {})

    def test_stale_and_missing_snapshots(self):
        self.snapshots["rpool/minecraft/erisia"] = NOW - 2701
        self.snapshots.pop("rpool/minecraft/testing")
        failing = self.failing()
        self.assertIn("45 min old", failing["snapshot:erisia"])
        self.assertIn("no zrepl_ snapshot", failing["snapshot:testing"])

    def test_replica_is_checked_only_for_replicated_worlds(self):
        self.snapshots["stash/zrepl/rpool/rpool/minecraft/erisia"] = NOW - 7201
        self.assertIn("2 h 0 min old", self.failing()["replica:erisia"])
        self.assertNotIn("replica:testing", self.failing())
        self.snapshots.pop("stash/zrepl/rpool/rpool/minecraft/erisia")
        self.assertIn("has no zrepl_", self.failing()["replica:erisia"])

    def test_boot_grace_skips_snapshot_verdicts(self):
        self.snapshots["rpool/minecraft/erisia"] = NOW - 99999
        with patch.object(watch, "uptime", lambda: 60.0):
            results, skipped = watch.evaluate(NOW, {})
        self.assertEqual(skipped, {"snapshot", "replica"})
        self.assertNotIn("snapshot:erisia", results)

    def test_stuck_save_lease(self):
        lease = self.root / "leases/erisia"
        lease.mkdir()
        (lease / "lock").touch()
        self.assertEqual(self.failing(), {})
        (lease / "pending.json").write_text("{}")
        os.utime(lease / "pending.json", (NOW - 300, NOW - 300))
        self.assertEqual(self.failing(), {})
        os.utime(lease / "pending.json", (NOW - 601, NOW - 601))
        self.assertIn("off for 10 min", self.failing()["lease:erisia"])

    def test_failed_recovery_unit(self):
        self.units["minecraft-save-recovery.service"] = {"ActiveState": "failed"}
        self.assertIn("lease:recovery", self.failing())

    def test_rollback_states(self):
        (self.root / "storage/rollback.json").write_text("{}")
        self.units["zrepl.service"] = {"ActiveState": "inactive"}
        self.assertEqual(set(self.failing()), {"zrepl:rollback"})
        (self.root / "storage/rollback.json").unlink()
        (self.root / "storage/complete.json").write_text("{}")
        self.assertEqual(set(self.failing()), {"zrepl:restart", "zrepl:active"})

    def test_world_down_and_restart_loop(self):
        self.units["minecraft@erisia.service"] = {"ActiveState": "failed", "SubState": "failed",
                                                  "NRestarts": "0"}
        self.assertIn("is failed", self.failing()["world:erisia"])
        state = {}
        unit = self.units["minecraft@erisia.service"] = {"ActiveState": "active",
                                                         "SubState": "running", "NRestarts": "5"}
        for offset, count in ((0, "5"), (600, "6"), (1200, "7")):
            unit["NRestarts"] = count
            results, _ = watch.evaluate(NOW + offset, state)
            self.assertTrue(results["world:erisia"].ok)
        unit["NRestarts"] = "8"
        results, _ = watch.evaluate(NOW + 1800, state)
        self.assertIn("restarted 3 times", results["world:erisia"].detail)
        # Old samples age out of the window.
        results, _ = watch.evaluate(NOW + 1800 + 3601, state)
        self.assertTrue(results["world:erisia"].ok)

    def test_manual_start_resets_restart_history(self):
        state = {}
        unit = self.units["minecraft@erisia.service"]
        unit["NRestarts"] = "10"
        watch.evaluate(NOW, state)
        unit["NRestarts"] = "1"
        results, _ = watch.evaluate(NOW + 300, state)
        self.assertTrue(results["world:erisia"].ok)
        self.assertEqual(state["restarts"]["erisia"], [[NOW + 300, 1]])

    def test_a_crashing_check_is_reported_and_keeps_its_keys(self):
        def broken():
            raise RuntimeError("zfs exploded")
        with patch.object(watch, "worlds", broken):
            results, skipped = watch.evaluate(NOW, {})
        self.assertIn("zfs exploded", results["error:snapshot"].detail)
        self.assertIn("snapshot", skipped)


class AlertTests(Fixture):
    def break_snapshot(self):
        self.snapshots["rpool/minecraft/erisia"] = NOW - 9999

    def test_fires_once_after_confirmation_and_resolves_once(self):
        self.run_watch()
        self.assertEqual(self.posts, [])
        self.break_snapshot()
        self.run_watch()
        self.assertEqual(self.posts, [])
        self.run_watch(3)
        self.assertEqual(len(self.posts), 1)
        self.assertTrue(self.posts[0].startswith("<@42> "))
        self.assertIn("snapshot:erisia", self.posts[0])
        self.snapshots["rpool/minecraft/erisia"] = NOW
        self.run_watch(3)
        self.assertEqual(len(self.posts), 2)
        self.assertIn("resolved", self.posts[1])
        self.assertFalse(self.posts[1].startswith("<@"))

    def test_a_blip_does_not_fire(self):
        self.break_snapshot()
        self.run_watch()
        self.snapshots["rpool/minecraft/erisia"] = NOW
        self.run_watch()
        self.break_snapshot()
        self.run_watch()
        self.assertEqual(self.posts, [])

    def test_failed_post_is_retried(self):
        self.break_snapshot()
        self.fail_post = True
        self.assertEqual(self.run_watch(2), 1)
        self.fail_post = False
        self.run_watch()
        self.assertEqual(len(self.posts), 1)
        self.run_watch()
        self.assertEqual(len(self.posts), 1)
        self.snapshots["rpool/minecraft/erisia"] = NOW
        self.fail_post = True
        self.run_watch()
        self.fail_post = False
        self.run_watch()
        self.assertEqual(len(self.posts), 2)
        self.assertIn("resolved", self.posts[1])

    def test_skipped_check_keeps_firing_state(self):
        self.break_snapshot()
        self.run_watch(2)
        with patch.object(watch, "uptime", lambda: 60.0):
            self.run_watch()
        self.assertEqual(len(self.posts), 1)
        self.snapshots["rpool/minecraft/erisia"] = NOW
        self.run_watch()
        self.assertIn("resolved", self.posts[-1])

    def test_vanished_subject_resolves(self):
        lease = self.root / "leases/old"
        lease.mkdir()
        (lease / "pending.json").write_text("{}")
        os.utime(lease / "pending.json", (NOW - 9999, NOW - 9999))
        self.run_watch(2)
        self.assertIn("lease:old", self.posts[0])
        (lease / "pending.json").unlink()
        lease.rmdir()
        self.run_watch()
        self.assertIn("resolved: `lease:old`", self.posts[1])

    def test_long_messages_are_capped(self):
        firing = [(f"snapshot:world{i}", "x" * 80) for i in range(100)]
        text = watch.message(firing, [], {})
        self.assertLessEqual(len(text), 2000)
        self.assertIn("more; run `minecraft-watch status`", text)


if __name__ == "__main__":
    unittest.main()
