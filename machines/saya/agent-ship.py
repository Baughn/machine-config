"""agent-ship COMMIT: push an agent's commit to master and deploy it, once Baughn approves.

Runs as svein (his ssh key and sudo), started by the saya agent through polkit.
Starting it proves nothing: this script checks everything itself. It posts the
request through a webhook the agent can't write to or edit, with the diff it
computed itself, and proceeds only on Baughn's ✅ reaction on that message.
See docs/agent-channel-design.md, "The saya identity".
"""

import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

GIT = "@git@"
JJ = "@jj@"
ORIGIN = "@origin@"
BUNDLE = Path("@bundle@")
USER_REPO = Path("@userRepo@")
OWNER = "@owner@"
CHANNEL = "@channel@"
STATE = Path(os.environ.get("STATE_DIRECTORY", "/var/lib/agent-ship"))
CREDENTIALS = Path(os.environ.get("CREDENTIALS_DIRECTORY", "/run/credentials"))
WEBHOOK = CREDENTIALS / "webhook"
TOKEN = CREDENTIALS / "discord-token"

API = "https://discord.com/api/v10"
USER_AGENT = "agent-ship (https://github.com/Baughn/machine-config, 1)"
APPROVE, REFUSE = "✅", "❌"
POLL, APPROVAL_TIMEOUT = 15, 60 * 60
# Discord answers 429 when the bot token (shared with saya's bridge) is busy; a
# ship failed on one while waiting for the ✅ (2026-10-01). Wait and retry.
RATE_LIMIT_RETRIES, RATE_LIMIT_MAX_WAIT = 5, 60.0
COMMIT = re.compile(r"[0-9a-f]{40}\Z")
REFUSED = 3  # a normal outcome, not a unit failure (SuccessExitStatus)


class Refused(Exception):
    pass


def git(repo, *args, check=True):
    result = subprocess.run([GIT, "-C", str(repo), *args], capture_output=True, text=True)
    if check and result.returncode != 0:
        raise Refused(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result


# --- Discord -------------------------------------------------------------------


def request(url, data=None, headers=None, method=None, sleep=time.sleep):
    headers = {"User-Agent": USER_AGENT, **(headers or {})}
    for attempt in range(RATE_LIMIT_RETRIES + 1):
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=30) as response:
                body = response.read()
        except urllib.error.HTTPError as error:
            if error.code != 429 or attempt == RATE_LIMIT_RETRIES:
                raise
            sleep(retry_after(error))
            continue
        return json.loads(body) if body else None


def retry_after(error):
    """Seconds to wait after a 429: Discord's retry_after, else Retry-After, else 5."""
    wait = 5.0
    try:
        wait = float(json.loads(error.read())["retry_after"])
    except (ValueError, KeyError, TypeError):
        try:
            wait = float(error.headers.get("Retry-After", wait))
        except (ValueError, TypeError, AttributeError):
            pass
    return min(max(wait, 0.5), RATE_LIMIT_MAX_WAIT)


def webhook_url():
    return WEBHOOK.read_text().strip()


def webhook_id(url):
    match = re.search(r"/webhooks/(\d+)/", url)
    if match is None:
        raise Refused("the webhook URL has no id")
    return match[1]


def multipart(payload, files):
    boundary = uuid.uuid4().hex
    parts = [f"--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\n"
             f"Content-Type: application/json\r\n\r\n{json.dumps(payload)}\r\n".encode()]
    for index, (name, data) in enumerate(files):
        parts.append(f"--{boundary}\r\nContent-Disposition: form-data; name=\"files[{index}]\"; "
                     f"filename=\"{name}\"\r\nContent-Type: text/plain\r\n\r\n".encode() + data + b"\r\n")
    parts.append(f"--{boundary}--\r\n".encode())
    return b"".join(parts), f"multipart/form-data; boundary={boundary}"


def post(content, files=()):
    """Post through the webhook; returns the message's id."""
    payload = {"content": content[:2000], "username": "saya deploy",
               "allowed_mentions": {"users": [OWNER]}}
    body, kind = multipart(payload, files)
    message = request(webhook_url() + "?wait=true", body, {"Content-Type": kind}, "POST")
    return message["id"]


def bot(path):
    return request(API + path, headers={"Authorization": f"Bot {TOKEN.read_text().strip()}"})


def verdict(message, reactors, hook):
    """APPROVE, REFUSE or None, from a request message and who reacted with what."""
    if message.get("webhook_id") != hook:
        raise Refused("the request message is not the deploy webhook's")
    if message.get("edited_timestamp"):
        raise Refused("the request message was edited")
    if OWNER in reactors.get(REFUSE, ()):
        return REFUSE
    if OWNER in reactors.get(APPROVE, ()):
        return APPROVE
    return None


def reactors(message_id, emoji):
    users = bot(f"/channels/{CHANNEL}/messages/{message_id}/reactions/{urllib.parse.quote(emoji)}?limit=100")
    return [u["id"] for u in users]


def await_verdict(message_id, hook, sleep=time.sleep, clock=time.monotonic):
    deadline = clock() + APPROVAL_TIMEOUT
    while clock() < deadline:
        sleep(POLL)
        message = bot(f"/channels/{CHANNEL}/messages/{message_id}")
        present = {r["emoji"]["name"] for r in message.get("reactions", [])}
        who = {e: reactors(message_id, e) for e in (APPROVE, REFUSE) if e in present}
        decided = verdict(message, who, hook)
        if decided is not None:
            return decided
    return None


