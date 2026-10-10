from __future__ import annotations

import io
from pathlib import Path
import stat
import tarfile
from typing import Any
import zipfile

import pytest

from agent_bridge import sources as sources_module
from agent_bridge.sources import ATTR, SourceError, Sources, SourcesConfig, archive_suffix, default_name


class Runner:
    def __init__(self, replies: list[tuple[int, str]] | None = None) -> None:
        self.calls: list[tuple[str, ...]] = []
        self.envs: list[dict[str, str] | None] = []
        self.replies = replies or []

    async def __call__(self, *argv: str, env: dict[str, str] | None = None, timeout: float = 0) -> tuple[int, str]:
        self.calls.append(argv)
        self.envs.append(env)
        if argv[:1] == ("git",):
            Path(argv[-1]).mkdir(parents=True)
            (Path(argv[-1]) / "README").write_text("hi")
        return self.replies.pop(0) if self.replies else (0, "")


def make(tmp_path: Path, runner: Any = None, **kwargs: Any) -> Sources:
    return Sources(SourcesConfig(dir=tmp_path / "src", **kwargs), runner or Runner())


def tar_bytes(members: list[tuple[tarfile.TarInfo, bytes | None]]) -> bytes:
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        for info, data in members:
            if data is not None:
                info.size = len(data)
            tar.addfile(info, io.BytesIO(data) if data is not None else None)
    return buffer.getvalue()


def serve(monkeypatch: pytest.MonkeyPatch, payload: bytes) -> None:
    def download(url: str, target: Path, max_bytes: int, timeout: float) -> None:
        if len(payload) > max_bytes:
            raise SourceError("too large")
        target.write_bytes(payload)
    monkeypatch.setattr(sources_module, "download", download)


@pytest.mark.parametrize("url", ["http://example.org/x.git", "file:///etc", "ssh://git@github.com/a/b",
                                 "https://user:pw@github.com/a/b", "ext::sh -c id", "https:///nohost"])
async def test_refuses_non_https(tmp_path: Path, url: str) -> None:
    with pytest.raises(SourceError):
        await make(tmp_path).fetch(url)


async def test_git_clone_is_defanged(tmp_path: Path) -> None:
    runner = Runner()
    text = await make(tmp_path, runner).fetch("https://github.com/openssh/openssh-portable", ref="V_9_9_P1")
    argv = runner.calls[0]
    assert "core.hooksPath=/dev/null" in argv and "protocol.allow=never" in argv
    assert "--no-recurse-submodules" in argv and "--depth" in argv
    assert argv[-3:] == ("--", "https://github.com/openssh/openssh-portable", str(tmp_path / "src/openssh-portable"))
    env = runner.envs[0] or {}
    assert env["GIT_CONFIG_GLOBAL"] == "/dev/null" and env["GIT_TERMINAL_PROMPT"] == "0"
    assert "openssh-portable" in text


@pytest.mark.parametrize("ref", ["--upload-pack=sh", "-b", "a b", "x;y"])
async def test_refuses_option_like_refs(tmp_path: Path, ref: str) -> None:
    with pytest.raises(SourceError):
        await make(tmp_path).fetch("https://github.com/a/b", ref=ref)


@pytest.mark.parametrize("name", ["../x", "/etc", "A", ".hidden", "a/b"])
async def test_refuses_bad_names(tmp_path: Path, name: str) -> None:
    with pytest.raises(SourceError):
        await make(tmp_path).fetch("https://github.com/a/b", name=name)


async def test_failed_clone_leaves_nothing(tmp_path: Path) -> None:
    sources = make(tmp_path, Runner([(128, "fatal: repository not found")]))
    with pytest.raises(SourceError, match="not found"):
        await sources.fetch("https://github.com/a/b")
    assert not (tmp_path / "src/b").exists()


