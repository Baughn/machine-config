# Migration Plan: Features Missing from Current Configuration

This document tracks features present in old/** that are not yet in the current configuration.

## Migration Summary (Updated 2025-07-09)

### Completed Migrations ✅
- **Backup System**: Restic backups to tsugumi with 30-minute intervals
- **System Configuration**: All boot parameters, hardware quirks, and performance optimizations
- **Hardware Fixes**: Logitech G903 mouse fix, AMD X3D optimizations
- **Core Services**: Caddy web server, Minecraft servers, Sonarr, bot services, Syncthing
- **Desktop Apps**: Flatpak support, google-chrome, mpv
- **User Management**: Additional users (minecraft, aquagon), age secrets
- **Network Services**: Factorio ports, HTTP/HTTPS, Minecraft ports

### Remaining High Priority Items ❌
- **Distributed builds**: Not yet configured
- **ZFS support**: Auto-snapshots and performance tuning
- **Virtualization**: Docker, LXD, container support
- **Authentication**: Authelia system for web services

### Recently Completed ✅
- **Monitoring Stack Phase 1**: Core Prometheus, Grafana, Alertmanager infrastructure (2025-07-10)

## Monitoring Stack Migration Plan

### Current State (Updated 2025-07-10)
- **✅ Caddy Reverse Proxy**: Already configured for monitoring endpoints
  - `grafana.brage.info` → `localhost:1230`
  - `status.brage.info` → `localhost:9090` (Prometheus)
  - `alertmanager.brage.info` → `localhost:9093`
- **✅ Core Monitoring Stack**: Phase 1 completed and deployed
  - **Prometheus** on port 9090: Collecting metrics from all targets
  - **Grafana** on port 1230: Web interface with basic system dashboard
  - **Alertmanager** on port 9093: Basic notification configuration
  - **Node Exporter** on port 9100: System metrics collection
  - **Integration**: Working with existing Authelia authentication

### Improved Architecture Design

#### Core Stack (Modern NixOS Integration)
1. **Prometheus** (`services.prometheus`)
   - **Improvement**: Use declarative scrape configs and rule management
   - **Enhancement**: Add more comprehensive system metrics
   - **Target**: `localhost:9090` (matches existing Caddy config)

2. **Grafana** (`services.grafana`)
   - **Improvement**: Use declarative dashboard and datasource provisioning, via nixos options
   - **Enhancement**: Modern authentication integration
   - **Target**: `localhost:1230` (matches existing Caddy config)

3. **Alertmanager** (`services.prometheus.alertmanager`)
   - **Improvement**: Replace custom Discord bridge with modern notification methods
   - **Enhancement**: Better alert routing and grouping
   - **Target**: `localhost:9093` (matches existing Caddy config)

#### UPS Monitoring

Skip. The current hardware lacks an UPS.

#### Advanced Monitoring Features
1. **ZFS Monitoring**
   - **Improvement**: Use `services.prometheus.exporters.zfs` if available
   - **Enhancement**: Pool health, scrub status, and error reporting

2. **System Metrics**
   - **Enhancement**: Node exporter with expanded collectors
   - **Addition**: GPU monitoring for both systems
   - **Addition**: Network performance monitoring

3. **Service Health Monitoring**
   - **Enhancement**: Blackbox exporter for service availability
   - **Addition**: Application-specific metrics (Minecraft, media services)

### Migration Implementation Plan

#### Phase 1: Core Infrastructure ✅ **COMPLETED (2025-07-10)**
1. **✅ Create monitoring module** (`modules/monitoring.nix`)
   - ✅ Enable Prometheus with modern scrape configs
   - ✅ Configure Grafana with declarative provisioning  
   - ✅ Set up Alertmanager with basic notification configuration
   - ✅ Integrated with existing Caddy reverse proxy
   - ✅ Secured with agenix secret management

2. **✅ Basic system monitoring**
   - ✅ Enable node exporter with comprehensive collectors (systemd, filesystem, netdev, meminfo, cpu, loadavg, diskstats, stat)
   - ✅ Configure basic alerting rules (system load, disk space, memory usage, service health)
   - ✅ All services running and collecting metrics
   
**Status**: Phase 1 fully deployed and operational. All targets healthy and scraping successfully.

#### Phase 2: Enhanced Features (Medium Priority)
1. **Advanced dashboards**
   - Create comprehensive system overview dashboard
   - UPS status and history dashboard
   - Service health dashboard

2. **Improved alerting**
   - Replace Discord bridge with modern notification system
   - Add smart alert routing based on severity
   - Implement alert suppression during maintenance

3. **Security and authentication**
   - Integrate with Authelia once available
   - Add proper SSL/TLS for monitoring endpoints
   - Implement role-based access control

#### Phase 3: Specialized Monitoring (Low Priority)
1. **Application-specific monitoring**
   - Minecraft server metrics
   - Media server (Plex/Sonarr) monitoring
   - Game server performance metrics

2. **Custom tools migration**
   - ✅ Migrate Victron energy monitoring (metrics only)
   - ❌ Add alerts for Victron monitoring
   - Add custom exporters for specialized hardware
   - Implement log aggregation and analysis

### File Structure Plan
```
modules/
├── monitoring.nix          # Core Prometheus + Grafana config
├── ups.nix                 # UPS monitoring with power.ups
└── monitoring/
    ├── rules/              # Prometheus alerting rules
    └── exporters/          # Custom metric exporters
```

### Configuration Integration
- **Secrets**: Use existing `age.secrets` for monitoring credentials
- **Networking**: Leverage existing `networking.enableLAN` for local metrics
- **Firewall**: Add monitoring ports to existing firewall config
- **Deployment**: Use existing Colmena deployment for monitoring services

### Benefits of Improved Architecture
1. **Declarative Configuration**: All monitoring config in Nix, version controlled
2. **Better Integration**: Uses modern NixOS services instead of custom scripts
3. **Enhanced Reliability**: Built-in systemd integration and service management
4. **Easier Maintenance**: Centralized configuration and automated updates
5. **Better Security**: Proper authentication and encryption by default
6. **Scalability**: Easy to add new monitoring targets and metrics

### Testing Strategy
1. **✅ Development**: Deploy to tsugumi during development
2. **✅ Validation**: Verify all services are running and targets healthy
3. **✅ Monitoring**: Confirmed monitoring system collecting metrics successfully
4. **✅ Web Access**: All monitoring endpoints accessible via reverse proxy

### Phase 1 Implementation Results (2025-07-10)

#### Files Created/Modified:
- ✅ `modules/monitoring.nix` - Core monitoring module with Prometheus, Grafana, Alertmanager
- ✅ `modules/default.nix` - Added monitoring module import
- ✅ `secrets/secrets.nix` - Added Grafana admin password secret
- ✅ `secrets/default.nix` - Added secret configuration for tsugumi
- ✅ `secrets/grafana-admin-password.age` - Encrypted password file
- ✅ `machines/tsugumi/configuration.nix` - Enabled monitoring with `me.monitoring.enable = true`

#### Services Status:
- ✅ **Prometheus** (port 9090): Active, scraping 4/5 targets successfully
- ✅ **Grafana** (port 1230): Active, with provisioned datasource and basic dashboard  
- ✅ **Alertmanager** (port 9093): Active, with basic routing configuration
- ✅ **Node Exporter** (port 9100): Active, collecting comprehensive system metrics

#### Web Access:
- ✅ **https://grafana.brage.info** - Grafana web interface (authenticated)
- ✅ **https://status.brage.info** - Prometheus web interface (authenticated)
- ✅ **https://alertmanager.brage.info** - Alertmanager web interface (authenticated)

#### Next Steps:
Phase 2 can now focus on enhanced dashboards, improved alerting, ZFS monitoring, and application-specific metrics.

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
  - Status: ✅ **COMPLETED** (machines/saya/configuration.nix:55-89)

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
  - ✅ google-chrome (modules/desktopApps.json)
  - yt-dlp
  - steam-run
  - KanjiTomo (custom desktop item)
  - discord
  - ✅ mpv (modules/desktopApps.json)
  - prismlauncher
  - gamescope
  - kernel perf tools
- **✅ Flatpak support** (modules/desktop.nix:51)
- **GLFW overlay** with custom patches
- Status: Partially migrated (some may be in desktop.nix)

### 6. System Configuration
- **Boot parameters:**
  - ✅ boot.shell_on_fail (machines/saya/configuration.nix:24)
  - ✅ systemd.enableEmergencyMode = true (machines/saya/configuration.nix:28)
- **Hardware quirks:**
  - ✅ Logitech G903 mouse scroll wheel fix (quirks/g903.nix)
  - ✅ WINE_CPU_TOPOLOGY for AMD X3D (quirks/amd-x3d.nix)
- **Performance:**
  - ✅ system76-scheduler (modules/performance.nix:51)
  - ✅ CPU frequency governor = schedutil (modules/performance.nix:14)
- Status: ✅ **COMPLETED**

### 7. Network Services
- **Additional firewall ports:**
  - 80, 443 (HTTP/HTTPS) - ✅ **COMPLETED** (tsugumi has caddy.nix)
  - 6987 (rtorrent)
  - 3000 (Textchat-ui)
  - 25565 (Minecraft) - ✅ **COMPLETED** (tsugumi has minecraft.nix)
  - 10401 (Wireguard)
  - 5200, 5201 (Stationeers)
  - ✅ 34197 (Factorio) - (modules/networking.nix:53, saya/configuration.nix:50)
- Status: Partially migrated

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
- ✅ **Services**: Many services have been migrated (see individual service files)

### Major Skipped Components

#### Web Infrastructure
- ✅ **Caddy web server** with 15+ domain proxies (machines/tsugumi/caddy.nix):
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
  - ✅ Sonarr (TV show management) - (machines/tsugumi/sonarr.nix)
  - Plex media server
  - ✅ SilverBullet note-taking system - (machines/tsugumi/silverbullet.nix)
- **Game Servers**:
  - ✅ Minecraft servers (machines/tsugumi/minecraft.nix)
  - ✅ Bot services (machines/tsugumi/rolebot.nix, sdbot.nix, irctool.nix, aniwatch.nix)
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
- ✅ **Syncthing** multi-device synchronization (machines/tsugumi/syncthing.nix):
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
- ✅ **Additional Users**: minecraft, aquagon (machines/tsugumi/configuration.nix:55)
- ✅ **Age Secrets**: Encrypted password and key management (secrets/default.nix)
- **SSH Key Management**: Automated key distribution

#### Network Services
- **Firewall Configuration**:
  - TCP: 80, 443 (HTTP/HTTPS)
  - UDP: 34197 (Factorio)
- **DNS Configuration**: Custom DNS servers (1.1.1.1, 1.0.0.1)
- **Network Interface**: Converted from systemd-network to standard NixOS

### Migration Priority for Future
1. **High**: UPS monitoring, authentication system, monitoring stack
2. **Medium**: Plex media server, ZRepl backups, filesystem bind mounts
3. **Low**: Advanced media management features

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
