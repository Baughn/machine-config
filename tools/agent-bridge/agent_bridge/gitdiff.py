"""The obelisk's diff waterfall from git: new +/- lines in the agent's work repos.

Edit/Write tool inputs miss edits made by shell commands (sed, python, generators), so the
bridge asks git instead (LabAssistant, msg 1573494373587492896). After a tool call that can
edit, each work repo's `git diff HEAD` (plus untracked files) is compared with the last look,
and lines not there before are new. When HEAD moved, commits made since the last look (by
committer time, so pulls and fast-forwards of others' old work don't count) add the lines that
never showed in the working tree: an edit and a commit in one command (msg 1573494576092553269).
A commit's patch-id is remembered, so a rebase doesn't replay it.

Only source files in repos named like the work repos show (render.diff_path_allowed); notes,
clones and world data never do. The first look at a repo is a silent baseline.
"""

from __future__ import annotations

from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass, field
import os
from pathlib import Path
import subprocess
import time

from .render import DIFF_EXCLUDED, DIFF_HUNK_LINES, DIFF_LINE, DIFF_REPOS, diff_path_allowed

GIT = "git"
GIT_TIMEOUT = 10.0
MAX_OUTPUT = 2 * 1024 * 1024  # a diff bigger than this is a mass change, not news
MAX_NEW_FILE = 64 * 1024
MAX_COMMITS = 20
COMMIT_SLACK = 120  # seconds of clock skew allowed for "committed since the last look"
REPO_DEPTH = 3
RESCAN_REPOS = 120.0  # seconds between looks for new repos
PATCH_IDS_KEPT = 2000

Hunk = tuple[str, list[str]]  # (file basename, +/- lines)


@dataclass
class RepoState:
    looked: bool = False  # the first look is a silent baseline
    head: str | None = None
    seen_at: float = 0.0
    files: dict[str, Counter[str]] = field(default_factory=dict)  # worktree diff lines per file


def _git(repo: Path, *args: str, stdin: bytes | None = None) -> bytes | None:
    try:
        result = subprocess.run([GIT, "-C", str(repo), *args], input=stdin, capture_output=True,
                                timeout=GIT_TIMEOUT, check=False,
                                env={**os.environ, "GIT_OPTIONAL_LOCKS": "0", "LC_ALL": "C"})
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stdout if result.returncode == 0 else None


def _clean(line: str) -> str:
    line = line.expandtabs(4)[:DIFF_LINE]
    # The board refuses lines over 300 bytes.
    return line.encode()[:300].decode(errors="ignore")


def parse_diff(text: str) -> dict[str, list[str]]:
    """`git diff -U0` output -> {path: +/- lines}, in order."""
    files: dict[str, list[str]] = {}
    current: list[str] | None = None
    old = "/dev/null"
    for line in text.splitlines():
        if line.startswith("diff --git "):
            current, old = None, "/dev/null"
        elif current is None and line.startswith("--- "):
            old = line[4:]
        elif current is None and line.startswith("+++ "):
            new = line[4:]
            # A deletion's lines go under the old name.
            path = (old if new == "/dev/null" else new).split("/", 1)[-1]
            current = files.setdefault(path, [])
        elif current is not None and line[:1] in ("+", "-"):
            current.append(_clean(line))
    return files


def repo_allowed(path: Path, root: Path | None = None) -> bool:
    """A repo named like a work repo, in no excluded directory below root."""
    parts = [part.lower() for part in (path.relative_to(root) if root else path).parts]
    if any(part in DIFF_EXCLUDED for part in parts):
        return False
    name = parts[-1] if parts else ""
    return any(name == repo or name.startswith((repo + "-", repo + "_")) for repo in DIFF_REPOS)


def find_repos(root: Path, depth: int = REPO_DEPTH) -> list[Path]:
    """Git repos and worktrees under root (not inside other repos) named like work repos."""
    found: list[Path] = []

    def walk(directory: Path, level: int) -> None:
        try:
            entries = sorted(os.scandir(directory), key=lambda e: e.name)
        except OSError:
            return
        if any(entry.name == ".git" for entry in entries):
            if repo_allowed(directory, root):
                found.append(directory)
            return
        if level >= depth:
            return
        for entry in entries:
            if entry.is_dir(follow_symlinks=False) and not entry.name.startswith("."):
                if entry.name.lower() not in DIFF_EXCLUDED:
                    walk(Path(entry.path), level + 1)

    walk(root, 0)
    return found


