#!/usr/bin/env nix-shell
#!nix-shell -i python3 --packages python3 nvd

import json
import os
import subprocess
import sys
import re
import tempfile
from pathlib import Path

# ANSI color codes
RED = '\033[0;31m'
GREEN = '\033[0;32m'
YELLOW = '\033[0;33m'
BLUE = '\033[0;34m'
RESET = '\033[0m'

def print_info(msg):
    print(f"{BLUE}==> {msg}{RESET}")

def print_success(msg):
    print(f"{GREEN}✓ {msg}{RESET}")

def print_error(msg):
    print(f"{RED}✗ {msg}{RESET}")

def print_warning(msg):
    print(f"{YELLOW}⚠ {msg}{RESET}")

def run_command(cmd, check=True):
    """Run a command and return success status."""
    print_info(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=False)
    if check and result.returncode != 0:
        return False
    return True

def save_flake_lock():
    """Save the current flake.lock to a temporary file."""
    with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.lock') as f:
        with open('flake.lock', 'r') as original:
            f.write(original.read())
        return f.name

def restore_flake_lock(backup_path):
    """Restore flake.lock from backup."""
    with open(backup_path, 'r') as backup:
        with open('flake.lock', 'w') as current:
            current.write(backup.read())

def get_inputs_from_flake_lock():
    """Parse flake.lock and return list of input names."""
    with open('flake.lock', 'r') as f:
        lock_data = json.load(f)
    
    inputs = []
    for node_name in lock_data.get('nodes', {}):
        if node_name != 'root':  # Exclude root node
            inputs.append(node_name)
    
    return inputs

def update_all_inputs():
    """Update all flake inputs."""
    print_info("Updating all inputs...")
    return run_command(['nix', '--extra-experimental-features', 'nix-command flakes', 
                       'flake', 'update'])

def update_selective_inputs(inputs_to_update):
    """Update specific inputs."""
    print_info(f"Updating inputs: {', '.join(inputs_to_update)}")
    for input_name in inputs_to_update:
        if not run_command(['nix', '--extra-experimental-features', 'nix-command flakes',
                           'flake', 'update', input_name]):
            return False
    return True

def run_flake_check():
    """Run nix flake check."""
    print_info("Running flake check...")
    return run_command(['nix', 'flake', 'check'])

def try_build(extra_args):
    """Try to build the system configuration."""
    print_info("Building system configuration...")
    cmd = ['colmena', 'build'] + extra_args
    return run_command(cmd)

def show_diff_and_deploy():
    """Show diff and prompt for deployment."""
    # Run nvd diff to show changes
    print_info("Showing system differences...")
    # Get the built system path from Colmena's build output
    result = subprocess.run(['colmena', 'build', '--on', 'saya'], capture_output=True, text=True)
    if result.returncode == 0:
        # Extract system path from Colmena output (it shows the store path)
        built_system = None
        for line in result.stderr.split('\n'):
            store = re.match(r'.*"(/nix/store/.*)".*', line)
            if store:
                built_system = store[1]
                break

        assert built_system is not None
        
        subprocess.run(['nvd', 'diff', '/run/current-system', built_system])
        
        # Check if flake.lock has changed before committing
        diff_check = subprocess.run(['jj', 'diff', '--stat', 'flake.lock'], capture_output=True, text=True)
        if '0 files changed' in diff_check.stdout.strip():
            print_info("No changes to flake.lock to commit.")
        else:
            run_command(['jj', 'commit', '-m', 'Bump nixpkgs', 'flake.lock'])
    
    # Alert sound
    print('\a', end='', flush=True)
    
    # Interactive prompt
    print("\nDeploy?")
    print("1) exit")
    print("2) apply (deploy now)")
    print("3) boot (apply on next boot)")
    
    while True:
        try:
            choice = input("Select [1-3]: ").strip()
            if choice == '1':
                print_info("Exiting without deployment.")
                break
            elif choice == '2':
                print_info("Deploying with Colmena...")
                subprocess.run(['colmena', 'apply'])
                break
            elif choice == '3':
                print_info("Setting new configuration for next boot...")
                subprocess.run(['colmena', 'apply', 'boot'])
                break
            else:
                print_error("Invalid choice. Please select 1, 2, or 3.")
        except KeyboardInterrupt:
            print("\nExiting...")
            break

def main():
    # Change to script directory
    script_dir = Path(__file__).parent.absolute()
    os.chdir(script_dir)
    
    # Get extra arguments for nixos-rebuild
    extra_args = sys.argv[1:]
    
    # Save current flake.lock
    backup_path = save_flake_lock()
    
    try:
        # Step 1: Try updating everything
        if update_all_inputs() and run_flake_check() and try_build(extra_args):
            print_success("Full update successful!")
            show_diff_and_deploy()
        else:
            print_warning("Full update failed, trying without nixpkgs-kernel...")
            
            # Step 2: Restore and try selective update
            restore_flake_lock(backup_path)
            
            # Get all inputs except nixpkgs-kernel
            all_inputs = get_inputs_from_flake_lock()
            inputs_to_update = [inp for inp in all_inputs if inp != 'nixpkgs-kernel']
            
            if inputs_to_update:
                if update_selective_inputs(inputs_to_update) and run_flake_check() and try_build(extra_args):
                    print_success("Selective update successful (excluded nixpkgs-kernel)!")
                    show_diff_and_deploy()
                else:
                    print_error("Build still failing, restoring original flake.lock...")
                    restore_flake_lock(backup_path)
                    print('\a', end='', flush=True)  # Alert sound
                    sys.exit(1)
            else:
                print_error("No inputs to update selectively.")
                restore_flake_lock(backup_path)
                sys.exit(1)
    
    finally:
        # Clean up backup file
        if os.path.exists(backup_path):
            os.unlink(backup_path)

if __name__ == '__main__':
    main()
