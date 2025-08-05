#!/usr/bin/env nix-shell
#!nix-shell -i python3 --packages python3 nvd

import json
import os
import subprocess
import sys
import re
import tempfile
from pathlib import Path
from dataclasses import dataclass
from typing import List, Dict, Optional, Callable, Any

# ANSI color codes
RED = '\033[0;31m'
GREEN = '\033[0;32m'
YELLOW = '\033[0;33m'
BLUE = '\033[0;34m'
RESET = '\033[0m'

# ============================================================================
# UTILITY FUNCTIONS
# ============================================================================

def print_info(msg):
    print(f"{BLUE}==> {msg}{RESET}")

def print_success(msg):
    print(f"{GREEN}✓ {msg}{RESET}")

def print_error(msg):
    print(f"{RED}✗ {msg}{RESET}")

def print_warning(msg):
    print(f"{YELLOW}⚠ {msg}{RESET}")

# ============================================================================
# EFFECTORS - Pure functions that perform individual operations
# ============================================================================

@dataclass
class ExecutionContext:
    """Context passed through execution pipeline."""
    extra_args: List[str]
    backup_path: Optional[str] = None
    inputs_to_exclude: List[str] = None
    
    def __post_init__(self):
        if self.inputs_to_exclude is None:
            self.inputs_to_exclude = []

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

def update_all_inputs(ctx: ExecutionContext) -> bool:
    """Update all flake inputs."""
    print_info("Updating all inputs...")
    return run_command(['nix', '--extra-experimental-features', 'nix-command flakes', 
                       'flake', 'update'])

def update_selective_inputs(ctx: ExecutionContext) -> bool:
    """Update specific inputs, excluding those in ctx.inputs_to_exclude."""
    all_inputs = get_inputs_from_flake_lock()
    inputs_to_update = [inp for inp in all_inputs if inp not in ctx.inputs_to_exclude]
    
    if not inputs_to_update:
        print_error("No inputs to update after exclusions.")
        return False
        
    print_info(f"Updating inputs: {', '.join(inputs_to_update)}")
    for input_name in inputs_to_update:
        if not run_command(['nix', '--extra-experimental-features', 'nix-command flakes',
                           'flake', 'update', input_name]):
            return False
    return True

def run_flake_check(ctx: ExecutionContext) -> bool:
    """Run nix flake check."""
    print_info("Running flake check...")
    return run_command(['nix', 'flake', 'check'])

def try_build(ctx: ExecutionContext) -> bool:
    """Try to build the system configuration."""
    print_info("Building system configurations with nom...")
    # Build all systems using nom
    cmd = ['nom', 'build', '.#all-systems'] + ctx.extra_args
    return run_command(cmd)

def backup_flake_lock(ctx: ExecutionContext) -> bool:
    """Create backup of flake.lock."""
    ctx.backup_path = save_flake_lock()
    return True

def restore_flake_lock_from_backup(ctx: ExecutionContext) -> bool:
    """Restore flake.lock from backup."""
    if ctx.backup_path:
        restore_flake_lock(ctx.backup_path)
        return True
    return False

def show_diff_and_deploy(ctx: ExecutionContext) -> bool:
    """Show diff and prompt for deployment."""
    # Get current hostname
    hostname = subprocess.run(['hostname'], capture_output=True, text=True).stdout.strip()
    
    # Run nvd diff to show changes
    print_info(f"Showing system differences for {hostname}...")
    # Build the specific system configuration with nom and get the output path
    result = subprocess.run(['nom', 'build', f'.#nixosConfigurations.{hostname}.config.system.build.toplevel', '--print-out-paths'], capture_output=True, text=True)
    if result.returncode == 0:
        # The built system path is printed to stdout
        built_system = result.stdout.strip()
        assert built_system and built_system.startswith('/nix/store/')
        
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
                return True
            elif choice == '2':
                # Deploy local machine immediately
                print_info(f"Deploying local machine ({hostname})...")
                subprocess.run(['colmena', 'apply-local', '--sudo'])
                
                # Deploy remote machines with boot option (apply after reboot)
                print_info("Deploying remote machines (will apply after reboot)...")
                subprocess.run(['colmena', 'apply', '--on', '@remote', '--reboot', 'boot'])
                return True
            elif choice == '3':
                # Deploy local machine for next boot
                print_info(f"Setting local machine ({hostname}) configuration for next boot...")
                subprocess.run(['colmena', 'apply-local', '--sudo', 'boot'])
                
                # Deploy remote machines with boot option (apply after reboot)
                print_info("Deploying remote machines (will apply after reboot)...")
                subprocess.run(['colmena', 'apply', '--on', '@remote', '--reboot', 'boot'])
                return True
            else:
                print_error("Invalid choice. Please select 1, 2, or 3.")
        except KeyboardInterrupt:
            print("\nExiting...")
            return True

def cleanup_backup(ctx: ExecutionContext) -> bool:
    """Clean up backup file."""
    if ctx.backup_path and os.path.exists(ctx.backup_path):
        os.unlink(ctx.backup_path)
    return True

def alert_sound(ctx: ExecutionContext) -> bool:
    """Play alert sound."""
    print('\a', end='', flush=True)
    return True

