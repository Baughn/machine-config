import importlib.util
import io
import json
from pathlib import Path
import unittest
import urllib.error

source = Path(__file__).resolve().parents[1] / "modules/discord-notify.py"
spec = importlib.util.spec_from_file_location("discord_notify", source)
notify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notify)
notify.OWNER = "236112302590394368"


class Response:
    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def read(self):
        return b""


def rate_limited(retry_after):
    body = io.BytesIO(json.dumps({"retry_after": retry_after}).encode())
    return urllib.error.HTTPError("https://example/hook", 429, "Too Many Requests", {}, body)


class Payload(unittest.TestCase):
    def test_no_mentions_by_default(self):
        body = notify.payload("  storm at <@1> @everyone \n", "weather", False)
        self.assertEqual(body["content"], "storm at <@1> @everyone")
        self.assertEqual(body["username"], "weather")
        self.assertEqual(body["allowed_mentions"], {"parse": [], "users": []})

    def test_mention_owner(self):
        body = notify.payload("storm", "weather", True)
        self.assertEqual(body["allowed_mentions"]["users"], [notify.OWNER])

    def test_truncates(self):
        body = notify.payload("x" * 5000, "n", False)
        self.assertEqual(len(body["content"]), notify.LIMIT)
        self.assertTrue(body["content"].endswith("…"))

    def test_empty(self):
        with self.assertRaises(ValueError):
            notify.payload(" \n", "n", False)


class Send(unittest.TestCase):
    def test_posts_json(self):
        seen = []

        def opener(request, timeout):
            seen.append(request)
            return Response()

        notify.send({"content": "hi"}, "https://example/hook", opener=opener)
        self.assertEqual(len(seen), 1)
        self.assertEqual(seen[0].get_method(), "POST")
        self.assertEqual(json.loads(seen[0].data), {"content": "hi"})

    def test_retries_rate_limits(self):
        calls, sleeps = [], []

        def opener(request, timeout):
            calls.append(request)
            if len(calls) < 3:
                raise rate_limited(0.5)
            return Response()

        notify.send({"content": "hi"}, "https://example/hook", opener=opener, sleep=sleeps.append)
        self.assertEqual(len(calls), 3)
        self.assertEqual(sleeps, [0.5, 0.5])

    def test_gives_up(self):
        def opener(request, timeout):
            raise rate_limited(0.1)

        with self.assertRaises(urllib.error.HTTPError):
            notify.send({"content": "hi"}, "https://example/hook", opener=opener, sleep=lambda _: None)

    def test_other_errors_are_not_retried(self):
        calls = []

        def opener(request, timeout):
            calls.append(request)
            raise urllib.error.HTTPError("u", 404, "Not Found", {}, io.BytesIO(b"{}"))

        with self.assertRaises(urllib.error.HTTPError):
            notify.send({"content": "hi"}, "https://example/hook", opener=opener, sleep=lambda _: None)
        self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
