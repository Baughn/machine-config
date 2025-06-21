# Migration Plan: Features Missing from Current Configuration

This document tracks features present in ../nixos-old/saya/configuration.nix that are not yet in the current configuration.

## Major Missing Features

### 1. Build Infrastructure
- **Distributed builds on tsugumi**
  - `nix.buildMachines` configuration for remote building
  - `nix.settings.cores = 8` to limit local core usage
  - Status: Not migrated

### 2. Backup System
- **Restic backups to tsugumi**
  - Automated backups every 30 minutes
  - Backs up /home/svein with cache exclusions
  - Retention policy: 36 hourly, 7 daily, 4 weekly, 3 monthly
  - Uses compression=max
  - Status: Not migrated

### 3. Storage & Filesystems
- **ZFS support** (/home/svein/nixos-old/modules/zfs.nix)
  - Auto-snapshots with UTC timestamps
  - Auto-scrub enabled
  - Performance tuning (txg_timeout = 60)
  - Status: Not migrated
- **Additional filesystems**
  - /home/svein/AI (ZFS volume)
  - /tsugumi (SSHFS mount with auto-mount)
  - /srv/web (bind mount from /home/svein/web)
  - Status: Not migrated

### 4. Virtualization
- **Docker and container support** (me.virtualisation.enable)
  - Docker with nvidia-container-toolkit
  - LXD support
  - QEMU and nixos-shell
  - docker-compose
  - Status: Not migrated

### 5. Desktop Software
- **Flatpak support**
- **Missing packages:**
  - krita
  - google-chrome
  - yt-dlp
  - steam-run
  - KanjiTomo (custom desktop item)
  - discord
  - mpv
  - prismlauncher
  - gamescope
  - kernel perf tools
- **GLFW overlay** with custom patches
- Status: Partially migrated (some may be in desktop.nix)

### 6. System Configuration
- **Boot parameters:**
  - boot.shell_on_fail
  - systemd.enableEmergencyMode = true
- **Hardware quirks:**
  - Logitech G903 mouse scroll wheel fix
  - WINE_CPU_TOPOLOGY for AMD X3D
- **Performance:**
  - system76-scheduler
  - CPU frequency governor = schedutil
- Status: Not migrated

### 7. Network Services
- **Additional firewall ports:**
  - 80, 443 (HTTP/HTTPS)
  - 6987 (rtorrent)
  - 3000 (Textchat-ui)
  - 25565 (Minecraft)
  - 10401 (Wireguard)
  - 5200, 5201 (Stationeers)
- Status: Not migrated

## Missing Module Imports

The old configuration imported these modules that are not in the current setup:

### From /home/svein/nixos-old/modules/:
1. **basics.nix** - Core system configuration including:
   - User management (users.nix)
   - Log rotation (logrotate.nix)
   - Machine naming (naming.nix)
   - Non-nix software support (nonnix.nix)
   - Launchable packages system (launchable.nix)
   - Spectre mitigations disabled
   - SSH keys management
   - Age secrets for passwords

2. **monitoring.nix** - Prometheus/Grafana stack (enabled by default)

3. **nginx.nix** - Web server with custom MIME types

4. **resilience.nix** - System resilience features

5. **virtualisation.nix** - Container and VM support

6. **exceptions.nix** - Exception handling

7. **ipblock.nix** - IP blocking/filtering

8. **nix-serve.nix** - Binary cache server (was commented out)

9. **mcupdater.nix** - Minecraft updater (imported via desktop.nix)

## Module Configuration Options

The old config used a custom "me" namespace for module options:
- `me.virtualisation.enable = true`
- `me.monitoring.enable = true` (default)
- `me.monitoring.zfs = false` (default)

## Other Configuration Details

### Cachix Configuration
The old modules/default.nix included:
- cuda-maintainers.cachix.org
- cache.flox.dev

### Package Management
- nix-index enabled (instead of command-not-found)
- nix-ld enabled for non-nix binaries

### User Configuration
- users.include system for managing users
- Mutable users disabled
- Root password from age secrets
- Default shell: zsh

## Migration Priority

### High Priority:
1. Backup system (data protection)
2. ZFS filesystem mounts (if still using ZFS)
3. Firewall ports (for active services)

### Medium Priority:
1. Distributed builds (performance)
2. Monitoring stack
3. Docker/virtualization (if needed)

### Low Priority:
1. Desktop applications (can be installed as needed)
2. Hardware quirks (unless experiencing issues)
3. Performance optimizations

## Notes

- The current config uses a simpler module structure
- Some features may have been intentionally removed
- Check if any services depending on these features are still needed
- The old config used age secrets - verify if the new config has an alternative