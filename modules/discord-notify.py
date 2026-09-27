"""discord-notify: post a message to the agent channel through the watchdog webhook.

For system services (weather warnings and the like) that need to say something
without an agent. Webhook messages are context for the agents, never triggers.
See modules/discord-notify.nix.

    discord-notify [--name NAME] [--mention-owner] [MESSAGE...]

Without MESSAGE, the message is read from stdin. The webhook URL is read from a
file (never argv, which any user can see in ps).
"""

import argparse
import json
from pathlib import Path
import sys
import time
import urllib.error
import urllib.request

WEBHOOK = Path('@webhook@')
OWNER = '@owner@'
USER_AGENT = "DiscordBot (https://github.com/baughn/machine-config, 1)"
LIMIT = 2000
ATTEMPTS = 4


def payload(message, name, mention_owner):
    message = message.strip()
    if not message:
        raise ValueError("empty message")
    if len(message) > LIMIT:
        message = message[:LIMIT - 1] + "…"
    # Mentions notify nobody unless asked for, whatever the text contains.
    return {"content": message, "username": name,
            "allowed_mentions": {"parse": [], "users": [OWNER] if mention_owner else []}}


def send(body, url, opener=urllib.request.urlopen, sleep=time.sleep):
    data = json.dumps(body).encode()
    for attempt in range(ATTEMPTS):
        request = urllib.request.Request(url, data=data, method="POST", headers={
            "Content-Type": "application/json", "User-Agent": USER_AGENT})
        try:
            with opener(request, timeout=30) as response:
                response.read()
                return
        except urllib.error.HTTPError as error:
            if error.code != 429 or attempt == ATTEMPTS - 1:
                raise
            try:
                delay = float(json.loads(error.read()).get("retry_after", 1))
            except ValueError:
                delay = 1.0
            sleep(min(delay, 60.0))


def main(argv=None):
    parser = argparse.ArgumentParser(prog="discord-notify")
    parser.add_argument("--name", default="notify", help="the name the message is posted under")
    parser.add_argument("--mention-owner", action="store_true", help="ping Baughn")
    parser.add_argument("message", nargs="*", help="the message (default: stdin)")
    args = parser.parse_args(argv)
    text = " ".join(args.message) if args.message else sys.stdin.read()
    try:
        body = payload(text, args.name[:80], args.mention_owner)
    except ValueError as error:
        parser.error(str(error))
    try:
        url = WEBHOOK.read_text().strip()
    except OSError as error:
        sys.exit(f"discord-notify: can't read the webhook ({error.strerror}); "
                 "is this unit in the discord-notify group?")
    try:
        send(body, url)
    except urllib.error.URLError as error:
        sys.exit(f"discord-notify: post failed: {error}")


if __name__ == "__main__":
    main()