async def test_tar_unpacks(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    serve(monkeypatch, tar_bytes([(tarfile.TarInfo("pkg-1.0/main.c"), b"int main;")]))
    await make(tmp_path).fetch("https://example.org/pkg-1.0.tar.gz")
    assert (tmp_path / "src/pkg-1.0/pkg-1.0/main.c").read_bytes() == b"int main;"
    assert not list((tmp_path / "src").glob(".*.download"))


def link(name: str, target: str, kind: bytes = tarfile.SYMTYPE) -> tuple[tarfile.TarInfo, None]:
    info = tarfile.TarInfo(name)
    info.type = kind
    info.linkname = target
    return info, None


@pytest.mark.parametrize("member", [
    (tarfile.TarInfo("../escape"), b"x"),
    link("evil", "/etc/passwd"),
    link("evil", "../../outside"),
    link("hard", "/etc/passwd", tarfile.LNKTYPE),
])
async def test_tar_escapes_are_refused(tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
                                       member: tuple[tarfile.TarInfo, bytes | None]) -> None:
    serve(monkeypatch, tar_bytes([member]))
    with pytest.raises(SourceError):
        await make(tmp_path).fetch("https://example.org/x.tar.gz")
    assert not (tmp_path / "src/x").exists()
    assert not (tmp_path / "escape").exists()


async def test_tar_strips_setuid_and_devices(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    suid = tarfile.TarInfo("bin")
    suid.mode = 0o4755
    serve(monkeypatch, tar_bytes([(suid, b"#!/bin/sh")]))
    await make(tmp_path).fetch("https://example.org/s.tar.gz")
    assert not (tmp_path / "src/s/bin").stat().st_mode & stat.S_ISUID
    device = tarfile.TarInfo("dev")
    device.type = tarfile.CHRTYPE
    serve(monkeypatch, tar_bytes([(device, None)]))
    with pytest.raises(SourceError):
        await make(tmp_path).fetch("https://example.org/d.tar.gz")


async def test_absolute_tar_paths_stay_inside(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    serve(monkeypatch, tar_bytes([(tarfile.TarInfo("/abs"), b"x")]))
    await make(tmp_path).fetch("https://example.org/a.tar.gz")
    assert (tmp_path / "src/a/abs").read_bytes() == b"x"


async def test_unpacked_size_is_capped(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    serve(monkeypatch, tar_bytes([(tarfile.TarInfo("big"), b"\0" * 5000)]))
    with pytest.raises(SourceError, match="more than"):
        await make(tmp_path, max_bytes=4096).fetch("https://example.org/big.tar.gz")
    assert not (tmp_path / "src/big").exists()


def zip_bytes(entries: list[tuple[zipfile.ZipInfo, bytes]]) -> bytes:
    buffer = io.BytesIO()
    with zipfile.ZipFile(buffer, "w") as zip_file:
        for info, data in entries:
            zip_file.writestr(info, data)
    return buffer.getvalue()


async def test_zip_unpacks_and_skips_links(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    symlink = zipfile.ZipInfo("ln")
    symlink.external_attr = (stat.S_IFLNK | 0o777) << 16
    serve(monkeypatch, zip_bytes([(zipfile.ZipInfo("a/b.txt"), b"ok"), (symlink, b"/etc/passwd")]))
    await make(tmp_path).fetch("https://example.org/z.zip")
    assert (tmp_path / "src/z/a/b.txt").read_bytes() == b"ok"
    assert not (tmp_path / "src/z/ln").exists()


async def test_zip_traversal_is_refused(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    serve(monkeypatch, zip_bytes([(zipfile.ZipInfo("../../evil"), b"x")]))
    with pytest.raises(SourceError, match="outside"):
        await make(tmp_path).fetch("https://example.org/z.zip")
    assert not (tmp_path / "evil").exists()


@pytest.mark.parametrize("attr,ok", [
    ("openssh", True), ("python3Packages.aiohttp", True), ("linuxPackages_latest.kernel", True),
    ("(import <nixpkgs> {})", False), ("a b", False), ("foo.${x}", False), ("", False),
    ("-version", False), ("a..b", False),
])
def test_attr_pattern(attr: str, ok: bool) -> None:
    assert (ATTR.fullmatch(attr) is not None) is ok


async def test_package_source_unpacks_archive(tmp_path: Path) -> None:
    store = tmp_path / "abc-openssh-9.9p1.tar.gz"
    store.write_bytes(tar_bytes([(tarfile.TarInfo("openssh-9.9p1/sshd.c"), b"x")]))
    runner = Runner([(0, "9.9p1"), (0, f"{store}\n")])
    text = await make(tmp_path, runner, nixpkgs="/nix/store/xyz-source").package("openssh")
    assert runner.calls[1][-1] == "path:/nix/store/xyz-source#legacyPackages.x86_64-linux.openssh.src"
    assert (tmp_path / "src/openssh-9.9p1/openssh-9.9p1/sshd.c").exists()
    assert "9.9p1" in text and "/nix/store/xyz-source/pkgs" in text


async def test_package_source_directory_is_returned(tmp_path: Path) -> None:
    store = tmp_path / "def-source"
    store.mkdir()
    runner = Runner([(0, "1.0"), (0, f"{store}\n")])
    text = await make(tmp_path, runner, nixpkgs="/nix/store/xyz-source").package("caddy")
    assert str(store) in text
    assert not (tmp_path / "src").exists() or not any(p for p in (tmp_path / "src").iterdir() if p.name != ".home")


async def test_package_source_needs_nixpkgs_and_a_plain_attr(tmp_path: Path) -> None:
    with pytest.raises(SourceError, match="no nixpkgs"):
        await make(tmp_path).package("openssh")
    with pytest.raises(SourceError, match="attribute"):
        await make(tmp_path, nixpkgs="/nix/store/x").package("openssh; rm")


async def test_drop_and_list(tmp_path: Path) -> None:
    sources = make(tmp_path)
    await sources.fetch("https://github.com/a/b")
    assert sources.listing().startswith("b ")
    with pytest.raises(SourceError):
        sources.drop("../src")
    sources.drop("b")
    assert sources.listing() == "(none)"


def test_names_and_suffixes() -> None:
    assert archive_suffix("https://x/y/openssl-3.4.tar.gz?raw=1") == ".tar.gz"
    assert archive_suffix("https://github.com/a/b") is None
    assert default_name("https://github.com/a/B.git") == "b"
    assert default_name("https://x/nginx-1.27.3.tar.gz") == "nginx-1.27.3"
