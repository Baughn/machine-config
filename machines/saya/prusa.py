"""prusa: the printer agent's handle on the Prusa (agent-printer.nix), over PrusaLink's local v1
API and the Buddy3D camera's RTSP stream.

Usage:
  prusa status                 printer, job, transfer and storage telemetry (JSON)
  prusa job                    the current job, or "no job"
  prusa storage                the storages and their paths
  prusa files [PATH]           a folder's listing or a file's metadata, e.g. usb/ or usb/x.bgcode
  prusa snapshot OUT.jpg       one camera frame (RTSP, via ffmpeg)
  prusa pause | resume | stop  the current job
  prusa upload FILE [DEST] [--print]
                               put FILE on the printer (DEST default usb/<name>); --print starts it
  prusa start PATH             print a file already on the printer, e.g. usb/x.bgcode
  prusa delete PATH            delete a file on the printer

Credentials: PRUSA_ENV_FILE (KEY=value lines) with PRUSA_HOST, PRUSA_PASSWORD (the PrusaLink
password or API key), PRUSA_USER (digest user, default maker) and PRUSA_CAMERA (rtsp://...).
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any
import urllib.error
import urllib.parse
import urllib.request


def settings(env: dict[str, str]) -> dict[str, str]:
    found = dict(env)
    path = env.get("PRUSA_ENV_FILE")
    if path:
        for line in Path(path).read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, value = line.split("=", 1)
                found.setdefault(key.strip(), value.strip().strip('"'))
    return found


def remote(path: str) -> str:
    """A printer path like usb/a b.bgcode, quoted for the URL; no way out of the storage."""
    parts = [p for p in path.strip("/").split("/") if p]
    if not parts or any(p in (".", "..") for p in parts):
        sys.exit(f"bad printer path: {path!r} (want e.g. usb/file.bgcode)")
    return "/".join(urllib.parse.quote(p) for p in parts)


class Printer:
    def __init__(self, conf: dict[str, str]) -> None:
        host = conf.get("PRUSA_HOST")
        if not host:
            sys.exit("PRUSA_HOST is not set (secrets/prusa-printer.env.age)")
        self.base = host if "://" in host else f"http://{host}"
        password = conf.get("PRUSA_PASSWORD", "")
        manager = urllib.request.HTTPPasswordMgrWithDefaultRealm()
        manager.add_password(None, self.base, conf.get("PRUSA_USER", "maker"), password)
        self.opener = urllib.request.build_opener(urllib.request.HTTPDigestAuthHandler(manager))
        self.key = password

    def call(self, method: str, path: str, body: bytes | None = None,
             headers: dict[str, str] | None = None) -> tuple[int, bytes]:
        request = urllib.request.Request(self.base + path, data=body, method=method, headers={
            "Accept": "application/json", "X-Api-Key": self.key, **(headers or {})})
        try:
            with self.opener.open(request, timeout=600 if body else 30) as response:
                return response.status, response.read()
        except urllib.error.HTTPError as error:
            sys.exit(f"{method} {path}: HTTP {error.code} {error.reason}: {error.read()[:500].decode(errors='replace')}")
        except urllib.error.URLError as error:
            sys.exit(f"{method} {path}: {error.reason}")

    def json(self, path: str) -> Any:
        status, body = self.call("GET", path)
        return None if status == 204 or not body else json.loads(body)

    def job_id(self) -> int:
        job = self.json("/api/v1/job")
        if not job:
            sys.exit("no job")
        return int(job["id"])


def main(argv: list[str]) -> None:
    if not argv or argv[0] in ("-h", "--help"):
        sys.exit(__doc__)
    conf = settings(dict(os.environ))
    command, args = argv[0], argv[1:]
    if command == "snapshot":
        if len(args) != 1 or not conf.get("PRUSA_CAMERA"):
            sys.exit("usage: prusa snapshot OUT.jpg (needs PRUSA_CAMERA)")
        subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-rtsp_transport", "tcp",
                        "-i", conf["PRUSA_CAMERA"], "-frames:v", "1", "-q:v", "2", args[0]],
                       check=True, timeout=60)
        print(args[0])
        return
    printer = Printer(conf)
    if command in ("status", "job", "storage") and not args:
        found = printer.json(f"/api/v1/{command}")
        print(json.dumps(found, indent=1) if found is not None else f"no {command}")
    elif command == "files" and len(args) <= 1:
        found = printer.json("/api/v1/files/" + remote(args[0] if args else "usb"))
        print(json.dumps(found, indent=1))
    elif command in ("pause", "resume") and not args:
        printer.call("PUT", f"/api/v1/job/{printer.job_id()}/{command}")
        print(f"{command}d")
    elif command == "stop" and not args:
        printer.call("DELETE", f"/api/v1/job/{printer.job_id()}")
        print("stopped")
    elif command == "start" and len(args) == 1:
        printer.call("POST", "/api/v1/files/" + remote(args[0]))
        print(f"printing {args[0]}")
    elif command == "delete" and len(args) == 1:
        printer.call("DELETE", "/api/v1/files/" + remote(args[0]))
        print(f"deleted {args[0]}")
    elif command == "upload" and 1 <= len([a for a in args if a != "--print"]) <= 2:
        start = "--print" in args
        rest = [a for a in args if a != "--print"]
        source = Path(rest[0])
        dest = remote(rest[1] if len(rest) == 2 else f"usb/{source.name}")
        data = source.read_bytes()
        printer.call("PUT", "/api/v1/files/" + dest, body=data, headers={
            "Content-Type": "application/octet-stream", "Content-Length": str(len(data)),
            "Print-After-Upload": "?1" if start else "?0", "Overwrite": "?0"})
        print(f"uploaded {dest}" + (", printing" if start else ""))
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
