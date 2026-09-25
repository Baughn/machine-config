#!/usr/bin/env python3
"""Local Erisia evidence collection and bounded diagnostic RCON commands (stdlib only)."""
import argparse
import datetime as dt
import hashlib
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import struct
import sys
import time
import urllib.request


def properties(path):
    # start.py writes simple key=value properties with a URL-safe random password.
    return dict(line.strip().split('=', 1) for line in path.read_text().splitlines()
                if '=' in line and not line.lstrip().startswith(('#', '!')))


def packet(sock, ident, kind, body):
    body = body.encode('utf-8')
    sock.sendall(struct.pack('<iii', len(body) + 10, ident, kind) + body + b'\0\0')


def receive(sock):
    def exact(n):
        data = bytearray()
        while len(data) < n:
            chunk = sock.recv(n - len(data))
            if not chunk:
                raise RuntimeError('RCON closed before completing response')
            data.extend(chunk)
        return bytes(data)
    size, = struct.unpack('<i', exact(4))
    if not 10 <= size <= 4 * 1024 * 1024:
        raise RuntimeError('Invalid RCON packet size')
    data = exact(size)
    if data[-2:] != b'\0\0':
        raise RuntimeError('Invalid RCON terminator')
    ident, kind = struct.unpack('<ii', data[:8])
    return ident, kind, data[8:-2]


def rcon(server, command):
    cfg = properties(server / 'server.properties')
    if cfg.get('enable-rcon') != 'true' or not cfg.get('rcon.password'):
        raise RuntimeError('RCON is not configured')
    with socket.create_connection(('127.0.0.1', int(cfg.get('rcon.port', 25575))), 5) as sock:
        sock.settimeout(10)
        packet(sock, 1, 3, cfg['rcon.password'])
        for _ in range(3):
            ident, kind, _ = receive(sock)
            if ident == -1:
                raise RuntimeError('RCON authentication failed')
            if ident == 1 and kind == 2:
                break
        else:
            raise RuntimeError('Missing RCON authentication response')
        packet(sock, 2, 2, command)
        # A separate read-only command delimits Minecraft's split response packets.
        packet(sock, 3, 2, 'list')
        parts = []
        for _ in range(1024):
            ident, kind, body = receive(sock)
            if ident == 3:
                return re.sub('\u00a7.', '', b''.join(parts).decode('utf-8', 'replace'))
            if ident != 2 or kind != 0:
                raise RuntimeError('Unexpected RCON response')
            parts.append(body)
        raise RuntimeError('RCON response exceeded limit')


READ_COMMANDS = ('list', 'forge tps', 'flare tps', 'flare health', 'flare sampler info', 'help flare')


def check_command(command):
    if command.startswith('erisia-inspect '):
        check_inspection(command.removeprefix('erisia-inspect '))
        return
    if command in READ_COMMANDS or command == 'flare sampler stop --save-to-file':
        return
    parts = command.split()
    if parts[:3] != ['flare', 'sampler', 'start']:
        raise ValueError('Only listed diagnostic commands and bounded local Flare profiles are supported')
    flags = parts[3:]
    seen = set()
    timeout = None
    while flags:
        flag = flags.pop(0)
        if flag in seen:
            raise ValueError('Duplicate profiler option')
        seen.add(flag)
        if flag in ('--save-to-file', '--force-java-sampler'):
            continue
        if flag not in ('--timeout', '--only-ticks-over', '--thread') or not flags:
            raise ValueError('Unsupported profiler option')
        value = flags.pop(0)
        if flag == '--thread':
            if value != '*':
                raise ValueError('Use default server thread or --thread *')
        elif not value.isdigit() or not 1 <= int(value) <= 300:
            raise ValueError('Duration/threshold must be between 1 and 300')
        elif flag == '--timeout':
            timeout = int(value)
    if timeout is None or timeout < 11 or '--save-to-file' not in seen:
        raise ValueError('Profiles require --timeout 11..300 and --save-to-file')


def check_inspection(command):
    parts = command.split()
    if parts in (['status'], ['census'], ['watch', 'status']):
        return
    if parts[:1] == ['census'] and len(parts) == 2:
        int(parts[1])
        return
    if parts[:1] == ['chunk'] and len(parts) in (4, 5):
        dim, x, z = map(int, parts[1:4])
        if abs(x) <= 1875000 and abs(z) <= 1875000 and (len(parts) == 4 or 0 <= int(parts[4]) <= 100000):
            return
    if parts[:2] == ['watch', 'start'] and len(parts) in (3, 6) and 1 <= int(parts[2]) <= 60:
        if len(parts) == 3:
            return
        dim, x, z = map(int, parts[3:])
        if abs(x) <= 1875000 and abs(z) <= 1875000:
            return
    raise ValueError('Supported inspections: status, census [DIM], chunk DIM CX CZ [OFFSET], watch start 1..60 [DIM CX CZ], watch status')


