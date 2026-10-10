"""Source code for an agent without a shell: clone a git repository, unpack an archive, or
realise a nixpkgs package's source, into one directory (the security watch's fetch tools).

Nothing fetched is ever run: git runs with hooks, submodules, LFS filters and non-https
transports off, and archives unpack through tarfile's "data" filter (no links out of the
tree, no devices, no setuid bits) or a checked zipfile. Sizes are capped here; the unit
puts the directory on a size-limited tmpfs as the backstop.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import tarfile
from typing import Any, Callable
import urllib.parse
import urllib.request
import zipfile

ARCHIVES = (".tar", ".tar.gz", ".tgz", ".tar.xz", ".txz", ".tar.bz2", ".tbz2", ".tar.zst", ".zip")
# An attribute path in nixpkgs, e.g. openssh or python3Packages.aiohttp. Never an expression.
ATTR = re.compile(r"[A-Za-z_][A-Za-z0-9_'-]*(\.[A-Za-z_][A-Za-z0-9_'-]*){0,4}")
NAME = re.compile(r"[a-z0-9][a-z0-9._-]{0,80}")
REF = re.compile(r"[A-Za-z0-9][A-Za-z0-9._/+-]{0,120}")
GIT_CONFIG = [
    "core.hooksPath=/dev/null", "core.fsmonitor=false", "protocol.allow=never", "protocol.https.allow=always",
    "submodule.recurse=false", "filter.lfs.smudge=", "filter.lfs.process=", "filter.lfs.required=false",
    "advice.detachedHead=false",
]


class SourceError(Exception):
    """A fetch that was refused or failed; the message is for the agent."""


@dataclass(frozen=True)
class SourcesConfig:
    dir: Path
    max_bytes: int = 2 * 1024**3
    timeout: float = 600.0
    nixpkgs: str | None = None  # the deployed nixpkgs, a /nix/store path holding flake.nix
    system: str = "x86_64-linux"


Runner = Callable[..., Any]  # async (*argv, env=..., timeout=...) -> (status, output)


async def run(*argv: str, env: dict[str, str] | None = None, timeout: float = 600.0) -> tuple[int, str]:
    process = await asyncio.create_subprocess_exec(*argv, stdout=asyncio.subprocess.PIPE,
                                                   stderr=asyncio.subprocess.STDOUT, env=env,
                                                   stdin=asyncio.subprocess.DEVNULL)
    try:
        output, _ = await asyncio.wait_for(process.communicate(), timeout)
    except TimeoutError:
        process.kill()
        await process.wait()
        return 124, f"timed out after {timeout:.0f} s"
    return process.returncode or 0, output.decode("utf-8", "replace")


def tree_size(path: Path) -> int:
    total = 0
    for root, _, files in os.walk(path):
        for name in files:
            with_path = Path(root, name)
            try:
                total += with_path.lstat().st_size
            except OSError:
                pass
    return total


def archive_suffix(url: str) -> str | None:
    path = urllib.parse.urlsplit(url).path.lower()
    return next((s for s in sorted(ARCHIVES, key=len, reverse=True) if path.endswith(s)), None)


def check_url(url: str) -> urllib.parse.SplitResult:
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != "https" or not parts.hostname or parts.username or parts.password:
        raise SourceError("only https:// URLs without credentials are allowed")
    return parts


def default_name(url: str) -> str:
    stem = PurePosixPath(urllib.parse.urlsplit(url).path).name or "source"
    for suffix in sorted(ARCHIVES, key=len, reverse=True) + [".git"]:
        if stem.lower().endswith(suffix):
            stem = stem[: -len(suffix)]
            break
    slug = re.sub(r"[^a-z0-9._-]+", "-", stem.lower()).strip("-.") or "source"
    return slug[:60]


def unpack_tar(archive: Path, dest: Path, max_bytes: int) -> None:
    with tarfile.open(archive, "r:*") as tar:
        members = tar.getmembers()
        if sum(m.size for m in members if m.isfile()) > max_bytes:
            raise SourceError(f"the archive unpacks to more than {max_bytes} bytes")
        tar.extractall(dest, members=members, filter="data")


def unpack_zip(archive: Path, dest: Path, max_bytes: int) -> None:
    with zipfile.ZipFile(archive) as zip_file:
        infos = zip_file.infolist()
        if sum(i.file_size for i in infos) > max_bytes:
            raise SourceError(f"the archive unpacks to more than {max_bytes} bytes")
        root = dest.resolve()
        for info in infos:
            mode = info.external_attr >> 16
            if stat.S_ISLNK(mode):
                continue  # links could point anywhere; the files they name are in the archive anyway
            target = (dest / info.filename).resolve()
            if target != root and root not in target.parents:
                raise SourceError(f"refusing a path outside the tree: {info.filename!r}")
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            with zip_file.open(info) as source, target.open("wb") as sink:
                shutil.copyfileobj(source, sink)
            target.chmod(0o644)


def unpack(archive: Path, suffix: str, dest: Path, max_bytes: int) -> None:
    dest.mkdir(parents=True)
    if suffix == ".zip":
        unpack_zip(archive, dest, max_bytes)
    else:
        unpack_tar(archive, dest, max_bytes)


def download(url: str, target: Path, max_bytes: int, timeout: float) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "agent-bridge-sources"})
    with urllib.request.urlopen(request, timeout=min(timeout, 60)) as response, target.open("wb") as sink:
        check_url(response.geturl())  # no redirects to http or file
        total = 0
        while chunk := response.read(1 << 16):
            total += len(chunk)
            if total > max_bytes:
                raise SourceError(f"the download is larger than {max_bytes} bytes")
            sink.write(chunk)


class Sources:
    def __init__(self, config: SourcesConfig, runner: Runner = run) -> None:
        self.config = config
        self.runner = runner

    def destination(self, name: str) -> Path:
        if NAME.fullmatch(name) is None:
            raise SourceError("name must be lowercase letters, digits, '.', '_' or '-'")
        self.config.dir.mkdir(parents=True, exist_ok=True)
        dest = self.config.dir / name
        if dest.exists() or dest.is_symlink():
            raise SourceError(f"{dest} exists already; drop_source it or pick another name")
        return dest

    def env(self) -> dict[str, str]:
        home = self.config.dir / ".home"
        home.mkdir(parents=True, exist_ok=True)
        keep = {k: v for k, v in os.environ.items() if k in ("PATH", "SSL_CERT_FILE", "NIX_SSL_CERT_FILE", "LANG", "TZ")}
        return {**keep, "HOME": str(home), "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
                "GIT_TERMINAL_PROMPT": "0", "GIT_LFS_SKIP_SMUDGE": "1", "GIT_ASKPASS": "/bin/false"}

    def finished(self, dest: Path, what: str) -> str:
        size = tree_size(dest)
        if size > self.config.max_bytes:
            shutil.rmtree(dest, ignore_errors=True)
            raise SourceError(f"{what} is larger than {self.config.max_bytes} bytes; dropped it")
        return f"{what}: {dest} ({size // 1024} KiB). Read it with Read, Grep and Glob."

    async def fetch(self, url: str, ref: str | None = None, name: str | None = None) -> str:
        check_url(url)
        if ref is not None and REF.fullmatch(ref) is None:
            raise SourceError("ref must be a branch or tag name")
        dest = self.destination(name or default_name(url))
        suffix = archive_suffix(url)
        if suffix is not None:
            if ref is not None:
                raise SourceError("ref applies to git repositories, not archives")
            archive = self.config.dir / f".{dest.name}.download"
            try:
                await asyncio.to_thread(download, url, archive, self.config.max_bytes, self.config.timeout)
                await asyncio.to_thread(unpack, archive, suffix, dest, self.config.max_bytes)
            except (OSError, tarfile.TarError, zipfile.BadZipFile, ValueError) as error:
                shutil.rmtree(dest, ignore_errors=True)
                raise SourceError(f"fetching {url} failed: {error}") from error
            except SourceError:
                shutil.rmtree(dest, ignore_errors=True)
                raise
            finally:
                archive.unlink(missing_ok=True)
            return self.finished(dest, f"unpacked {url}")
        argv = ["git"]
        for setting in GIT_CONFIG:
            argv += ["-c", setting]
        argv += ["clone", "--depth", "1", "--single-branch", "--no-tags", "--no-recurse-submodules"]
        if ref is not None:
            argv += ["--branch", ref]
        argv += ["--", url, str(dest)]
        status, output = await self.runner(*argv, env=self.env(), timeout=self.config.timeout)
        if status != 0:
            shutil.rmtree(dest, ignore_errors=True)
            raise SourceError(f"git clone failed ({status}): {output.strip()[-1500:]}")
        return self.finished(dest, f"cloned {url}" + (f" at {ref}" if ref else ""))

    async def package(self, attr: str) -> str:
        """The deployed nixpkgs' source for one package: realised, then unpacked if it's an archive."""
        nixpkgs = self.config.nixpkgs
        if nixpkgs is None:
            raise SourceError("no nixpkgs is configured for this identity")
        if ATTR.fullmatch(attr) is None:
            raise SourceError("attr must be a nixpkgs attribute path, e.g. openssh or python3Packages.aiohttp")
        installable = f"path:{nixpkgs}#legacyPackages.{self.config.system}.{attr}"
        nix = ["nix", "--extra-experimental-features", "nix-command flakes"]
        status, version = await self.runner(*nix, "eval", "--raw", f"{installable}.version",
                                            env=self.env(), timeout=self.config.timeout)
        version = version.strip() if status == 0 else "unknown"
        status, output = await self.runner(*nix, "build", "--no-link", "--print-out-paths", f"{installable}.src",
                                           env=self.env(), timeout=self.config.timeout)
        if status != 0:
            raise SourceError(f"building {attr}.src failed ({status}): {output.strip()[-1500:]}")
        store = Path(output.strip().splitlines()[-1])
        where = f"{attr} {version}: nixpkgs expression under {nixpkgs}/pkgs (grep for the attribute)"
        if store.is_dir():
            return f"{where}; source tree {store} (read-only store path)."
        suffix = archive_suffix(store.name) or (".zip" if zipfile.is_zipfile(store) else ".tar")
        dest = self.destination(f"{default_name(attr)}-{default_name(version)}")
        try:
            await asyncio.to_thread(unpack, store, suffix, dest, self.config.max_bytes)
        except (OSError, tarfile.TarError, zipfile.BadZipFile, ValueError) as error:
            shutil.rmtree(dest, ignore_errors=True)
            raise SourceError(f"unpacking {store} failed: {error}") from error
        except SourceError:
            shutil.rmtree(dest, ignore_errors=True)
            raise
        return f"{where}; " + self.finished(dest, f"unpacked {store}")

    def drop(self, name: str) -> str:
        if NAME.fullmatch(name) is None:
            raise SourceError("name must be one of the directories fetch_source made")
        dest = self.config.dir / name
        if not dest.is_dir() or dest.is_symlink():
            raise SourceError(f"no source directory {name}")
        shutil.rmtree(dest)
        return f"dropped {dest}"

    def listing(self) -> str:
        if not self.config.dir.is_dir():
            return "(none)"
        dirs = sorted(p for p in self.config.dir.iterdir() if p.is_dir() and not p.name.startswith("."))
        return "\n".join(f"{p.name} ({tree_size(p) // 1024} KiB)" for p in dirs) or "(none)"
