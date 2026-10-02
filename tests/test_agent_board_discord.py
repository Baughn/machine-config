import importlib.util
from pathlib import Path
import unittest

source = Path(__file__).resolve().parents[1] / "machines/tsugumi/agent-board-discord.py"
spec = importlib.util.spec_from_file_location("agent_board_discord", source)
poller = importlib.util.module_from_spec(spec)
spec.loader.exec_module(poller)

CONFIG = {
    "guild": "1", "channels": ["100"], "socket": "/nonexistent",
    "names": {"236112302590394368": "baughn", "1553406246269489152": "tsugumi-lab"},
    "agents": ["1553406246269489152"],
}
BAUGHN = {"id": "236112302590394368", "username": "baughn_"}
LAB = {"id": "1553406246269489152", "username": "LabAgent", "bot": True}
MAXWELL = {"id": "42", "username": "maxwell", "global_name": "Maxwell"}


def message(id, author, content="", **extra):
    return {"id": str(id), "type": 0, "author": author, "content": content, **extra}


def convert(msg, thread=None, fetched=None):
    def fetch_text(url):
        fetched.append(url)
        return "the plan"
    return poller.convert(msg, channel="100", thread=thread, guild="1",
                          roster=poller.Roster(CONFIG), fetch_text=fetch_text)


class Convert(unittest.TestCase):
    def test_names_and_kinds(self):
        out = convert(message(1555567596084924429, BAUGHN, "<@1553406246269489152> look",
                              mentions=[LAB]))
        self.assertEqual(out["author"], "baughn")
        self.assertEqual(out["author_kind"], "human")
        self.assertEqual(out["content"], "@tsugumi-lab look")
        self.assertEqual(out["created"], 1790946615)
        self.assertEqual(out["url"], "https://discord.com/channels/1/100/1555567596084924429")
        self.assertEqual(convert(message(2, MAXWELL))["author"], "maxwell")
        self.assertEqual(convert(message(3, LAB, "\U0001F510 **asks to run Bash**"))["author_kind"],
                         "approval")
        self.assertEqual(convert(message(4, LAB, "\U0001F4AC done"))["author_kind"], "agent")
        hook = message(5, {"id": "9", "username": "saya deploy", "bot": True}, webhook_id="9")
        self.assertEqual(convert(hook)["author_kind"], "webhook")
        self.assertEqual(convert(hook)["author"], "saya deploy")

    def test_replies_threads_and_skips(self):
        reply = message(10, BAUGHN, "yes", type=19, message_reference={"message_id": "9"})
        out = convert(reply, thread="555")
        self.assertEqual(out["reply_to"], 9)
        self.assertEqual(out["thread"], 555)
        self.assertEqual(out["url"], "https://discord.com/channels/1/555/10")
        self.assertIsNone(convert(message(11, BAUGHN, type=7)))  # a join notice
        edited = convert(message(12, BAUGHN, "x", edited_timestamp="2026-10-02T13:00:00+00:00"))
        self.assertEqual(edited["edited"], 1790946000)

    def test_attachments_and_embeds(self):
        fetched = []
        msg = message(20, LAB, "plan attached", attachments=[
            {"filename": "plan.md", "size": 100, "url": "https://cdn/plan.md"},
            {"filename": "shot.png", "size": 100, "content_type": "image/png", "url": "https://cdn/shot.png"},
            {"filename": "huge.log", "size": 5 * 1024 * 1024, "url": "https://cdn/huge.log"},
        ], embeds=[{"title": "Deploy", "fields": [{"name": "commit", "value": "6136bc6"}]}])
        out = convert(msg, fetched=fetched)
        self.assertEqual(fetched, ["https://cdn/plan.md"])
        self.assertEqual([a["text"] for a in out["attachments"]], ["the plan", None, None])
        self.assertEqual(out["content"], "plan attached\n\nDeploy\ncommit: 6136bc6")


class FakeDiscord:
    def __init__(self, pages):
        self.pages = pages
        self.paths = []

    def get(self, path):
        self.paths.append(path)
        return self.pages.get(path, [])

    @staticmethod
    def fetch_text(url):
        return ""


class FakeBoard:
    def __init__(self):
        self.stored = []

    def cursor(self, channel, thread):
        return None

    def ingest(self, messages):
        self.stored += messages


class Poll(unittest.TestCase):
    def test_backfills_in_pages_and_refreshes_edits(self):
        first = [message(i, BAUGHN, str(i)) for i in range(200, 100, -1)]
        second = [message(i, BAUGHN, str(i), edited_timestamp="2026-10-02T13:00:00+00:00")
                  for i in (201, 202)]
        discord = FakeDiscord({
            "/channels/100/messages?after=0&limit=100": first,
            "/channels/100/messages?after=200&limit=100": second,
            "/channels/100/messages?limit=50": second,
            "/guilds/1/threads/active": {"threads": [{"id": "555", "parent_id": "100"},
                                                     {"id": "556", "parent_id": "999"}]},
            "/channels/100/threads/archived/public?limit=100": {"threads": [], "has_more": False},
            "/channels/555/messages?after=0&limit=100": [message(300, LAB, "in thread")],
        })
        board = FakeBoard()
        poller.Poller(CONFIG, discord, board).cycle()
        ids = [m["id"] for m in board.stored]
        self.assertEqual(ids[:100], list(range(101, 201)))
        self.assertEqual(ids[100:], [201, 202, 201, 202, 300])
        self.assertEqual(board.stored[-1]["thread"], 555)
        self.assertNotIn("/channels/556/messages?after=0&limit=100", discord.paths)


if __name__ == "__main__":
    unittest.main()
