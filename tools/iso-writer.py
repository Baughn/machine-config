#!/usr/bin/env nix-shell
#!nix-shell -i python3 -p python3 python3Packages.rich
"""
NixOS ISO Builder and USB Writer
A TUI application for building custom NixOS ISOs and writing them to USB drives.
"""

import os
import sys
import subprocess
import json
import time
import threading
import re
import argparse
import tempfile
from pathlib import Path
from typing import List, Dict, Optional, Tuple

try:
    import rich
    from rich.console import Console
    from rich.table import Table
    from rich.progress import Progress, SpinnerColumn, TextColumn, BarColumn, TaskProgressColumn
    from rich.prompt import Prompt, Confirm
    from rich.panel import Panel
    from rich.text import Text
    from rich.live import Live
except ImportError:
    print("Error: This script requires the 'rich' library.")
    print("Install it with: pip install rich")
    sys.exit(1)

console = Console()


class USBDrive:
    """Represents a USB storage device."""
    
    def __init__(self, device: str, size: str, model: str, vendor: str):
        self.device = device
        self.size = size
        self.model = model
        self.vendor = vendor
        self.display_name = f"{vendor} {model}".strip() or "Unknown Device"
    
    def __str__(self):
        return f"{self.device} - {self.display_name} ({self.size})"


