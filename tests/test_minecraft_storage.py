import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import MagicMock, patch

source = Path(__file__).resolve().parents[1] / "machines/tsugumi/minecraft-storage.py"
spec = importlib.util.spec_from_file_location("minecraft_storage", source)
storage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(storage)

LIVE = "rpool/minecraft/erisia"
BACKUP = storage.BACKUP_PREFIX + LIVE


def version(name, guid, txg, creation=None):
    return dict(name=name, guid=guid, txg=txg, creation=creation or txg,
                snapshot="@" in name, columns=["1024", "-", "2048", "-"])


class StorageTests(unittest.TestCase):
    def test_public_interface(self):
        for operation in ("snapshot", "rollback", "mount"):
            self.assertEqual(storage.command([operation, LIVE + "@test"]),
                             (operation, LIVE + "@test"))
        for operation in ("status", "pools", "snapshots", "unmount"):
            self.assertEqual(storage.command([operation]), (operation, None))

    def test_rejects_argument_injection_and_other_datasets(self):
        for name in [
            "rpool/minecraft", "rpool/minecraft-other@test", "rpool/root@test",
            "rpool/minecraft/../root@test", "rpool/minecraft@test -r",
            "rpool/minecraft@test\n", "rpool/minecraft/@test", "-r",
            BACKUP + "@test", LIVE + "#bookmark", LIVE + "@" + "a" * 256,
        ]:
            for operation in ["snapshot", "rollback", "mount"]:
                with self.subTest(name=name, operation=operation):
                    with self.assertRaises(ValueError):
                        storage.command([operation, name])
        for args in (["mount", LIVE + "@test", "--target", "/etc"], ["status", "-c", "evil"]):
            with self.assertRaises(ValueError):
                storage.command(args)

    def test_merge_preserves_names_and_prefers_ssd(self):
        local = version(LIVE + "@new", 2, 200, 20)
        remote = version(BACKUP + "@new", 2, 5, 20)
        old = version(BACKUP + "@old", 1, 4, 10)
        bookmark = version(LIVE + "#cursor", 2, 200, 20)
        self.assertEqual(storage.merge_snapshots([remote, old, local, bookmark]),
                         [(LIVE + "@old", old), (LIVE + "@new", local)])
        with self.assertRaisesRegex(ValueError, "Conflicting"):
            storage.merge_snapshots([local, dict(remote, guid=99)])
        conflicts = set()
        self.assertEqual(storage.merge_snapshots([dict(remote, guid=99), local], conflicts),
                         [(LIVE + "@new", local)])
        self.assertEqual(conflicts, {LIVE + "@new"})

    def test_listing_survives_a_conflict_and_avoids_scientific_notation(self):
        local = dict(version(LIVE + "@new", 2, 200, 20), columns=["1047552", "-", "1975000000", "-"])
        remote = version(BACKUP + "@new", 99, 5, 20)
        other = version(BACKUP + "@old", 1, 4, 10)
        with patch.object(storage, "filesystems", return_value={storage.ROOT, storage.BACKUP_PREFIX + storage.ROOT}), \
             patch.object(storage, "versions", side_effect=[[local], [remote, other]]), \
             patch("sys.stdout", new_callable=io.StringIO) as output, \
             patch("sys.stderr", new_callable=io.StringIO) as errors:
            storage.list_snapshots()
        self.assertEqual(output.getvalue(),
                         LIVE + "@old\t1K\t-\t2K\t-\n" + LIVE + "@new\t1023K\t-\t1.84G\t-\n")
        self.assertIn("Conflicting SSD/HDD snapshot GUIDs", errors.getvalue())
        self.assertIn(LIVE + "@new", errors.getvalue())

    def test_stale_mount_record_for_missing_dataset_is_dropped(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(storage, "STATE", Path(directory)), \
             patch.object(storage, "mounts", return_value=[]), \
             patch.object(storage, "filesystems", return_value=set()), \
             patch.object(storage, "snapshots") as snapshots, \
             patch.object(storage, "release") as release:
            storage.write_state("mount.json", version(BACKUP + "@old", 1, 1))
            storage.clean_mount_hold()
            self.assertFalse((Path(directory) / "mount.json").exists())
            snapshots.assert_not_called()
            release.assert_not_called()

    def test_listing_is_compatible_with_existing_wrappers(self):
        entry = version(BACKUP + "@old", 1, 10)
        with patch.object(storage, "filesystems", return_value={storage.ROOT, storage.BACKUP_PREFIX + storage.ROOT}), \
             patch.object(storage, "versions", side_effect=[[], [entry]]), \
             patch("sys.stdout", new_callable=io.StringIO) as output:
            storage.list_snapshots()
        self.assertEqual(output.getvalue(), LIVE + "@old\t1K\t-\t2K\t-\n")

    def test_filters_follow_longest_matching_dataset(self):
        filters = {"rpool<": True, LIVE + "/dynmap": False, LIVE + "/private<": False}
        with patch.object(storage, "FILTERS", json.dumps(filters)):
            self.assertTrue(storage.replicated(LIVE))
            self.assertFalse(storage.replicated(LIVE + "/dynmap"))
            self.assertTrue(storage.replicated(LIVE + "/dynmap/child"))
            self.assertFalse(storage.replicated(LIVE + "/private/child"))
            self.assertFalse(storage.replicated("other/world"))

    def test_local_rollback_finds_surviving_bookmark_base(self):
        target = version(LIVE + "@manual", 2, 200, 20)
        bookmark = version(LIVE + "#older", 1, 100, 10)
        recent = version(LIVE + "@recent", 3, 300, 30)
        remote = [version(BACKUP + "@old", 1, 9000, 10),
                  version(BACKUP + "@recent", 3, 9500, 30)]
        plan = storage.choose_restore(target["name"], [target, bookmark, recent], remote, True)
        self.assertEqual(plan["method"], "local")
        self.assertEqual(plan["anchor"]["sender"], bookmark)
        self.assertEqual(plan["remove_live"], [recent])
        self.assertEqual(plan["remove_backup"], [remote[1]])

    def test_hdd_incremental_uses_guid_and_pool_local_txg(self):
        live = [version(LIVE + "@renamed", 1, 9000, 10), version(LIVE + "@new", 3, 9900, 30)]
        backup = [version(BACKUP + "@base", 1, 2, 10),
                  version(BACKUP + "@target", 2, 3, 20),
                  version(BACKUP + "@new", 3, 4, 30)]
        plan = storage.choose_restore(LIVE + "@target", live, backup, True)
        self.assertEqual(plan["method"], "incremental")
        self.assertEqual(plan["base"]["receiver"], live[0])
        self.assertEqual(plan["base"]["sender"], backup[0])
        self.assertEqual(plan["remove_live"], live[1:])
        self.assertEqual(plan["remove_backup"], backup[2:])

    def test_full_receive_only_discards_newer_ssd_history(self):
        target = version(BACKUP + "@old", 1, 10000, 10)
        live = [version(LIVE + "@new", 2, 2, 20)]
        plan = storage.choose_restore(LIVE + "@old", live, [target], True)
        self.assertEqual(plan["method"], "full")
        self.assertEqual(plan["remove_live"], live)
        with self.assertRaisesRegex(ValueError, "older SSD-only"):
            storage.choose_restore(LIVE + "@old", [dict(live[0], creation=5)], [target], True)

    def test_refuses_no_common_replica_and_excluded_hdd_restore(self):
        live = version(LIVE + "@local", 1, 1)
        remote = version(BACKUP + "@remote", 2, 2)
        with self.assertRaisesRegex(ValueError, "No surviving common"):
            storage.choose_restore(live["name"], [live], [remote], True)
        with self.assertRaisesRegex(ValueError, "excluded"):
            storage.choose_restore(LIVE + "@remote", [live], [remote], False)
        plan = storage.choose_restore(live["name"], [live], [], False)
        self.assertIsNone(plan["backup"])

    def test_remove_planned_preserves_foreign_holds(self):
        entry = version(LIVE + "@new", 2, 2)
        with patch.object(storage, "versions", return_value=[entry]), \
             patch.object(storage, "holds", return_value=["someone-else"]), \
             patch.object(storage, "zfs") as zfs:
            with self.assertRaisesRegex(ValueError, "Unexpected hold"):
                storage.remove_planned([entry])
            zfs.assert_not_called()

    def test_mount_target_is_read_only_and_held(self):
        entry = version(BACKUP + "@old", 1, 1)
        with patch.object(storage, "mounts", return_value=[]), \
             patch.object(storage.os.path, "ismount", return_value=False), \
             patch.object(storage, "resolve", return_value=entry), \
             patch.object(storage, "write_state"), patch.object(storage, "hold") as hold, \
             patch.object(storage, "clean_mount_hold"), patch.object(storage, "run") as run:
            storage.mount_snapshot(LIVE + "@old")
        hold.assert_called_once_with(entry["name"], storage.MOUNT_HOLD)
        run.assert_called_once_with("@mount@", "-i", "-t", "zfs", "-o",
                                    "ro,nosuid,nodev,noexec", "--source", entry["name"],
                                    "--target", storage.MOUNTPOINT)

    def test_directory_traversal_rejects_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / "link").symlink_to("/tmp", target_is_directory=True)
            with self.assertRaises(OSError):
                storage.safe_directory(str(path / "link"))

    def test_journal_retained_and_daemon_not_restarted_after_failure(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(storage, "STATE", Path(directory)), \
             patch.object(storage, "run") as run, \
             patch.object(storage, "resume_rollback", side_effect=ValueError("receive failed")), \
             patch("sys.stderr", new_callable=io.StringIO):
            plan = {"name": LIVE + "@old", "was_active": True, "version": 1}
            storage.write_state("rollback.json", plan)
            with self.assertRaisesRegex(ValueError, "receive failed"):
                storage.rollback(plan["name"])
            self.assertTrue((Path(directory) / "rollback.json").exists())
            run.assert_called_once_with("@systemctl@", "stop", "zrepl.service")
            with self.assertRaisesRegex(ValueError, "Recovery pending"):
                storage.rollback(LIVE + "@different")

    def test_completed_recovery_respects_prior_service_state(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(storage, "STATE", Path(directory)), patch.object(storage, "run") as run:
            run.return_value.returncode = 0
            storage.write_state("complete.json", {"was_active": False, "wake": True})
            storage.finish_restart()
            run.assert_not_called()
            storage.write_state("complete.json", {"was_active": True, "wake": True})
            storage.finish_restart()
            self.assertEqual([call.args for call in run.call_args_list], [
                ("@systemctl@", "start", "zrepl.service"),
                ("@zrepl@", "signal", "wakeup", "rpool"),
            ])

    def test_both_pipeline_exit_codes_are_checked(self):
        for sender_status, receiver_status in ((1, 0), (0, 1)):
            with self.subTest(sender=sender_status, receiver=receiver_status), \
                 patch.object(storage.subprocess, "Popen") as popen, \
                 patch.object(storage.subprocess, "run") as run:
                sender = popen.return_value.__enter__.return_value
                sender.wait.return_value = sender_status
                run.return_value.returncode = receiver_status
                with self.assertRaisesRegex(ValueError, "ZFS transfer failed"):
                    storage.send_receive(BACKUP + "@old", LIVE)
                sender.stdout.close.assert_called_once()
                sender.wait.assert_called_once()

    def test_wakeup_waits_for_control_socket(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(storage, "STATE", Path(directory)), \
             patch.object(storage, "run") as run, patch.object(storage.time, "sleep") as sleep:
            run.side_effect = [MagicMock(returncode=0), MagicMock(returncode=1), MagicMock(returncode=0)]
            storage.write_state("complete.json", {"was_active": True, "wake": True})
            storage.finish_restart()
            sleep.assert_called_once_with(0.1)
            self.assertFalse((Path(directory) / "complete.json").exists())

    def test_children_retain_operation_lock(self):
        with patch.object(storage, "LOCK_FD", 42), patch.object(storage.subprocess, "run") as run:
            storage.run("test", pass_fds=(43,))
            self.assertEqual(run.call_args.kwargs["pass_fds"], (43, 42))
            self.assertEqual(run.call_args.kwargs["env"], storage.ENV)


if __name__ == "__main__":
    unittest.main()
