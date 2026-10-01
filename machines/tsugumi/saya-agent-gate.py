# Forced command for saya-agent's ssh key: read-only journalctl and systemctl.
# sshd puts the requested command in SSH_ORIGINAL_COMMAND; it is split like a
# shell would, checked, and exec'd directly (no shell).
import os
import shlex
import sys

JOURNALCTL = "@journalctl@"
SYSTEMCTL = "@systemctl@"

# systemctl's first argument must be one of these verbs.
READ_ONLY_VERBS = {
    "status", "show", "cat",
    "list-units", "list-unit-files", "list-timers", "list-sockets",
    "list-dependencies", "list-jobs", "list-machines", "list-paths",
    "is-active", "is-failed", "is-enabled", "is-system-running",
}
# Options that reach another host or root, or would change the journal.
# Most of the latter need root anyway; refuse them regardless.
DENIED = {
    "systemctl": ("-H", "--host", "-M", "--machine", "--root", "--image"),
    "journalctl": (
        "-H", "--host", "-M", "--machine", "--root", "--image",
        "--vacuum-size", "--vacuum-time", "--vacuum-files", "--rotate",
        "--flush", "--sync", "--relinquish-var", "--smart-relinquish-var",
        "--setup-keys", "--update-catalog",
    ),
}


def refuse(why):
    print(f"saya-agent-gate: {why}", file=sys.stderr)
    print("allowed: journalctl …, systemctl <"
          + "|".join(sorted(READ_ONLY_VERBS)) + "> …", file=sys.stderr)
    sys.exit(126)


try:
    argv = shlex.split(os.environ.get("SSH_ORIGINAL_COMMAND", ""))
except ValueError as e:
    refuse(f"can't parse the command: {e}")
if not argv:
    refuse("no command given")

prog, args = argv[0], argv[1:]
if prog not in DENIED:
    refuse(f"{prog!r} isn't allowed")
if prog == "systemctl" and (not args or args[0] not in READ_ONLY_VERBS):
    refuse("systemctl needs a read-only verb as its first argument")
for arg in args:
    for opt in DENIED[prog]:
        if opt.startswith("--"):
            bad = arg == opt or arg.startswith(opt + "=")
        else:
            # Short options combine (-bH host) and carry values (-Hhost), so
            # refuse the letter anywhere in a cluster; at worst a value
            # containing it is refused too.
            bad = arg.startswith("-") and not arg.startswith("--") and opt[1] in arg[1:]
        if bad:
            refuse(f"option {opt} isn't allowed")

os.environ.update(SYSTEMD_PAGER="cat", PAGER="cat", SYSTEMD_COLORS="0")
path = JOURNALCTL if prog == "journalctl" else SYSTEMCTL
os.execv(path, [prog, "--no-pager", *args])
