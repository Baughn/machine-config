#!/usr/bin/env nix-shell
#!nix-shell -i python3 --packages python3 nvd

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Optional, Tuple

RED = '\033[0;31m'
GREEN = '\033[0;32m'
YELLOW = '\033[0;33m'
BLUE = '\033[0;34m'
RESET = '\033[0m'


def print_info(message: str) -> None:
    print(f"{BLUE}==> {message}{RESET}")


def print_success(message: str) -> None:
    print(f"{GREEN}✓ {message}{RESET}")


def print_warning(message: str) -> None:
    print(f"{YELLOW}⚠ {message}{RESET}")


def print_error(message: str) -> None:
    print(f"{RED}✗ {message}{RESET}")


def run_command(cmd, *, fatal: bool = True, **kwargs) -> subprocess.CompletedProcess:
    print_info(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, **kwargs)
    if result.returncode != 0:
        if fatal:
            print_error(f"Command failed with exit code {result.returncode}")
            sys.exit(result.returncode)
        print_warning(f"Command exited with {result.returncode}")
    return result


def ensure_built_system(hostname: str) -> str:
    result_dir = Path('result') / hostname
    if result_dir.exists():
        return os.path.realpath(result_dir)

    build_result = run_command(
        [
            'nix',
            'build',
            f'.#nixosConfigurations.{hostname}.config.system.build.toplevel',
            '--print-out-paths',
        ],
        capture_output=True,
        text=True,
    )
    return build_result.stdout.strip()


def show_system_diff(target_path: str) -> None:
    print_info('Comparing against the current system...')
    run_command(['nvd', 'diff', '/run/current-system', target_path], fatal=False)


def backup_flake_lock() -> Optional[Path]:
    lock_path = Path('flake.lock')
    if not lock_path.exists():
        return None

    with tempfile.NamedTemporaryFile(delete=False) as tmp:
        tmp.write(lock_path.read_bytes())
    return Path(tmp.name)


def restore_flake_lock(backup_path: Optional[Path]) -> None:
    if not backup_path or not backup_path.exists():
        return

    lock_path = Path('flake.lock')
    lock_path.write_bytes(backup_path.read_bytes())
    print_info('Restored flake.lock from backup.')


def cleanup_backup(backup_path: Optional[Path]) -> None:
    if backup_path and backup_path.exists():
        backup_path.unlink()


def has_flake_lock_changes() -> bool:
    result = run_command(['jj', 'status'], capture_output=True, text=True, fatal=False)
    if result.returncode != 0:
        return False

    for line in result.stdout.splitlines():
        if line.startswith(('A ', 'M ')) and 'flake.lock' in line:
            return True
    return False


def should_squash_into_parent() -> bool:
    result = run_command(
        ['jj', 'log', '-r', '@- & mutable()', '-T', 'description', '--no-graph'],
        capture_output=True,
        text=True,
        fatal=False,
    )
    if result.returncode != 0 or not result.stdout.strip():
        return False

    parent_description = result.stdout.strip()
    return parent_description.startswith('Bump nixpkgs')


def commit_flake_lock() -> None:
    if not has_flake_lock_changes():
        return

    if should_squash_into_parent():
        print_info('Squashing flake.lock into previous "Bump nixpkgs" commit...')
        run_command(['jj', 'squash', '-m', 'Bump nixpkgs', 'flake.lock'], fatal=False)
    else:
        print_info('Creating new "Bump nixpkgs" commit for flake.lock...')
        run_command(['jj', 'commit', '-m', 'Bump nixpkgs', 'flake.lock'], fatal=False)


def get_flake_inputs(exclude: Optional[List[str]] = None) -> List[str]:
    lock_path = Path('flake.lock')
    if not lock_path.exists():
        return []

    data = json.loads(lock_path.read_text())
    inputs = [name for name in data.get('nodes', {}) if name != 'root']
    if exclude:
        excluded = set(exclude)
        inputs = [name for name in inputs if name not in excluded]
    return sorted(inputs)


