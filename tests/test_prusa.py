import contextlib
import http.server
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import threading
import unittest
import unittest.mock

source = Path(__file__).resolve().parents[1] / "machines/saya/prusa.py"
spec = importlib.util.spec_from_file_location("prusa", source)
prusa = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prusa)


class FakeLink(http.server.BaseHTTPRequestHandler):
    """Just enough PrusaLink: an X-Api-Key check, a job, and file uploads."""
    calls = []

    def log_message(self, *args):
        pass

    def reply(self, code, body=None):
        self.send_response(code)
        data = json.dumps(body).encode() if body is not None else b""
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def handle_any(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        FakeLink.calls.append((self.command, self.path, dict(self.headers), body))
        if self.headers.get("X-Api-Key") != "secret":
            return self.reply(401)
        if self.command == "GET" and self.path == "/api/v1/job":
            return self.reply(200, {"id": 7, "state": "PRINTING", "progress": 12, "time_printing": 60})
        if self.command == "GET" and self.path == "/api/v1/status":
            return self.reply(200, {"printer": {"state": "PRINTING"}})
        return self.reply(204)

    do_GET = do_PUT = do_POST = do_DELETE = handle_any


class Commands(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FakeLink)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.addClassCleanup(cls.server.shutdown)

    def run_prusa(self, *argv):
        with tempfile.NamedTemporaryFile("w", suffix=".env", delete=False) as env:
            env.write(f"PRUSA_HOST=127.0.0.1:{self.server.server_port}\nPRUSA_PASSWORD=secret\n")
        FakeLink.calls.clear()
        out = io.StringIO()
        with unittest.mock.patch.dict("os.environ", {"PRUSA_ENV_FILE": env.name}, clear=True), \
                contextlib.redirect_stdout(out):
            prusa.main(list(argv))
        return out.getvalue()

    def test_status(self):
        self.assertEqual(json.loads(self.run_prusa("status"))["printer"]["state"], "PRINTING")

    def test_pause_uses_the_job_id(self):
        self.assertEqual(self.run_prusa("pause").strip(), "paused")
        self.assertEqual([c[:2] for c in FakeLink.calls], [("GET", "/api/v1/job"), ("PUT", "/api/v1/job/7/pause")])

    def test_upload_and_print(self):
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "a b.bgcode").write_bytes(b"GCODE")
            self.run_prusa("upload", f"{tmp}/a b.bgcode", "--print")
        method, path, headers, body = FakeLink.calls[0]
        self.assertEqual((method, path, body), ("PUT", "/api/v1/files/usb/a%20b.bgcode", b"GCODE"))
        self.assertEqual((headers["Print-After-Upload"], headers["Overwrite"]), ("?1", "?0"))

    def test_bad_paths(self):
        for path in ("../etc", "usb/../x", ""):
            with self.assertRaises(SystemExit):
                prusa.remote(path)


if __name__ == "__main__":
    unittest.main()
