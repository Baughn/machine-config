from __future__ import annotations

import os
from pathlib import Path
import subprocess

from agent_bridge import gitdiff
from agent_bridge.gitdiff import GitDiffs, find_repos, parse_diff

ENV = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@x", "GIT_COMMITTER_NAME": "t",
       "GIT_COMMITTER_EMAIL": "t@x", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}


def git(repo: Path, *args: str, when: int | None = None) -> str:
    env = dict(ENV)
    if when is not None:
        env |= {"GIT_AUTHOR_DATE": f"@{when} +0000", "GIT_COMMITTER_DATE": f"@{when} +0000"}
    result = subprocess.run(["git", "-C", str(repo), *args], env=env, capture_output=True, text=True)
    assert result.returncode == 0, result.stderr
    return result.stdout


def make_repo(root: Path, name: str = "eln") -> Path:
    repo = root / "src" / name
    repo.mkdir(parents=True)
    git(repo, "init", "-q", "-b", "main")
    (repo / "A.java").write_text("one\ntwo\n}\n")
    git(repo, "add", ".")
    git(repo, "commit", "-qm", "init", when=1_000)
    return repo


class Clock:
    def __init__(self) -> None:
        self.t = 2_000_000_000.0

    def __call__(self) -> float:
        return self.t


def test_parse_diff_keeps_signs_and_files() -> None:
    text = ("diff --git a/A.java b/A.java\n--- a/A.java\n+++ b/A.java\n@@ -2 +2 @@\n-two\n+2\n"
            "diff --git a/Gone.kt b/Gone.kt\ndeleted file mode 100644\n--- a/Gone.kt\n+++ /dev/null\n"
            "@@ -1 +0,0 @@\n-bye\n")
    assert parse_diff(text) == {"A.java": ["-two", "+2"], "Gone.kt": ["-bye"]}


def test_find_repos_skips_unnamed_and_excluded(tmp_path: Path) -> None:
    for name in ("eln", "eln-wt-floppy", "somewhere"):
        (tmp_path / "src" / name / ".git").mkdir(parents=True)
    (tmp_path / "notes" / "stasis" / ".git").mkdir(parents=True)
    assert [p.name for p in find_repos(tmp_path)] == ["eln", "eln-wt-floppy"]


def test_shell_edits_show_once_and_baseline_is_silent(tmp_path: Path) -> None:
    repo = make_repo(tmp_path)
    (repo / "A.java").write_text("one\nTWO\n}\n")  # dirty before the bridge starts
    diffs = GitDiffs(tmp_path, Clock())
    assert diffs.scan() == []
    (repo / "A.java").write_text("one\nTWO\n}\nthree\nfour\n")
    (repo / "notes.md").write_text("not source\n")
    (repo / "New.kt").write_text("fun x() {}\n")
    hunks = dict(diffs.scan())
    assert hunks == {"A.java": ["+three", "+four"], "New.kt": ["+fun x() {}"]}
    assert diffs.scan() == []


def test_edit_and_commit_in_one_go_shows_the_commit(tmp_path: Path) -> None:
    repo = make_repo(tmp_path)
    clock = Clock()
    diffs = GitDiffs(tmp_path, clock)
    diffs.scan()
    (repo / "A.java").write_text("one\ntwo\n}\nfour\n")
    git(repo, "commit", "-qam", "four", when=int(clock.t) + 5)
    clock.t += 10
    assert diffs.scan() == [("A.java", ["+four"])]
    # A commit of lines already seen in the worktree isn't news again.
    (repo / "A.java").write_text("one\ntwo\n}\nfour\nfive\n")
    assert diffs.scan() == [("A.java", ["+five"])]
    git(repo, "commit", "-qam", "five", when=int(clock.t) + 5)
    clock.t += 10
    assert diffs.scan() == []


def test_pulled_old_commits_and_rebases_dont_show(tmp_path: Path) -> None:
    repo = make_repo(tmp_path)
    clock = Clock()
    diffs = GitDiffs(tmp_path, clock)
    diffs.scan()
    (repo / "A.java").write_text("one\ntwo\n}\nothers\n")
    git(repo, "commit", "-qam", "someone else's, long ago", when=1_500)
    assert diffs.scan() == []
    # Commit, then rebase it (new committer time, same patch): shown once.
    (repo / "B.java").write_text("mine\n")
    git(repo, "add", "B.java")
    git(repo, "commit", "-qm", "mine", when=int(clock.t))
    clock.t += 10
    assert diffs.scan() == [("B.java", ["+mine"])]
    git(repo, "commit", "-q", "--amend", "--no-edit", "--reset-author", when=int(clock.t))
    clock.t += 10
    assert diffs.scan() == []


def test_a_broken_repo_is_skipped(tmp_path: Path) -> None:
    (tmp_path / "src" / "eln" / ".git").mkdir(parents=True)
    assert GitDiffs(tmp_path, Clock()).scan() == []
    assert gitdiff.repo_allowed(Path("/w/src/stasis"))