def inspection(server, command, out):
    check_inspection(command)
    # Verify the evidence destination before starting an observational watch.
    with out.open('x') as output:
        response = rcon(server, 'erisia-inspect ' + command)
        output.write(response + '\n')
    try:
        report = json.loads(response)
    except json.JSONDecodeError as exc:
        raise RuntimeError('Inspector did not return JSON; verify the server-only mod is deployed') from exc
    if report.get('error') or report.get('schema') != 1:
        raise RuntimeError('Inspector error or incompatible protocol; inspect the saved response')
    print(out)
    return 0


def utc():
    return dt.datetime.now(dt.timezone.utc).isoformat()


def java_processes(server):
    result = []
    for proc in Path('/proc').glob('[0-9]*'):
        try:
            exe = (proc / 'exe').resolve(strict=True)
            if exe.name != 'java' or (proc / 'cwd').resolve(strict=True) != server:
                continue
            result.append({'pid': int(proc.name), 'exe': str(exe),
                           'start_stat': (proc / 'stat').read_text()})
        except (OSError, RuntimeError):
            continue
    return result


def snapshot(server, out):
    out.mkdir(parents=True, exist_ok=False, mode=0o700)
    errors = []

    def save(name, fn):
        try:
            data = fn()
            (out / name).write_text(data if isinstance(data, str) else json.dumps(data, indent=2) + '\n')
        except Exception as exc:
            errors.append({'artifact': name, 'error': str(exc)})

    meta = {'started_utc': utc(), 'server': str(server), 'processes': java_processes(server)}
    for name in ('server', 'pack'):
        meta[name + '_store_path'] = str((server / name).resolve())
    save('target.txt', lambda: (server / 'server.nix-target').read_text())
    save('jvm-args.txt', lambda: (server / 'user_jvm_args.txt').read_text())
    safe_keys = ('view-distance', 'simulation-distance', 'max-players', 'level-type',
                 'max-tick-time', 'network-compression-threshold', 'allow-nether')
    save('server-settings.json', lambda: {k: v for k, v in properties(server / 'server.properties').items() if k in safe_keys})

    def inventory():
        result = []
        # Only jar files, including version subdirectories; do not traverse world assets.
        paths = list((server / 'mods').glob('*.jar')) + list((server / 'mods').glob('*/*.jar'))
        for path in sorted(paths):
            with path.open('rb') as f:
                digest = hashlib.file_digest(f, 'sha256').hexdigest()
            result.append({'path': str(path.relative_to(server)), 'bytes': path.stat().st_size, 'sha256': digest})
        return result
    save('mods.json', inventory)

    def metrics():
        cfg = (server / 'config/prometheus-integration.cfg').read_text()
        port = int(re.search(r'I:jetty\s*=\s*(\d+)', cfg).group(1))
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(f'http://127.0.0.1:{port}/metrics', timeout=5) as response:
            body = response.read(8 * 1024 * 1024 + 1)
        if len(body) > 8 * 1024 * 1024:
            raise ValueError('Metrics exceed 8 MiB limit')
        return body.decode('utf-8')
    meta['metrics_requested_utc'] = utc()
    save('metrics.prom', metrics)
    meta['metrics_finished_utc'] = utc()
    for cmd in READ_COMMANDS[:5]:
        save(cmd.replace(' ', '-') + '.txt', lambda cmd=cmd: rcon(server, cmd))
    for name in ('stat', 'meminfo', 'loadavg', 'diskstats', 'pressure/cpu', 'pressure/io', 'pressure/memory'):
        save('host-' + name.replace('/', '-') + '.txt', lambda name=name: Path('/proc', name).read_text())
    for proc in meta['processes']:
        for name in ('stat', 'status', 'io', 'cgroup'):
            save(f'java-{proc["pid"]}-{name}.txt', lambda proc=proc, name=name: Path('/proc', str(proc['pid']), name).read_text())
    for name in ('latest.log', 'debug.log'):
        def tail(name=name):
            with (server / 'logs' / name).open('rb') as f:
                f.seek(0, 2)
                start = max(0, f.tell() - 512 * 1024)
                f.seek(start)
                if start:
                    f.readline()
                return f.read().decode('utf-8', 'replace')
        save(name, tail)
    if not meta['processes']:
        errors.append({'artifact': 'processes', 'error': 'No Java process visible for this server directory; check host PID namespace/access'})
    meta['ended_utc'] = utc()
    meta['errors'] = errors
    (out / 'collection.json').write_text(json.dumps(meta, indent=2) + '\n')
    print(json.dumps({'bundle': str(out), 'errors': errors}, indent=2))
    return 2 if errors else 0


