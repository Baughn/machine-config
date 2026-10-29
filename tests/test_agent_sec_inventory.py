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


SS = """tcp LISTEN 0 4096 0.0.0.0:25565 0.0.0.0:* users:(("java",pid=10,fd=5))
tcp ESTAB 0 0 192.0.2.1:25565 81.2.69.7:50000 users:(("java",pid=10,fd=6))
tcp ESTAB 0 0 192.0.2.1:25565 81.2.69.9:50001 users:(("java",pid=10,fd=7))
tcp ESTAB 0 0 192.0.2.1:41000 198.51.100.5:4444 users:(("sh",pid=11,fd=3))
tcp ESTAB 0 0 127.0.0.1:9090 127.0.0.1:41234 users:(("prometheus",pid=12,fd=3))
udp ESTAB 0 0 [2001:db8::1]:50754 [2a00:1450:400b:c01::5e]:443
"""


class Flows(unittest.TestCase):
    def test_sockets(self):
        owners = {10: {"exe": "/nix/store/x-jdk/bin/java", "unit": "minecraft.service", "user": "minecraft"},
                  11: {"exe": "/tmp/sh", "unit": "caddy.service", "user": "caddy"}}
        got = inventory.classify_sockets(inventory.parse_ss(SS), owners)
        self.assertEqual(got["loopback_connections"], 1)
        self.assertEqual(got["inbound"], [{"proto": "tcp", "local_port": "25565", "connections": 2, "sources": 1,
                                           "top_prefixes": {"81.2.69.0/24": 2}}])
        peers = {(o["peer"], o["port"], o["unit"]) for o in got["outbound"]}
        self.assertEqual(peers, {("198.51.100.5", "4444", "caddy.service"), ("2a00:1450:400b:c01::5e", "443", "")})
        self.assertNotIn("81.2.69.7", json.dumps(got))

    def test_conntrack(self):
        text = ("tcp      6 431996 ESTABLISHED src=192.0.2.1 dst=198.51.100.5 sport=41000 dport=443 "
                "src=198.51.100.5 dst=192.0.2.1 sport=443 dport=41000 [ASSURED] mark=0 use=1\n"
                "udp      17 29 src=81.2.69.7 dst=192.0.2.1 sport=5000 dport=24454 [UNREPLIED] "
                "src=192.0.2.1 dst=81.2.69.7 sport=24454 dport=5000 mark=0 use=1\n"
                "icmp     1 29 src=10.171.0.2 dst=10.0.0.9 type=8 code=0 id=1 src=10.0.0.9 dst=10.171.0.2 "
                "type=0 code=0 id=1 mark=0 use=1\n")
        got = inventory.parse_conntrack(text, {"192.0.2.1"})
        self.assertEqual(got["outbound"], [{"proto": "tcp", "peer": "198.51.100.5", "port": "443", "count": 1}])
        self.assertEqual(got["inbound"][0]["top_prefixes"], {"81.2.69.0/24": 1})
        self.assertEqual(got["forwarded"], 1)

    def test_new_outbound_survives_and_expires(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            self.assertEqual(inventory.mark_new(out, ".seen.json", ["a"], 1000.0), [])  # first run: baseline
            self.assertEqual(inventory.mark_new(out, ".seen.json", ["a", "b"], 2000.0), ["b"])
            later = 2000.0 + 31 * 86400
            self.assertEqual(inventory.mark_new(out, ".seen.json", ["a"], later), ["a"])


class Processes(unittest.TestCase):
    def test_redact(self):
        text = inventory.redact(["tool", "--token=abc", "PASSWORD:hunter2", "--port=1"], 1024)
        self.assertNotIn("abc", text)
        self.assertNotIn("hunter2", text)
        self.assertIn("--port=1", text)
        self.assertTrue(inventory.redact(["x" * 50], 10).endswith("…"))

    def test_redact_separate_values(self):
        text = inventory.redact(["tool", "--token=a b", "--password", "hunter2", "-p", "s3cret",
                                 "https://user:pw@example.org/x", "--port", "8"], 1024)
        for secret in ("a b", "hunter2", "s3cret", "user:pw"):
            self.assertNotIn(secret, text)
        self.assertIn("--port 8", text)

    def test_unit_of(self):
        self.assertEqual(inventory.unit_of("0::/system.slice/agent-bridge-saya.service\n"), "agent-bridge-saya.service")
        self.assertEqual(inventory.unit_of("0::/user.slice/user-1000.slice/user@1000.service/app.slice/x.service/sub\n"),
                         "x.service")
        self.assertEqual(inventory.unit_of("0::/init.scope\n"), "init.scope")

    def test_proc_stat(self):
        text = "42 (we ird) S 1 42 42 0 -1 4194304 1 0 0 0 30 12 0 0 20 0 1 0 5000 1000 10"
        self.assertEqual(inventory.proc_stat(text), (1, 42, 5000))

    def test_flags(self):
        store = "/nix/store/abc-bash/bin/bash"
        self.assertEqual(inventory.process_flags("bash", store, "/", 0, "sshd.service"), [])
        self.assertIn("shell or interpreter under a system user", inventory.process_flags("bash", store, "/", 990, "caddy.service"))
        self.assertIn("exe deleted", inventory.process_flags("x", "/nix/store/a-x/bin/x (deleted)", "/", 0, "a.service"))
        got = inventory.process_flags("x", "/dev/shm/x", "/", 0, "system.slice")
        self.assertEqual(set(got), {"exe outside /nix/store", "runs from a temp dir", "not in a unit"})


class Load(unittest.TestCase):
    def test_baseline_and_unusual(self):
        state = {}
        now = 0.0
        for i in range(8):
            usage = int(i * 4 * 3600 * 1e6 * 0.1)  # 0.1 core
            _, state = inventory.load_report(state, now, {"a.service": (usage, 2**20)})
            now += 4 * 3600
        report, state = inventory.load_report(state, now, {"a.service": (int(usage + 4 * 3600 * 1e6 * 2), 2**20)})
        self.assertEqual(report["unusual"], ["a.service"])
        self.assertEqual(report["units"]["a.service"]["cpu_cores"], 2.0)
        report, _ = inventory.load_report({}, now, {"a.service": (1, 2**20)})
        self.assertNotIn("cpu_cores", report["units"]["a.service"])

    def test_cgroups_include_template_instances(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for group in ("system.slice/caddy.service", "system.slice/system-minecraft\\x2dlab.slice/minecraft-lab@x.service"):
                (root / group).mkdir(parents=True)
                (root / group / "cpu.stat").write_text("usage_usec 5\nuser_usec 3\n")
                (root / group / "memory.current").write_text("1024\n")
            self.assertEqual(inventory.cgroups(root), {"caddy.service": (5, 1024), "minecraft-lab@x.service": (5, 1024)})

    def test_rerun_soon_keeps_the_state(self):
        state = {"time": 0, "usage": {"a.service": 0}, "history": {}}
        report, new = inventory.load_report(state, 60.0, {"a.service": (10**6, 0)})
        self.assertIs(new, state)
        self.assertIn("skipped", report)

    def test_restarted_unit_is_skipped(self):
        report, _ = inventory.load_report({"time": 0, "usage": {"a.service": 10**9}}, 3600.0, {"a.service": (5, 0)})
        self.assertNotIn("cpu_cores", report["units"]["a.service"])


class Auth(unittest.TestCase):
    def test_parse(self):
        entries = [
            {"SYSLOG_IDENTIFIER": "sshd-session", "MESSAGE": "Accepted publickey for svein from 81.2.69.7 port 5000 ssh2: ED25519 SHA256:abcDEF+/1"},
            {"SYSLOG_IDENTIFIER": "sshd-session", "MESSAGE": "Accepted publickey for svein from 81.2.69.8 port 5001 ssh2: "
             "ED25519-CERT SHA256:certKEY ID svein@kaho (serial 7) CA ED25519 SHA256:caKEY"},
            {"SYSLOG_IDENTIFIER": "sshd", "MESSAGE": "Failed password for invalid user admin from 81.2.69.9 port 1 ssh2"},
            {"SYSLOG_IDENTIFIER": "sshd", "MESSAGE": "Invalid user oracle from 81.2.69.10 port 2"},
            {"SYSLOG_IDENTIFIER": "sudo", "__REALTIME_TIMESTAMP": "2000000",
             "MESSAGE": "   svein : TTY=pts/1 ; PWD=/home/svein ; USER=root ; COMMAND=/run/current-system/sw/bin/tool --token=abc"},
            {"SYSLOG_IDENTIFIER": "systemd-logind", "MESSAGE": "New session 12 of user svein."},
        ]
        got = inventory.parse_auth(entries, b"k")
        self.assertEqual(got["ssh_accepted"][0]["user"], "svein")
        self.assertTrue(got["ssh_accepted"][0]["source"].startswith("src-"))
        keys = sorted((a["key_type"], a["fingerprint"], a.get("cert_id"), a.get("cert_serial"), a.get("ca_fingerprint"))
                      for a in got["ssh_accepted"])
        self.assertEqual(keys, [("ED25519", "SHA256:abcDEF+/1", None, None, None),
                                ("ED25519-CERT", "SHA256:certKEY", "svein@kaho", "7", "SHA256:caKEY")])
        self.assertNotIn("81.2.69.7", json.dumps(got))
        self.assertEqual(got["ssh_failed"]["total"], 2)
        self.assertEqual(got["ssh_failed"]["users"], {"admin": 1, "oracle": 1})
        self.assertEqual(got["ssh_failed"]["top_prefixes"], {"81.2.69.0/24": 2})
        self.assertEqual(got["sudo"][0]["as"], "root")
        self.assertNotIn("abc", got["sudo"][0]["command"])
        self.assertEqual(got["sudo"][0]["time"], 2)
        self.assertEqual(got["new_sessions"], {"svein": 1})

    def test_compare(self):
        self.assertEqual(inventory.compare({"a": "1", "b": "2"}, {"b": "3", "c": "4"}),
                         {"added": ["c"], "removed": ["a"], "changed": ["b"]})

    def test_mapped_v4(self):
        self.assertEqual(inventory.split_address("[::ffff:81.2.69.7]:443"), ("81.2.69.7", "443"))


class Persistence(unittest.TestCase):
    def test_foreign_units(self):
        with tempfile.TemporaryDirectory() as tmp:
            local = Path(tmp) / "evil.service"
            local.write_text("[Service]\n")
            units = [
                {"Id": "good.service", "FragmentPath": "/nix/store/abc-unit/good.service", "DropInPaths": "", "Transient": "no"},
                {"Id": "evil.service", "FragmentPath": str(local), "DropInPaths": "", "Transient": "no"},
                {"Id": "-.mount", "FragmentPath": "/run/systemd/generator/-.mount", "DropInPaths": "", "Transient": "no"},
                {"Id": "run-u5.service", "FragmentPath": "/run/systemd/transient/run-u5.service", "DropInPaths": "", "Transient": "yes"},
                {"Id": "session-3.scope", "FragmentPath": "", "DropInPaths": "", "Transient": "yes"},
                {"Id": "x.service", "FragmentPath": "/nix/store/abc-unit/x.service", "DropInPaths": str(local), "Transient": "no"},
            ]
            got = inventory.foreign_units(units)
            self.assertEqual([f["unit"] for f in got["foreign"]], ["evil.service", "x.service"])
            self.assertEqual(got["transient"], ["run-u5.service"])
            masked = inventory.foreign_units([{"Id": "m.service", "FragmentPath": "/dev/null", "Transient": "no"}])
            self.assertEqual((masked["foreign"], masked["masked"]), ([], ["m.service"]))


if __name__ == "__main__":
    unittest.main()
