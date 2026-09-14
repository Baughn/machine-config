import fcntl
import importlib.util
import json
from pathlib import Path
import socket
import struct
import tempfile
import threading
import unittest
from unittest.mock import MagicMock, patch

source = Path(__file__).resolve().parents[1] / "machines/tsugumi/minecraft-snapshot.py"
spec = importlib.util.spec_from_file_location("minecraft_snapshot", source)
snapshot = importlib.util.module_from_spec(spec)
spec.loader.exec_module(snapshot)

OFF = "Turned off world auto-saving"
ON = "Turned on world auto-saving"
SAVED = "Saving...Flushing all saves...Flushing completedSaved the world"


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for name in ("WORLDS", "STATE"):
            p = patch.object(snapshot, name, self.root / name)
            p.start().mkdir()
            self.addCleanup(p.stop)
        self.hook = snapshot.SnapshotHook("erisia")
        self.hook.world.mkdir()
        self.hook.state.mkdir()
        self.hook.world.joinpath("server.properties").write_text(
            "enable-rcon=true\nrcon.port=35565\nrcon.password=test\\:password\n")
        self.client = MagicMock()
        p = patch.object(snapshot, "connect")
        self.connection = p.start()
        self.addCleanup(p.stop)
        self.connection.return_value.__enter__.return_value = self.client

    def record(self, deadline=0, name="test"):
        self.hook.record.write_text(json.dumps({"snapshot": name, "deadline": deadline}))

    def commands(self):
        return [c.args[0] for c in self.client.command.call_args_list]

    def test_save_flush_snapshot_then_resume(self):
        self.client.command.side_effect = [OFF, SAVED, ON]
        self.hook.run("pre_snapshot", "test")
        self.assertEqual(self.commands(), ["save-off", "save-all flush"])
        self.assertTrue(self.hook.record.exists())
        self.hook.run("recover")  # Timer must leave a fresh save alone.
        self.assertEqual(len(self.commands()), 2)
        self.hook.run("post_snapshot", "test")
        self.assertEqual(self.commands(), ["save-off", "save-all flush", "save-on"])
        self.assertFalse(self.hook.record.exists())

    def test_closed_port_allows_snapshot(self):
        self.connection.side_effect = ConnectionRefusedError()
        self.hook.run("pre_snapshot", "test")
        self.hook.run("post_snapshot", "test")
        self.assertFalse(self.hook.record.exists())

    def test_authentication_errors_and_timeouts_do_not_allow_snapshot(self):
        for error in (ValueError("authentication failed"), TimeoutError()):
            with self.subTest(error=error):
                self.connection.side_effect = error
                with self.assertRaises(type(error)):
                    self.hook.run("pre_snapshot", "test")

    def test_failed_or_incomplete_flush_restores_saving_and_fails(self):
        for reply in ("Saving failed: disk full", "Saved the world", "Unknown command"):
            with self.subTest(reply=reply):
                self.client.command.side_effect = [OFF, reply, ON]
                with self.assertRaisesRegex(ValueError, "completed save-all"):
                    self.hook.run("pre_snapshot", "test")
                self.assertEqual(self.commands()[-1], "save-on")
                self.assertFalse(self.hook.record.exists())

    def test_lost_save_off_reply_still_attempts_recovery(self):
        self.client.command.side_effect = [TimeoutError(), ON]
        with self.assertRaises(TimeoutError):
            self.hook.run("pre_snapshot", "test")
        self.assertEqual(self.commands(), ["save-off", "save-on"])
        self.assertFalse(self.hook.record.exists())

    def test_failed_cleanup_is_retried_by_independent_timer(self):
        self.client.command.side_effect = [OFF, TimeoutError(), TimeoutError()]
        with self.assertRaises(TimeoutError):
            self.hook.run("pre_snapshot", "test")
        self.assertTrue(self.hook.record.exists())
        self.record()  # Expired lease, including a hook killed without cleanup.
        self.client.command.side_effect = [ON]
        self.hook.run("recover")
        self.assertFalse(self.hook.record.exists())

    def test_post_failure_keeps_recovery_record(self):
        self.record()
        self.client.command.return_value = "Unknown command"
        with self.assertRaisesRegex(ValueError, "confirm save-on"):
            self.hook.run("post_snapshot", "test")
        self.assertTrue(self.hook.record.exists())

    def test_existing_manual_save_off_is_preserved(self):
        self.client.command.return_value = "Saving is already turned off"
        with self.assertRaisesRegex(ValueError, "already disabled"):
            self.hook.run("pre_snapshot", "test")
        self.assertEqual(self.commands(), ["save-off"])
        self.assertFalse(self.hook.record.exists())

    def test_new_snapshot_recovers_abandoned_save_first(self):
        self.record(name="old")
        self.client.command.side_effect = [ON, OFF, SAVED]
        self.hook.run("pre_snapshot", "new")
        self.assertEqual(self.commands(), ["save-on", "save-off", "save-all flush"])
        self.assertEqual(self.hook.pending()["snapshot"], "new")

    def test_stopped_server_needs_no_recovery(self):
        self.record()
        self.connection.side_effect = ConnectionRefusedError()
        self.hook.run("recover")
        self.assertFalse(self.hook.record.exists())

    def test_recovery_does_not_interrupt_active_hook(self):
        self.record()
        with self.hook.state.joinpath("lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            self.hook.run("recover")
        self.connection.assert_not_called()
        self.assertTrue(self.hook.record.exists())

    def test_dry_run_does_not_connect_or_create_state(self):
        with patch("sys.argv", ["hook"]), patch.dict("os.environ", {
            "ZREPL_FS": "rpool/minecraft/new-server",
            "ZREPL_HOOKTYPE": "pre_snapshot", "ZREPL_DRYRUN": "true",
        }, clear=True):
            snapshot.main()
        self.connection.assert_not_called()
        self.assertFalse((snapshot.STATE / "new-server").exists())

    def test_parent_and_auxiliary_datasets_do_not_run_hooks(self):
        for dataset in ("rpool/minecraft", "rpool/minecraft/erisia/dynmap"):
            with patch("sys.argv", ["hook"]), patch.dict("os.environ", {
                "ZREPL_FS": dataset, "ZREPL_HOOKTYPE": "pre_snapshot",
            }, clear=True):
                snapshot.main()
        self.connection.assert_not_called()

    def test_instance_names_cannot_escape_root(self):
        for name in ("..", "../erisia", "/tmp", "", "a/b"):
            with self.assertRaises(ValueError):
                snapshot.SnapshotHook(name)


class ProtocolTests(unittest.TestCase):
    def test_fragmented_tcp_response_and_authentication(self):
        client_sock, server_sock = socket.socketpair()
        self.addCleanup(client_sock.close)
        self.addCleanup(server_sock.close)
        client_sock.settimeout(1)
        server_sock.settimeout(1)
        received = []

        def server():
            with server_sock.makefile("rb", buffering=0) as stream:
                for kind, reply in ((2, ""), (0, SAVED)):
                    size, = struct.unpack("<i", stream.read(4))
                    body = stream.read(size)
                    sequence, request_kind = struct.unpack("<ii", body[:8])
                    received.append((request_kind, body[8:-2].decode()))
                    response = struct.pack("<ii", sequence, kind) + reply.encode() + b"\0\0"
                    for byte in struct.pack("<i", len(response)) + response:
                        server_sock.sendall(bytes([byte]))

        thread = threading.Thread(target=server, daemon=True)
        thread.start()
        client = snapshot.Rcon(client_sock)
        self.assertEqual(client.request(3, "password", timeout=1), "")
        self.assertEqual(client.command("save-all flush", timeout=1), SAVED)
        thread.join(timeout=2)
        self.assertFalse(thread.is_alive())
        self.assertEqual(received, [(3, "password"), (2, "save-all flush")])

    def test_rejects_authentication_failure_without_logging_password(self):
        sock = MagicMock()
        sock.recv.side_effect = [struct.pack("<i", 10), struct.pack("<ii", -1, 2) + b"\0\0"]
        with self.assertRaisesRegex(ValueError, "authentication failed") as error:
            snapshot.Rcon(sock).request(3, "secret")
        self.assertNotIn("secret", str(error.exception))

    def test_closed_port_is_checked_before_disabled_legacy_rcon(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory)
            world.joinpath("server.properties").write_text("enable-rcon=false\n")
            with patch.object(snapshot, "port_listening", return_value=True), \
                 patch.object(socket, "create_connection", side_effect=ConnectionRefusedError()):
                with self.assertRaises(ConnectionRefusedError):
                    with snapshot.connect(world):
                        self.fail("A refused connection must not yield an RCON client")

    def test_closed_filtered_port_does_not_attempt_connection(self):
        with tempfile.TemporaryDirectory() as directory:
            world = Path(directory)
            world.joinpath("server.properties").write_text("enable-rcon=false\n")
            with patch.object(snapshot, "port_listening", return_value=False), \
                 patch.object(socket, "create_connection") as connect:
                with self.assertRaises(ConnectionRefusedError):
                    with snapshot.connect(world):
                        self.fail("A closed port must be treated as offline")
                connect.assert_not_called()

    def test_listener_check_covers_ipv6_and_ignores_established_connections(self):
        with tempfile.TemporaryDirectory() as directory:
            tcp, tcp6 = (Path(directory) / name for name in ("tcp", "tcp6"))
            tcp.write_text("header\n0: 0100007F:63DD 00000000:0000 01\n")
            tcp6.write_text("header\n0: 00000000000000000000000000000000:8AED "
                            "00000000000000000000000000000000:0000 0A\n")
            with patch.object(snapshot, "TCP_TABLES", (tcp, tcp6)):
                self.assertTrue(snapshot.port_listening(35565))
                self.assertFalse(snapshot.port_listening(25565))
                tcp6.unlink()
                self.assertFalse(snapshot.port_listening(35565))


if __name__ == "__main__":
    unittest.main()