def profile(server, out, seconds, ticks_over=None):
    """Explicit stop/export: Flare 0.8.0's timeout does not clear/export its container."""
    out.mkdir(parents=True, exist_ok=False, mode=0o700)
    key = hashlib.sha256(str(server).encode()).hexdigest()[:16]
    lock_path = Path('/tmp') / f'tick-debug-{os.getuid()}-{key}.lock'
    fd = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    events = []

    def command(text):
        event = {'utc': utc(), 'command': text}
        events.append(event)
        try:
            event['response'] = rcon(server, text)
            return event['response']
        except Exception as exc:
            event['error'] = str(exc)
            raise
        finally:
            (out / 'commands.json').write_text(json.dumps(events, indent=2) + '\n')

    with os.fdopen(fd, 'w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if 'No active sampler found' not in command('flare sampler info'):
            raise RuntimeError('Sampler already active or state unrecognized; leaving it alone')
        directory = server / 'config/flare/profiler'
        previous = {p: (p.stat().st_mtime_ns, p.stat().st_size) for p in directory.glob('*.sparkprofile')}
        start = f'flare sampler start --timeout {seconds + 15} --save-to-file --force-java-sampler'
        if ticks_over is not None:
            start += f' --only-ticks-over {ticks_over}'
        response = command(start)
        if 'Sampler started!' not in response:
            raise RuntimeError('Sampler start was not acknowledged; inspect commands.json and live state')
        try:
            time.sleep(seconds)
        finally:
            command('flare sampler stop --save-to-file')
        if 'No active sampler found' not in command('flare sampler info'):
            raise RuntimeError('Sampler cleanup not confirmed; inspect live state')
        # Export is asynchronous. Require stable nonempty bytes and retain its own timestamp.
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            candidates = [p for p in directory.glob('*.sparkprofile')
                          if (p.stat().st_mtime_ns, p.stat().st_size) != previous.get(p)]
            if len(candidates) == 1:
                path = candidates[0]
                before = path.stat()
                time.sleep(0.5)
                after = path.stat()
                if before.st_size > 0 and (before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns):
                    shutil.copyfile(path, out / 'capture.sparkprofile')
                    (out / 'capture.json').write_text(json.dumps({'source': str(path), 'copied_utc': utc(),
                        'requested_seconds': seconds, 'backend': 'java', 'only_ticks_over': ticks_over}, indent=2) + '\n')
                    print(out / 'capture.sparkprofile')
                    return 0
            time.sleep(0.5)
        raise RuntimeError('No unique stable profile file exported; inspect commands.json and server logs')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server', type=Path, default=Path.home() / 'erisia')
    subs = parser.add_subparsers(dest='action', required=True)
    snap = subs.add_parser('snapshot', help='Collect local metrics, inventory, logs and console diagnostics')
    snap.add_argument('--out', type=Path, required=True, help='New private directory; never overwritten')
    command = subs.add_parser('command', help='Send one diagnostic command; no automatic retries')
    command.add_argument('text', help='Quote the entire console command, without a leading slash')
    capture = subs.add_parser('profile', help='Time, stop, and save a local Java-sampler capture')
    capture.add_argument('--out', type=Path, required=True)
    capture.add_argument('--seconds', type=int, choices=range(11, 121), default=30, metavar='11..120')
    capture.add_argument('--only-ticks-over', type=int, choices=range(1, 301), metavar='1..300')
    inspect = subs.add_parser('inspect', help='Save a read-only live-inspector JSON response')
    inspect.add_argument('query', help='Quoted inspector query without erisia-inspect prefix')
    inspect.add_argument('--out', type=Path, required=True, help='New local JSON file; parent must exist')
    args = parser.parse_args()
    os.umask(0o077)
    server = args.server.expanduser().resolve(strict=True)
    if args.action == 'snapshot':
        return snapshot(server, args.out.expanduser().absolute())
    if args.action == 'inspect':
        return inspection(server, args.query, args.out.expanduser().absolute())
    if args.action == 'profile':
        def terminate(signum, frame):
            raise KeyboardInterrupt('Termination requested')
        signal.signal(signal.SIGTERM, terminate)
        return profile(server, args.out.expanduser().absolute(), args.seconds, args.only_ticks_over)
    check_command(args.text)
    print(rcon(server, args.text))
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError) as exc:
        print(f'Collection failed: {exc}', file=sys.stderr)
        sys.exit(1)
