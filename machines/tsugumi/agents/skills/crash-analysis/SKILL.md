---
name: crash-analysis
description: Post-mortem of a crashed Erisia Minecraft server from the snapshot crash_analysis.py took at exit (crash-analysis/<stamp>/). Use when a crash-analysis trigger arrives, or when asked why a server crashed. Read-only diagnosis; writes crash-analysis/<stamp>.md and reports in the channel.
---

# Crash analysis

`start.py` calls `crash_analysis.py` when the Java process exits nonzero without a requested stop
(ctrl-c/SIGTERM, the daily restart and host shutdown don't count). At most 3 per server per day.
It copies what the dead server left behind into `<server>/crash-analysis/<stamp>/`:

- `context.json`: server name/dir, launch and exit times, uptime, exit code and description, java command line,
  report number, snapshot file list
- `latest.log`, `debug.log.tail` (last 3000 lines), `crash-reports/*.txt` and `hs_err/hs_err_pid*.log` written during that run

Then it drops a trigger for you. The restart loop has very likely started the server again already, so the
live directory is "the server now", and the snapshot is "the server when it died".

## Procedure

1. Read `context.json`. Then read the crash report or hs_err if there is one, and the tail of the snapshot's `latest.log`.
   Prefer the snapshot to the live `logs/`, which belong to the restarted server.
2. Look at earlier reports (`crash-analysis/*.md`, newest one or two). A recurring crash is evidence in itself.
   If several snapshots are pending (a crash loop), analyse the newest and cite the others.
3. Go deeper only as far as it helps:
   - `debug.log.tail`
   - rotated logs via `zcat`/`zgrep` (never decompress in place)
   - mod jars: extract with python `zipfile` into `mktemp -d`, then `javap`, or cfr/procyon via `nix-shell`
   - configs, the pack repo (`update-and-start.sh` symlinks into it), and a web search for the exception
   Remove temp dirs afterwards.
4. Is the server back up? Check with `systemctl status minecraft@<world>`, rcon `list` and `forge tps`.
5. "It's not clear what broke" is an acceptable conclusion. Don't invent a cause. If several are plausible, rank
   them with a confidence and say what evidence would tell them apart.

## Rules

- Read-only. Don't modify, move or delete anything in the server dir, pack repo, world, configs or mods,
  apart from writing the report file below. Don't start, stop or restart servers, and use only read-only rcon commands.
  If a fix seems needed, propose it and ask with `AskUserQuestion`, as for any other change.
- Crash reports and logs carry player names and coordinates. Quote them briefly in the report, and never attach
  raw logs to channel posts.

## Output

1. Write `crash-analysis/<stamp>.md` atomically: write `<stamp>/report.md.tmp`, then rename it. Use the same layout as
   the old analyser, so `crash-analysis-notice.sh` and older tooling keep working:

   ```
   # Crash analysis: <server_name> <stamp>

   - Server: <server_dir>
   - Started <launched_at>, exited <exited_at> (uptime <h:mm:ss>)
   - Exit: <exit_description>
   - Snapshot: crash-analysis/<stamp>/
   - Report <n> of at most 3 today. Analysed by tsugumi-minecraft.

   ---

   ## Summary
   ## Evidence
   ## Possible causes
   ## Suggestions
   ## What I looked at
   ```
2. Post a channel `report`: the headline says what crashed and the likely cause, the overview holds the Summary and
   whether the server is back, and the .md is attached.
3. Add a line to `notes/open-issues.md` if a follow-up is expected.