# --- git -----------------------------------------------------------------------


def prepare(repo, commit):
    """Update the clone, fetch the agent's bundle, and check the commit fast-forwards master."""
    if not (repo / ".git").exists():
        subprocess.run([GIT, "clone", ORIGIN, str(repo)], check=True, capture_output=True)
    git(repo, "fetch", "--prune", "origin")
    git(repo, "fetch", str(BUNDLE), "+refs/heads/*:refs/agent/*")
    if git(repo, "cat-file", "-e", f"{commit}^{{commit}}", check=False).returncode != 0:
        raise Refused(f"{commit} is not in the agent's bundle; run agent-publish")
    master = git(repo, "rev-parse", "origin/master").stdout.strip()
    if master == commit:
        raise Refused("that commit is already master")
    if git(repo, "merge-base", "--is-ancestor", "origin/master", commit, check=False).returncode != 0:
        raise Refused("the commit is not based on the current master; rebase onto master@origin and ship again")
    log = git(repo, "log", "--format=%h %s", f"origin/master..{commit}").stdout.strip()
    stat = git(repo, "diff", "--stat", "origin/master", commit).stdout.strip()
    diff = git(repo, "diff", "origin/master", commit).stdout
    return log, stat, diff


def request_text(commit, log, stat):
    lines = [f"<@{OWNER}> 🚀 **saya asks to push `{commit[:12]}` to master and deploy it**",
             f"commit `{commit}`", "```", log[:600], "```", "```", stat[-700:], "```",
             f"React {APPROVE} to push and deploy, {REFUSE} to refuse. Full diff attached."]
    return "\n".join(lines)


# --- main ----------------------------------------------------------------------


def ship(commit, out, deploy=None, check=None, sleep=time.sleep):
    # A commit that is already in master (shipped before) fails the fast-forward check.
    repo = STATE / "nixos"
    check = check or run_check
    log, stat, diff = prepare(repo, commit)
    git(repo, "checkout", "--detach", "--force", commit)
    check(repo)
    hook = webhook_id(webhook_url())
    message_id = post(request_text(commit, log, stat), [(f"{commit[:12]}.diff", diff.encode())])
    print(f"request posted as {message_id}; waiting for Baughn", file=out, flush=True)
    decided = await_verdict(message_id, hook, sleep)
    if decided != APPROVE:
        raise Refused("refused by Baughn" if decided == REFUSE else "no approval within an hour")
    check(repo)  # again: someone may have deployed during the wait
    git(repo, "push", "origin", f"{commit}:refs/heads/master")
    print(f"pushed {commit} to master", file=out, flush=True)
    status = (deploy or run_deploy)(repo, out)
    subprocess.run([JJ, "-R", str(USER_REPO), "git", "fetch"], capture_output=True)
    return status


def run_check(repo):
    """Refuse unless every machine runs an ancestor of the checked-out commit.

    Otherwise deploying would roll back work deployed from another checkout,
    such as Baughn's unpushed commits. --strict also refuses when a machine
    runs uncommitted changes, or doesn't record what it runs.
    """
    result = subprocess.run(["deploy", "--check", "--strict"], cwd=repo, capture_output=True, text=True)
    if result.returncode != 0:
        lines = [l for l in result.stderr.splitlines() if not l.startswith("warning: Git tree")]
        raise Refused("deploying would clobber what the machines run:\n" + "\n".join(lines)[-1200:])


def run_deploy(repo, out):
    return subprocess.run(["deploy", "--mode", "switch", "--strict"], cwd=repo, stdout=out,
                          stderr=subprocess.STDOUT).returncode


def tail(path, size=1500):
    try:
        return path.read_text()[-size:]
    except OSError:
        return ""


def main(argv):
    if len(argv) != 2 or COMMIT.fullmatch(argv[1]) is None:
        print("usage: agent-ship COMMIT (40 hex digits)", file=sys.stderr)
        return 2
    commit = argv[1]
    os.umask(0o022)  # the log is read by the agent
    log_path = STATE / f"{commit}.log"
    with open(STATE / "lock", "w") as lock, open(log_path, "w") as out:
        fcntl.flock(lock, fcntl.LOCK_EX)
        try:
            status = ship(commit, out)
        except Refused as error:
            print(f"refused: {error}", file=out, flush=True)
            post(f"🛑 `{commit[:12]}` not shipped: {error}")
            return REFUSED
        except Exception as error:  # report, then fail the unit
            print(f"failed: {type(error).__name__}: {error}", file=out, flush=True)
            post(f"✗ shipping `{commit[:12]}` failed: {type(error).__name__}: {error}"[:1900])
            raise
        finally:
            # The log (deploy output included) also goes to the journal.
            sys.stdout.write(tail(log_path, 100_000))
    outcome = "deployed" if status == 0 else f"pushed, but deploy exited {status}"
    post(f"{'✅' if status == 0 else '⚠️'} `{commit[:12]}` {outcome}.\n```\n{tail(log_path)[-1500:]}\n```")
    return 0 if status == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