class ISOWriter:
    """Main application class for ISO building and USB writing."""
    
    def __init__(self, dry_run: bool = False):
        self.console = Console()
        self.repo_root = Path(__file__).parent.parent
        self.dry_run = dry_run
        self.temp_file = None
        if self.dry_run:
            self.console.print("[yellow]Running in DRY-RUN mode - no actual writes to USB[/yellow]")
        
    def get_usb_drives(self) -> List[USBDrive]:
        """Detect plugged-in USB drives using lsblk."""
        try:
            # Get block devices in JSON format
            result = subprocess.run([
                'lsblk', '-J', '-o', 'NAME,SIZE,TYPE,TRAN,VENDOR,MODEL,MOUNTPOINT'
            ], capture_output=True, text=True, check=True)
            
            data = json.loads(result.stdout)
            usb_drives = []
            
            for device in data.get('blockdevices', []):
                # Look for USB devices that are disks (not partitions)
                if (device.get('tran') == 'usb' and 
                    device.get('type') == 'disk' and
                    device.get('name', '').startswith(('sd', 'nvme'))):
                    
                    drive = USBDrive(
                        device=f"/dev/{device['name']}",
                        size=device.get('size', 'Unknown'),
                        model=device.get('model', '').strip(),
                        vendor=device.get('vendor', '').strip()
                    )
                    usb_drives.append(drive)
            
            return usb_drives
            
        except subprocess.CalledProcessError as e:
            self.console.print(f"[red]Error detecting USB drives: {e}[/red]")
            return []
        except json.JSONDecodeError as e:
            self.console.print(f"[red]Error parsing lsblk output: {e}[/red]")
            return []
    
    def display_usb_drives(self, drives: List[USBDrive]) -> None:
        """Display available USB drives in a table."""
        if not drives:
            self.console.print("[yellow]No USB drives detected.[/yellow]")
            return
        
        self.console.print("\n")
        table = Table(title="Available USB Drives")
        table.add_column("Index", style="cyan", no_wrap=True)
        table.add_column("Device", style="green")
        table.add_column("Name", style="white")
        table.add_column("Size", style="yellow")
        
        for i, drive in enumerate(drives, 1):
            table.add_row(
                str(i),
                drive.device,
                drive.display_name,
                drive.size
            )
        
        self.console.print(table)
    
    def select_usb_drive(self, drives: List[USBDrive]) -> Optional[USBDrive]:
        """Let user select a USB drive."""
        if self.dry_run:
            # In dry-run mode, create a temp file and return a mock USB drive
            self.temp_file = tempfile.NamedTemporaryFile(prefix="nixos-iso-dryrun-", suffix=".img", delete=False)
            self.temp_file.close()
            mock_drive = USBDrive(
                device=self.temp_file.name,
                size="16GB",
                model="DryRun Mock Drive",
                vendor="Test"
            )
            self.console.print(f"[yellow]Dry-run mode: Using temporary file {self.temp_file.name}[/yellow]")
            return mock_drive
            
        if not drives:
            return None
        
        self.display_usb_drives(drives)
        
        while True:
            try:
                choice = Prompt.ask(
                    "Select USB drive by index",
                    choices=[str(i) for i in range(1, len(drives) + 1)],
                    default="1"
                )
                return drives[int(choice) - 1]
            except (ValueError, IndexError):
                self.console.print("[red]Invalid selection. Please try again.[/red]")
    
    def check_prerequisites(self) -> bool:
        """Check if required tools are available."""
        required_tools = ['nix', 'dd', 'lsblk']
        missing_tools = []
        
        for tool in required_tools:
            if subprocess.run(['which', tool], capture_output=True).returncode != 0:
                missing_tools.append(tool)
        
        if missing_tools:
            self.console.print(f"[red]Missing required tools: {', '.join(missing_tools)}[/red]")
            return False
        
        return True
    
    def build_iso(self) -> Optional[Path]:
        """Build the NixOS ISO."""
        iso_path = None
        
        with Progress(
            SpinnerColumn(),
            TextColumn("[progress.description]{task.description}"),
            BarColumn(),
            TaskProgressColumn(),
            console=self.console
        ) as progress:
            
            task = progress.add_task("Building NixOS ISO...", total=None)
            
            try:
                # Change to repository directory
                original_cwd = os.getcwd()
                os.chdir(self.repo_root)
                
                # Build the ISO
                result = subprocess.run([
                    'nix', 'build', 
                    '.#packages.x86_64-linux.iso',
                    '--print-out-paths'
                ], capture_output=True, text=True, check=True)
                
                # Extract the ISO path from the build result
                build_path = Path(result.stdout.strip())
                iso_files = list(build_path.glob('iso/*.iso'))
                
                if iso_files:
                    iso_path = iso_files[0]
                    progress.update(task, description=f"ISO built: {iso_path.name}")
                else:
                    self.console.print("[red]Error: No ISO file found in build output[/red]")
                
            except subprocess.CalledProcessError as e:
                self.console.print(f"[red]Error building ISO: {e}[/red]")
                if e.stderr:
                    self.console.print(f"[red]Error details: {e.stderr}[/red]")
            finally:
                os.chdir(original_cwd)
        
        return iso_path
    
    def parse_dd_progress(self, line: str) -> Optional[Tuple[int, str]]:
        """Parse dd progress output to extract bytes written and transfer rate.
        
        Example line: "4160749568 bytes (4.2 GB, 3.9 GiB) copied, 7 s, 592 MB/s"
        Returns: (bytes_written, transfer_rate) or None if parsing fails
        """
        # Pattern to match dd progress output
        pattern = r'(\d+) bytes .* copied, .* s, (.+/s)'
        match = re.search(pattern, line)
        
        if match:
            bytes_written = int(match.group(1))
            transfer_rate = match.group(2)
            return bytes_written, transfer_rate
        
        return None

    def verify_usb_contents(self, iso_path: Path, usb_drive: USBDrive) -> bool:
        """Verify that the USB drive contents match the ISO file using checksums."""
        
        # Get ISO file size for progress calculation
        iso_size = iso_path.stat().st_size
        
        with Progress(
            SpinnerColumn(),
            TextColumn("[progress.description]{task.description}"),
            BarColumn(),
            TaskProgressColumn(),
            TextColumn("[cyan]{task.fields[transfer_rate]}"),
            console=self.console
        ) as progress:
            
            # Calculate ISO checksum
            iso_task = progress.add_task(
                "Calculating ISO checksum...", 
                total=iso_size,
                transfer_rate=""
            )
            
            try:
                # Use dd with sha256sum to calculate ISO checksum
                iso_checksum_process = subprocess.Popen([
                    'dd', f'if={iso_path}', 'bs=64M', 'status=progress'
                ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                
                sha256_iso_process = subprocess.Popen([
                    'sha256sum'
                ], stdin=iso_checksum_process.stdout, stdout=subprocess.PIPE, 
                stderr=subprocess.PIPE, text=True)
                
                iso_checksum_process.stdout.close()
                
                # Monitor ISO checksum progress
                bytes_processed = 0
                while iso_checksum_process.poll() is None:
                    if iso_checksum_process.stderr:
                        line = iso_checksum_process.stderr.readline()
                        if line:
                            parsed = self.parse_dd_progress(line.strip())
                            if parsed:
                                bytes_written, transfer_rate = parsed
                                progress.update(
                                    iso_task,
                                    completed=bytes_written,
                                    transfer_rate=transfer_rate
                                )
                    time.sleep(0.1)
                
                iso_checksum_process.wait()
                iso_checksum, _ = sha256_iso_process.communicate()
                iso_hash = iso_checksum.split()[0] if iso_checksum else ""
                
                if iso_checksum_process.returncode != 0 or not iso_hash:
                    self.console.print("[red]Error calculating ISO checksum[/red]")
                    return False
                
                progress.update(iso_task, completed=iso_size, description="ISO checksum calculated")
                
                # Calculate USB checksum
                usb_task = progress.add_task(
                    f"Verifying USB contents for {usb_drive.device}...",
                    total=iso_size,
                    transfer_rate=""
                )
                
                # Use dd with sha256sum to calculate USB checksum (read exact ISO size)
                usb_checksum_process = subprocess.Popen([
                    'sudo', 'dd', f'if={usb_drive.device}', 
                    f'bs=64M', f'iflag=count_bytes', f'count={iso_size}',
                    'status=progress'
                ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
                
                sha256_usb_process = subprocess.Popen([
                    'sha256sum'
                ], stdin=usb_checksum_process.stdout, stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, text=True)
                
                usb_checksum_process.stdout.close()
                
                # Monitor USB checksum progress
                while usb_checksum_process.poll() is None:
                    if usb_checksum_process.stderr:
                        line = usb_checksum_process.stderr.readline()
                        if line:
                            parsed = self.parse_dd_progress(line.strip())
                            if parsed:
                                bytes_written, transfer_rate = parsed
                                progress.update(
                                    usb_task,
                                    completed=min(bytes_written, iso_size),
                                    transfer_rate=transfer_rate
                                )
                    time.sleep(0.1)
                
                usb_checksum_process.wait()
                usb_checksum, _ = sha256_usb_process.communicate()
                usb_hash = usb_checksum.split()[0] if usb_checksum else ""
                
                if usb_checksum_process.returncode != 0 or not usb_hash:
                    self.console.print("[red]Error calculating USB checksum[/red]")
                    return False
                
                progress.update(usb_task, completed=iso_size, description="USB verification complete")
                
                # Compare checksums
                if iso_hash == usb_hash:
                    self.console.print("[green]✓ Verification successful: USB contents match ISO file![/green]")
                    return True
                else:
                    self.console.print("[red]✗ Verification failed: USB contents do not match ISO file[/red]")
                    self.console.print(f"[dim]ISO checksum:  {iso_hash}[/dim]")
                    self.console.print(f"[dim]USB checksum:  {usb_hash}[/dim]")
                    return False
                    
            except subprocess.CalledProcessError as e:
                self.console.print(f"[red]Error during verification: {e}[/red]")
                return False
            except Exception as e:
                self.console.print(f"[red]Unexpected error during verification: {e}[/red]")
                return False

    def write_iso_to_usb(self, iso_path: Path, usb_drive: USBDrive) -> bool:
        """Write ISO to USB drive using dd."""
        # Confirm the operation
        warning_text = Text()
        warning_text.append("⚠️  WARNING: This will COMPLETELY ERASE all data on:\n", style="red bold")
        warning_text.append(f"   {usb_drive}\n", style="yellow")
        warning_text.append("This action cannot be undone!", style="red bold")
        
        self.console.print(Panel(warning_text, title="Destructive Operation"))
        
        if not Confirm.ask("Are you absolutely sure you want to continue?", default=False):
            return False
        
        # Get ISO file size for progress calculation
        iso_size = iso_path.stat().st_size
        
        # Unmount any mounted partitions
        try:
            subprocess.run(['umount', f"{usb_drive.device}*"], 
                         capture_output=True, check=False)
        except:
            pass  # Ignore errors, partitions might not be mounted
        
        # Write ISO to USB
        with Progress(
            SpinnerColumn(),
            TextColumn("[progress.description]{task.description}"),
            BarColumn(),
            TaskProgressColumn(),
            TextColumn("[cyan]{task.fields[transfer_rate]}"),
            console=self.console
        ) as progress:
            
            task = progress.add_task(
                f"Writing ISO to {usb_drive.device}...", 
                total=iso_size,
                transfer_rate=""
            )
            
            try:
                # Use dd with progress monitoring
                dd_process = subprocess.Popen([
                    'sudo', 'dd',
                    f'if={iso_path}',
                    f'of={usb_drive.device}',
                    'bs=64M',
                    'status=progress',
                    'conv=fsync',
                    'oflag=direct',
                ], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, bufsize=1)
                
                # Monitor progress by parsing dd's stderr output
                while dd_process.poll() is None:
                    if dd_process.stderr:
                        line = dd_process.stderr.readline()
                        if line:
                            parsed = self.parse_dd_progress(line.strip())
                            if parsed:
                                bytes_written, transfer_rate = parsed
                                progress.update(
                                    task, 
                                    completed=bytes_written,
                                    transfer_rate=transfer_rate
                                )
                    time.sleep(0.1)
                
                # Read any remaining output
                remaining_output = dd_process.stderr.read()
                if remaining_output:
                    for line in remaining_output.splitlines():
                        parsed = self.parse_dd_progress(line.strip())
                        if parsed:
                            bytes_written, transfer_rate = parsed
                            progress.update(
                                task, 
                                completed=bytes_written,
                                transfer_rate=transfer_rate
                            )
                
                dd_process.wait()
                
                if dd_process.returncode == 0:
                    progress.update(task, completed=iso_size, description="ISO written successfully!")
                    
                    # Sync to ensure all data is written
                    subprocess.run(['sync'], check=True)
                    
                    self.console.print("[green]✓ ISO successfully written to USB drive![/green]")
                    return True
                else:
                    error_output = dd_process.stderr.read() if dd_process.stderr else "Unknown error"
                    self.console.print(f"[red]Error writing ISO: {error_output}[/red]")
                    return False
                    
            except subprocess.CalledProcessError as e:
                self.console.print(f"[red]Error writing ISO: {e}[/red]")
                return False
    
    def run(self):
        """Main application loop."""
        # Check prerequisites
        if not self.check_prerequisites():
            return 1
        
        # Detect USB drives (even in dry-run to exercise the code)
        usb_drives = self.get_usb_drives()
        
        if not usb_drives:
            self.console.print("[yellow]No USB drives found. Please plug in a USB drive and try again.[/yellow]")
            return 1
        
        # Select USB drive
        selected_drive = self.select_usb_drive(usb_drives)
        if not selected_drive:
            self.console.print("[yellow]No drive selected. Exiting.[/yellow]")
            return 1
        
        self.console.print(f"\n[green]Selected drive: {selected_drive}[/green]")
        
        # Build ISO
        self.console.print("\n[cyan]Building NixOS ISO...[/cyan]")
        iso_path = self.build_iso()
        
        if not iso_path:
            self.console.print("[red]Failed to build ISO. Exiting.[/red]")
            return 1
        
        self.console.print(f"[green]ISO built successfully: {iso_path}[/green]")
        
        # Write ISO to USB
        self.console.print(f"\n[cyan]Writing ISO to USB drive...[/cyan]")
        if self.write_iso_to_usb(iso_path, selected_drive):
            # Verify the written USB contents
            self.console.print(f"\n[cyan]Verifying USB contents...[/cyan]")
            if self.verify_usb_contents(iso_path, selected_drive):
                self.console.print("\n[green bold]✓ Process completed successfully![/green bold]")
                self.console.print(f"Your USB drive {selected_drive.device} is now ready to boot.")
                return 0
            else:
                self.console.print("\n[red]USB verification failed. The drive may be corrupted.[/red]")
                return 1
        else:
            self.console.print("\n[red]Failed to write ISO to USB drive.[/red]")
            return 1


def main():
    """Entry point."""
    parser = argparse.ArgumentParser(
        description="NixOS ISO Builder and USB Writer"
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Run in dry-run mode (writes to temporary file instead of USB)"
    )
    args = parser.parse_args()
    
    app = None
    try:
        app = ISOWriter(dry_run=args.dry_run)
        return app.run()
    except KeyboardInterrupt:
        console.print("\n[yellow]Operation cancelled by user.[/yellow]")
        return 1
    except Exception as e:
        console.print(f"\n[red]Unexpected error: {e}[/red]")
        return 1
    finally:
        # Clean up temp file if in dry-run mode
        if app and app.dry_run and app.temp_file:
            try:
                os.unlink(app.temp_file.name)
                console.print(f"[yellow]Cleaned up temporary file: {app.temp_file.name}[/yellow]")
            except:
                pass


if __name__ == "__main__":
    sys.exit(main())
