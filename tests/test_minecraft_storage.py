import importlib.util
from pathlib import Path
import unittest

source = Path(__file__).resolve().parents[1] / "machines/tsugumi/minecraft-storage.py"
spec = importlib.util.spec_from_file_location("minecraft_storage", source)
storage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(storage)


class StorageTests(unittest.TestCase):
    def test_mount_is_read_only_at_root_owned_location(self):
        argv = storage.command(["mount", "rpool/minecraft/erisia@test"])
        self.assertEqual(argv, [
            "@mount@", "-t", "zfs", "-o", "ro,nosuid,nodev,noexec",
            "--source", "rpool/minecraft/erisia@test",
            "--target", "/run/minecraft-snapshot",
        ])

    def test_rejects_argument_injection_and_other_datasets(self):
        for name in [
            "rpool/minecraft", "rpool/minecraft-other@test", "rpool/root@test",
            "rpool/minecraft/../root@test", "rpool/minecraft@test -r",
            "rpool/minecraft@test\n", "rpool/minecraft/@test", "-r",
        ]:
            for operation in ["snapshot", "rollback", "mount"]:
                with self.subTest(name=name, operation=operation):
                    with self.assertRaises(ValueError):
                        storage.command([operation, name])
        with self.assertRaises(ValueError):
            storage.command(["mount", "rpool/minecraft@test", "--target", "/etc"])
        with self.assertRaises(ValueError):
            storage.command(["status", "-c", "/tmp/script"])

    def test_snapshot_operations(self):
        self.assertEqual(storage.command(["snapshot", "rpool/minecraft@test"]),
                         ["@zfs@", "snapshot", "rpool/minecraft@test"])
        self.assertEqual(storage.command(["rollback", "rpool/minecraft/erisia@test"]),
                         ["@zfs@", "rollback", "-r", "rpool/minecraft/erisia@test"])
        self.assertEqual(storage.command(["unmount"]),
                         ["@umount@", "--", "/run/minecraft-snapshot"])


if __name__ == "__main__":
    unittest.main()
