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
- Status: Migrated

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

## Tsugumi Server Migration

### Configuration Status
- ✅ **Core system**: Hardware config with all ZFS mounts, boot, networking
- ✅ **Deployment**: Added to flake.nix colmenaHive for remote deployment
- ❌ **Services**: All applications and services temporarily skipped

### Major Skipped Components

#### Web Infrastructure
- **Caddy web server** with 15+ domain proxies:
  - brage.info (file server)
  - madoka.brage.info (minecraft web)
  - grafana.brage.info
  - map.brage.info, incognito.brage.info (minecraft maps)
  - home.brage.info (home assistant)
  - comfyui.brage.info (AI proxy to saya)
  - status.brage.info (prometheus)
  - alertmanager.brage.info
  - znc.brage.info, obico.brage.info, klipper.brage.info
  - racer.brage.info, ar-innna.brage.info
  - qbt.brage.info, todo.brage.info, sonarr.brage.info, radarr.brage.info
  - jellyfin.brage.info, plex.brage.info, store.brage.info
- **Authelia authentication** with TOTP, session management, user database
- **Custom Caddy build** with Cloudflare DNS plugin
- **TLS certificates** via Cloudflare DNS challenge

#### Applications & Services
- **Media Management**:
  - Sonarr (TV show management)
  - Plex media server
  - SilverBullet note-taking system
- **Game Servers**:
  - Minecraft servers (sonarr.nix, minecraft.nix)
  - Bot services (rolebot.nix, sdbot.nix, irctool.nix, aniwatch.nix)
- **Monitoring Stack**:
  - Grafana dashboard server
  - Prometheus with custom UPS monitoring rules
  - Blackbox exporter for connectivity monitoring
  - Custom alerting for UPS status, voltage, frequency, battery level

#### Hardware & Infrastructure
- **UPS Management** (Phoenix TEC VFI 2000):
  - NUT (Network UPS Tools) configuration
  - Serial connection via /dev/ttyUSB0
  - Custom battery pack configuration (24 cells, PbAc)
  - Runtime calculations and shutdown procedures
  - Age-encrypted password management
- **Hardware Support**:
  - NVIDIA GPU persistence daemon
  - AMD GPU drivers
  - Power management and thermal control

#### Data & Sync
- **Syncthing** multi-device synchronization:
  - 4 devices: saya, sayanix, kaho, koyomi
  - 4 folders: default Sync, Music, Documents, secure
  - Device IDs and folder configurations
- **ZRepl Backups**:
  - Push configuration to stash/zrepl
  - 15-minute snapshot intervals
  - Sophisticated retention policies (hourly, daily, weekly, monthly)
  - Excludes dynmap directories for minecraft servers
- **Filesystem Bind Mounts**:
  - /srv/ web server root with multiple bind mounts
  - Media directories (Anime, Movies, TV) mounted for web access
  - User web directories mounted for serving

#### User Management
- **Additional Users**: minecraft, aquagon, nixremote
- **Age Secrets**: Encrypted password and key management
- **SSH Key Management**: Automated key distribution

#### Network Services
- **Firewall Configuration**:
  - TCP: 80, 443 (HTTP/HTTPS)
  - UDP: 34197 (Factorio)
- **DNS Configuration**: Custom DNS servers (1.1.1.1, 1.0.0.1)
- **Network Interface**: Converted from systemd-network to standard NixOS

### Migration Priority for Future
1. **High**: UPS monitoring, basic web serving, media access
2. **Medium**: Game servers, authentication system, monitoring
3. **Low**: Advanced media management, specialized bots

### Technical Notes
- Original used systemd-network with MAC address matching (74:56:3c:b2:26:07)
- Converted to standard NixOS networking with interface-based DHCP
- All ZFS filesystems preserved (20+ mounts including encrypted datasets)
- Age secrets system needs to be re-implemented or replaced
- Custom module system ("me" namespace) not migrated

## Notes

- The current config uses a simpler module structure
- Some features may have been intentionally removed
- Check if any services depending on these features are still needed
- The old config used age secrets - verify if the new config has an alternative