def _new_lines(current: list[str], before: Counter[str]) -> list[str]:
    """Lines of current beyond those counted in before (consumed from it), in order."""
    left = Counter(before)
    out = []
    for line in current:
        if left[line] > 0:
            left[line] -= 1
        else:
            out.append(line)
    return out


class GitDiffs:
    """Watches the work repos under the agent's workdir for new diff lines."""

    def __init__(self, root: Path, clock: Callable[[], float] | None = None) -> None:
        self.root = root
        self.clock = clock or time.time
        self.repos: dict[Path, RepoState] = {}
        self.repos_at = float("-inf")
        self.patch_ids: dict[str, None] = {}  # insertion-ordered set

    def scan(self) -> list[Hunk]:
        now = self.clock()
        if now - self.repos_at >= RESCAN_REPOS:
            self.repos_at = now
            present = find_repos(self.root)
            self.repos = {repo: self.repos.get(repo, RepoState()) for repo in present}
        hunks: list[Hunk] = []
        for repo, state in self.repos.items():
            hunks.extend(self.scan_repo(repo, state, now))
        return hunks

    def allowed(self, repo: Path, path: str) -> bool:
        """diff_path_allowed below the workdir (where the workdir itself lives doesn't matter)."""
        return diff_path_allowed(str((repo / path).relative_to(self.root)))

    def worktree(self, repo: Path) -> dict[str, list[str]] | None:
        out = _git(repo, "diff", "--no-color", "--no-ext-diff", "--no-renames", "-U0", "HEAD", "--")
        if out is None or len(out) > MAX_OUTPUT:
            return None
        files = parse_diff(out.decode(errors="replace"))
        untracked = _git(repo, "ls-files", "--others", "--exclude-standard", "-z")
        for name in (untracked or b"").decode(errors="replace").split("\0"):
            if not name or not self.allowed(repo, name):
                continue
            try:
                data = (repo / name).read_bytes()[:MAX_NEW_FILE]
            except OSError:
                continue
            files[name] = [_clean("+" + line) for line in data.decode(errors="replace").splitlines()]
        return files

    def commits(self, repo: Path, old: str, new: str, since: float) -> list[tuple[str, str]]:
        """(commit, patch-id) of non-merge commits in old..new committed since `since`."""
        out = _git(repo, "log", "--no-merges", f"--max-count={MAX_COMMITS}", "--format=%H %ct", f"{old}..{new}")
        result = []
        for line in reversed((out or b"").decode().splitlines()):
            sha, _, stamp = line.partition(" ")
            if not stamp.isdigit() or int(stamp) < since - COMMIT_SLACK:
                continue
            patch = _git(repo, "show", "--no-color", "--no-ext-diff", "--format=", "-U0", sha)
            if patch is None or len(patch) > MAX_OUTPUT:
                continue
            ids = _git(repo, "patch-id", "--stable", stdin=patch)
            pid = (ids or b"").decode().split(" ", 1)[0] or sha
            result.append((sha, pid))
        return result

    def scan_repo(self, repo: Path, state: RepoState, now: float) -> list[Hunk]:
        head_out = _git(repo, "rev-parse", "--verify", "-q", "HEAD")
        head = head_out.decode().strip() if head_out else None
        files = self.worktree(repo)
        if files is None:
            return []
        baseline = not state.looked
        new: dict[str, list[str]] = {}
        if not baseline and head and state.head and head != state.head:
            for sha, pid in self.commits(repo, state.head, head, state.seen_at):
                if pid in self.patch_ids:
                    continue
                self.patch_ids[pid] = None
                patch = _git(repo, "show", "--no-color", "--no-ext-diff", "--format=", "-U0", sha)
                for path, lines in parse_diff((patch or b"").decode(errors="replace")).items():
                    before = state.files.get(path, Counter())
                    fresh = _new_lines(lines, before)
                    # Lines the worktree showed before are spent: the commit only took them over.
                    state.files[path] = before - Counter(lines)
                    new.setdefault(path, []).extend(fresh)
            while len(self.patch_ids) > PATCH_IDS_KEPT:
                del self.patch_ids[next(iter(self.patch_ids))]
        if not baseline:
            for path, lines in files.items():
                new.setdefault(path, []).extend(_new_lines(lines, state.files.get(path, Counter())))
        state.looked, state.head, state.seen_at = True, head, now
        state.files = {path: Counter(lines) for path, lines in files.items()}
        return [(Path(path).name, lines[:DIFF_HUNK_LINES]) for path, lines in new.items()
                if lines and self.allowed(repo, path)]
