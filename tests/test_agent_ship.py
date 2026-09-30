import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

source = Path(__file__).resolve().parents[1] / "machines/saya/agent-ship.py"
spec = importlib.util.spec_from_file_location("agent_ship", source)
ship = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ship)

OWNER, HOOK = "1", "77"
ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t",
       "GIT_COMMITTER_EMAIL": "t@t", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}


def sh(cwd, *argv):
    return subprocess.run(argv, cwd=cwd, env=ENV, check=True, capture_output=True, text=True).stdout.strip()


class Verdict(unittest.TestCase):
    def setUp(self):
        patcher = patch.object(ship, "OWNER", OWNER)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.message = {"webhook_id": HOOK, "edited_timestamp": None}

    def test_only_the_owner_decides(self):
        self.assertIsNone(ship.verdict(self.message, {}, HOOK))
        self.assertIsNone(ship.verdict(self.message, {ship.APPROVE: ["2", "3"]}, HOOK))
        self.assertEqual(ship.verdict(self.message, {ship.APPROVE: ["2", OWNER]}, HOOK), ship.APPROVE)
        both = {ship.APPROVE: [OWNER], ship.REFUSE: [OWNER]}
        self.assertEqual(ship.verdict(self.message, both, HOOK), ship.REFUSE)

    def test_only_our_unedited_webhook_message_counts(self):
        approved = {ship.APPROVE: [OWNER]}
        with self.assertRaises(ship.Refused):
            ship.verdict({**self.message, "webhook_id": None}, approved, HOOK)  # e.g. posted by the bot
        with self.assertRaises(ship.Refused):
            ship.verdict({**self.message, "edited_timestamp": "2026-09-26T20:00:00"}, approved, HOOK)

    def test_waiting_times_out(self):
        clock = iter(range(0, 10_000, 1000))
        with patch.object(ship, "bot", return_value={"webhook_id": HOOK, "reactions": []}):
            self.assertIsNone(ship.await_verdict("m", HOOK, sleep=lambda s: None, clock=lambda: next(clock)))


class Ship(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        self.root = root
        sh(root, "git", "init", "-q", "--bare", "-b", "master", "origin.git")
        sh(root, "git", "clone", "-q", "origin.git", "upstream")
        self.upstream = root / "upstream"
        (self.upstream / "a").write_text("a\n")
        sh(self.upstream, "git", "add", "a")
        sh(self.upstream, "git", "commit", "-q", "-m", "base")
        sh(self.upstream, "git", "push", "-q", "origin", "master")
        # The agent's clone: one commit on top of master, published as a bundle.
        sh(root, "git", "clone", "-q", "origin.git", "agent")
        self.agent = root / "agent"
        (self.agent / "a").write_text("b\n")
        sh(self.agent, "git", "commit", "-q", "-am", "agent change")
        sh(self.agent, "git", "branch", "saya/topic")
        self.commit = sh(self.agent, "git", "rev-parse", "HEAD")
        self.bundle = root / "nixos.bundle"
        sh(self.agent, "git", "bundle", "create", "-q", str(self.bundle), "--branches=saya/*")
        (root / "state").mkdir()
        (root / "webhook").write_text(f"https://discord.com/api/webhooks/{HOOK}/secret\n")
        for name, value in dict(GIT="git", JJ="true", ORIGIN=str(root / "origin.git"), BUNDLE=self.bundle,
                                STATE=root / "state", WEBHOOK=root / "webhook", OWNER=OWNER).items():
            patcher = patch.object(ship, name, value)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.posts = []
        patcher = patch.object(ship, "post", side_effect=lambda content, files=(): self.posts.append(content) or "m1")
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = patch.dict(os.environ, {k: v for k, v in ENV.items() if k.startswith("GIT_")})
        patcher.start()
        self.addCleanup(patcher.stop)

    def origin_master(self):
        return sh(self.root, "git", "--git-dir", "origin.git", "rev-parse", "master")

    def run_ship(self, decision, check=lambda repo: None):
        deployed = []
        with patch.object(ship, "await_verdict", return_value=decision):
            status = ship.ship(self.commit, io.StringIO(), check=check,
                               deploy=lambda repo, out: deployed.append(sh(repo, "git", "rev-parse", "HEAD")) or 0)
        return status, deployed

    def test_approved_commit_is_pushed_and_deployed(self):
        status, deployed = self.run_ship(ship.APPROVE)
        self.assertEqual(status, 0)
        self.assertEqual(self.origin_master(), self.commit)
        self.assertEqual(deployed, [self.commit])
        self.assertIn(self.commit, self.posts[0])
        self.assertIn(f"<@{OWNER}>", self.posts[0])

    def test_refused_or_unanswered_commit_is_not_pushed(self):
        before = self.origin_master()
        for decision in (ship.REFUSE, None):
            with self.assertRaises(ship.Refused):
                self.run_ship(decision)
        self.assertEqual(self.origin_master(), before)

    def test_only_fast_forwards(self):
        # master moved on meanwhile: the agent must rebase first.
        (self.upstream / "b").write_text("b\n")
        sh(self.upstream, "git", "add", "b")
        sh(self.upstream, "git", "commit", "-q", "-m", "other")
        sh(self.upstream, "git", "push", "-q", "origin", "master")
        with self.assertRaisesRegex(ship.Refused, "rebase"):
            self.run_ship(ship.APPROVE)
        self.assertEqual(self.posts, [])  # nothing asked of Baughn

    def test_lineage_is_checked_on_the_commit_before_asking_and_before_pushing(self):
        checked = []
        self.run_ship(ship.APPROVE, check=lambda repo: checked.append(sh(repo, "git", "rev-parse", "HEAD")))
        self.assertEqual(checked, [self.commit, self.commit])

    def test_clobbering_what_runs_is_refused_before_asking(self):
        def clobbers(repo):
            raise ship.Refused("deploying would clobber what the machines run")
        before = self.origin_master()
        with self.assertRaisesRegex(ship.Refused, "clobber"):
            self.run_ship(ship.APPROVE, check=clobbers)
        self.assertEqual(self.posts, [])
        self.assertEqual(self.origin_master(), before)

    def test_deploys_during_the_wait_are_caught_before_pushing(self):
        calls = []
        def second_fails(repo):
            calls.append(repo)
            if len(calls) == 2:
                raise ship.Refused("deploying would clobber what the machines run")
        before = self.origin_master()
        with self.assertRaisesRegex(ship.Refused, "clobber"):
            self.run_ship(ship.APPROVE, check=second_fails)
        self.assertEqual(len(self.posts), 1)
        self.assertEqual(self.origin_master(), before)

    def test_shipped_or_unknown_commits_are_refused(self):
        self.run_ship(ship.APPROVE)
        with self.assertRaisesRegex(ship.Refused, "already master"):
            self.run_ship(ship.APPROVE)
        self.commit = "f" * 40
        with self.assertRaisesRegex(ship.Refused, "not in the agent's bundle"):
            self.run_ship(ship.APPROVE)

    def test_bad_arguments(self):
        self.assertEqual(ship.main(["agent-ship", "master"]), 2)


if __name__ == "__main__":
    unittest.main()