def update_selected_inputs(inputs: List[str]) -> bool:
    if not inputs:
        print_warning('No inputs selected for update.')
        return False

    cmd = ['nix', '--extra-experimental-features', 'nix-command flakes', 'flake', 'update']
    for name in inputs:
        cmd.extend(['--update-input', name])
    result = run_command(cmd, fatal=False)
    return result.returncode == 0


def build_all_systems(extra_args: List[str]) -> bool:
    cmd = ['nom', 'build', '.#all-systems', *extra_args]
    result = run_command(cmd, fatal=False)
    return result.returncode == 0


def prompt_goal(label: str, default: Optional[str]) -> Optional[str]:
    goal_mapping = {'1': 'switch', '2': 'boot', '3': None}
    default_to_choice = {'switch': '1', 'boot': '2', None: '3'}
    default_choice = default_to_choice[default]

    print(f"\n{label} deployment goal:")
    print('1) switch (apply now)')
    print('2) boot (next reboot)')
    print('3) skip')

    while True:
        choice = input(f"Select [default {default_choice}]: ").strip()
        if not choice:
            choice = default_choice
        if choice in goal_mapping:
            return goal_mapping[choice]
        print_error('Invalid choice, please try again.')


def prompt_deployment() -> Tuple[Optional[str], Optional[str]]:
    print('\nDeploy?')
    print('1) exit')
    print('2) deploy')

    choice = input('Select [1]: ').strip()
    if choice not in {'2'}:
        print_info('Exiting without deployment.')
        return None, None

    local_goal = prompt_goal('Local machine', default='switch')
    remote_goal = prompt_goal('Remote machines', default='boot')
    if not local_goal and not remote_goal:
        print_info('No deployment targets selected.')
    return local_goal, remote_goal


def deploy(local_goal: Optional[str], remote_goal: Optional[str]) -> None:
    if local_goal:
        cmd = ['colmena', 'apply-local', '--sudo']
        if local_goal != 'switch':
            cmd.append(local_goal)
        run_command(cmd)

    if remote_goal:
        cmd = ['colmena', 'apply', '--on', '@remote']
        if remote_goal != 'switch':
            cmd.append(remote_goal)
        if remote_goal == 'boot':
            cmd.append('--reboot')
        run_command(cmd)


def post_deploy_tasks() -> None:
    # Currently handled by determinate nix
    #run_command(['colmena', 'exec', '--on', '@remote', 'nix-collect-garbage', '-d'], fatal=False)
    run_command(['sudo', 'flatpak', 'update', '-y'], fatal=False)


def main() -> None:
    script_dir = Path(__file__).parent.resolve()
    os.chdir(script_dir)

    extra_args = sys.argv[1:]
    backup_path = backup_flake_lock()
    fallback_used = False

    try:
        run_command(['nix', '--extra-experimental-features', 'nix-command flakes', 'flake', 'update'])

        if not build_all_systems(extra_args):
            print_warning('nom build failed; attempting update without nixpkgs-lagging input...')
            if not backup_path:
                print_error('No flake.lock backup available; cannot retry without nixpkgs-lagging.')
                sys.exit(1)

            restore_flake_lock(backup_path)
            inputs = get_flake_inputs(exclude=['nixpkgs-lagging'])
            if not update_selected_inputs(inputs):
                print_error('Fallback update without nixpkgs-lagging failed.')
                sys.exit(1)

            if not build_all_systems(extra_args):
                print_error('Build failed even after excluding nixpkgs-lagging.')
                sys.exit(1)

            fallback_used = True

        commit_flake_lock()

        hostname = subprocess.check_output(['hostname'], text=True).strip()
        built_system = ensure_built_system(hostname)

        show_system_diff(built_system)
        print('\a', end='', flush=True)

        local_goal, remote_goal = prompt_deployment()
        if local_goal or remote_goal:
            deploy(local_goal, remote_goal)
            post_deploy_tasks()

        if fallback_used:
            print_warning('Update completed without nixpkgs-lagging input.')

        print_success('Update script finished.')

    finally:
        cleanup_backup(backup_path)


if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        print_info('\nInterrupted by user, exiting...')
        sys.exit(130)