def remote_garbage_collect(ctx: ExecutionContext) -> bool:
    """Run garbage collection on remote machines."""
    print_info("Running garbage collection on remote machines...")
    return run_command(['colmena', 'exec', '--on', '@remote', 'nix-collect-garbage', '-d'])

def update_flatpaks(ctx: ExecutionContext) -> bool:
    """Update Flatpak applications."""
    print_info("Updating Flatpak applications...")
    return run_command(['sudo', 'flatpak', 'update', '-y'])

# ============================================================================
# POLICY DEFINITIONS - Declarative strategies
# ============================================================================

@dataclass
class Strategy:
    """A declarative update strategy."""
    name: str
    description: str
    steps: List[str]
    success_message: str
    failure_message: str
    fallback_strategy: Optional[str] = None
    on_failure_steps: List[str] = None
    
    def __post_init__(self):
        if self.on_failure_steps is None:
            self.on_failure_steps = []

# Available effector functions mapped by name
EFFECTORS: Dict[str, Callable[[ExecutionContext], bool]] = {
    'backup_flake_lock': backup_flake_lock,
    'update_all_inputs': update_all_inputs,
    'update_selective_inputs': update_selective_inputs,
    'run_flake_check': run_flake_check,
    'try_build': try_build,
    'show_diff_and_deploy': show_diff_and_deploy,
    'restore_flake_lock_from_backup': restore_flake_lock_from_backup,
    'cleanup_backup': cleanup_backup,
    'alert_sound': alert_sound,
    'remote_garbage_collect': remote_garbage_collect,
    'update_flatpaks': update_flatpaks,
}

# Update strategies defined declaratively
UPDATE_STRATEGIES: Dict[str, Strategy] = {
    'full_update': Strategy(
        name='full_update',
        description='Update all inputs and build',
        steps=['backup_flake_lock', 'update_all_inputs', 'try_build', 'show_diff_and_deploy', 'remote_garbage_collect', 'update_flatpaks'],
        success_message='Full update successful!',
        failure_message='Full update failed, trying selective update...',
        fallback_strategy='selective_update',
        on_failure_steps=['restore_flake_lock_from_backup']
    ),
    
    'selective_update': Strategy(
        name='selective_update',
        description='Update inputs excluding problematic ones',
        steps=['update_selective_inputs', 'try_build', 'show_diff_and_deploy', 'remote_garbage_collect', 'update_flatpaks'],
        success_message='Selective update successful (excluded problematic inputs)!',
        failure_message='Build still failing, restoring original flake.lock...',
        fallback_strategy='restore_and_exit',
        on_failure_steps=['restore_flake_lock_from_backup', 'alert_sound']
    ),
    
    'restore_and_exit': Strategy(
        name='restore_and_exit',
        description='Restore flake.lock and exit with error',
        steps=['restore_flake_lock_from_backup', 'alert_sound'],
        success_message='',
        failure_message='Failed to restore backup or no backup available.',
        fallback_strategy=None
    )
}

# ============================================================================
# POLICY EXECUTION ENGINE
# ============================================================================

def execute_strategy(strategy: Strategy, ctx: ExecutionContext) -> bool:
    """Execute a strategy by running its steps."""
    print_info(f"Executing strategy: {strategy.name} - {strategy.description}")
    
    for step_name in strategy.steps:
        if step_name not in EFFECTORS:
            print_error(f"Unknown effector: {step_name}")
            return False
            
        effector = EFFECTORS[step_name]
        if not effector(ctx):
            print_warning(f"Step '{step_name}' failed in strategy '{strategy.name}'")
            return False
    
    return True

def run_update_pipeline(ctx: ExecutionContext) -> bool:
    """Run the complete update pipeline with fallback strategies."""
    # Set up kernel exclusion for selective updates
    ctx.inputs_to_exclude = ['nixpkgs-kernel']
    
    current_strategy_name = 'full_update'
    
    while current_strategy_name:
        strategy = UPDATE_STRATEGIES.get(current_strategy_name)
        if not strategy:
            print_error(f"Unknown strategy: {current_strategy_name}")
            return False
            
        success = execute_strategy(strategy, ctx)
        
        if success:
            if strategy.success_message:
                print_success(strategy.success_message)
            return True
        else:
            if strategy.failure_message:
                print_warning(strategy.failure_message)
            
            # Execute failure steps
            for step_name in strategy.on_failure_steps:
                if step_name in EFFECTORS:
                    EFFECTORS[step_name](ctx)
            
            # Move to fallback strategy
            current_strategy_name = strategy.fallback_strategy
            
            # Special case: if we're going to exit, do it now
            if current_strategy_name == 'restore_and_exit':
                execute_strategy(UPDATE_STRATEGIES[current_strategy_name], ctx)
                return False
    
    return False

# ============================================================================
# MAIN ORCHESTRATOR
# ============================================================================

def main():
    # Change to script directory
    script_dir = Path(__file__).parent.absolute()
    os.chdir(script_dir)
    
    # Get extra arguments for colmena build
    extra_args = sys.argv[1:]
    
    # Create execution context
    ctx = ExecutionContext(extra_args=extra_args)
    
    try:
        # Run the declarative update pipeline
        success = run_update_pipeline(ctx)
        
        if not success:
            sys.exit(1)
    
    finally:
        # Clean up backup file
        cleanup_backup(ctx)

if __name__ == '__main__':
    main()
