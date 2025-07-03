# Advanced NixOS Configuration with AI-Assisted Development

A sophisticated NixOS configuration repository showcasing cutting-edge hardware optimizations, AI-integrated development workflows, and enterprise-grade service management across multiple machines.

## 🚀 What Makes This Configuration Special

This isn't your typical NixOS config. This repository demonstrates advanced patterns and innovative approaches that push the boundaries of what's possible with declarative system management.

### 🎮 Gaming Performance Engineering

**AMD 7950X3D V-Cache Optimization**
- Custom core pinning that routes game threads to V-Cache cores (0-7, 16-23) and parks frequency cores (8-15, 24-31)
- Eliminates inter-CCD latency stutter in gaming workloads
- GameMode integration with automatic core affinity management
- Kernel tuning: `amd_pstate=active`, `amd_prefcore=1`, `mitigations=off`

**Hardware-Specific Fixes**
- Logitech G903 mouse scroll wheel fix via libinput overrides
- 10G networking with Intel 82599 NIC and jumbo frame support (9000 MTU)
- Zen kernel with optimized I/O schedulers (kyber for NVMe, bfq for rotating media)

### 🤖 AI-Integrated Development Workflow

**Claude Code Integration**
- Purpose-built tooling in `tools-for-claude/` for AI-assisted development
- Intelligent NixOS option search with colored output and size limits
- AIDEV anchor comment system (`AIDEV-NOTE:`, `AIDEV-TODO:`, `AIDEV-QUESTION`) for AI context preservation
- Automated package classification between CLI and desktop applications

**Intelligent Automation**
- Smart update system (`update.py`) with fallback strategies and visual diffs
- Pre-commit hooks that enforce formatting and run comprehensive builds
- Automated linting with statix, deadnix, and nixpkgs-fmt integration

### 🏗️ Advanced NixOS Architecture

**Modular Design Patterns**
- Custom `me.*` namespace for all user-defined options
- Hardware abstraction through dedicated `quirks/` directory
- Conditional module loading based on machine capabilities
- JSON-based application management with automatic categorization

**Enterprise Secrets Management**
- Agenix integration with host-specific secret decryption
- Multi-machine key distribution with user and system key support
- Secrets filtered by hostname to minimize attack surface

**Modern Version Control**
- Jujutsu (jj) instead of Git for superior conflict resolution
- Conventional Commits enforcement with proper scoping
- Colocated Git repository for tool compatibility

### 🛠️ Custom Service Ecosystem

**Rust-Based Microservices**
- **v4proxy**: IPv4-to-IPv6 proxy for Minecraft servers with comprehensive testing
- **rolebot**: Discord role automation based on user activity patterns
- **irc-tool**: IRC notification system with webhook integration for media management
- **aniwatch**: Automated anime file synchronization with cleanup scheduling

**Production-Grade Service Management**
- Systemd hardening with DynamicUser and capability restrictions
- Timer-based automation for maintenance tasks
- Multi-protocol support (IRC, Discord, HTTP webhooks, TCP proxying)

### 🌐 Network Infrastructure

**High-Performance Networking**
- 10 Gigabit Ethernet with custom udev rules for consistent interface naming
- IPv6-ready configuration with proper routing and jumbo frame support
- Service discovery via mDNS and LLMNR for seamless local network integration

**Multi-Machine Deployment**
- Colmena-based deployment with local and remote building capabilities
- Machine-specific profiles: gaming desktop, media server, IPv4 proxy
- Centralized configuration with distributed secret management

## 🏛️ Architecture Overview

```
├── machines/           # Machine-specific configurations
│   ├── saya/          # Gaming desktop (AMD 7950X3D + RTX 4090)
│   ├── tsugumi/       # Media server (ZFS + various services)
│   └── v4/            # IPv4 proxy server
├── modules/           # Shared NixOS modules with me.* namespace
├── quirks/            # Hardware-specific workarounds
├── tools/             # Custom Rust applications
├── tools-for-claude/  # AI development assistance
├── secrets/           # Agenix-encrypted secrets
└── tests/             # VM-based configuration validation
```

## 💡 Innovation Highlights

### Development Experience
- **Intelligent Updates**: `update.py` with rollback capabilities and visual system diffs
- **AI Tooling**: Purpose-built scripts for AI-assisted configuration management
- **Comprehensive Testing**: VM tests, flake checks, and pre-commit validation
- **Modern VCS**: Jujutsu integration for superior history management

### Performance Engineering
- **CPU Optimization**: V-Cache aware scheduling for AMD X3D processors
- **Network Tuning**: Jumbo frames and optimized drivers for high-throughput workloads
- **I/O Optimization**: Scheduler selection based on storage technology
- **Memory Management**: zram with zstd compression for better memory utilization

### Service Architecture
- **Microservice Design**: Purpose-built Rust applications for specific automation tasks
- **Security-First**: Systemd hardening with minimal privileges and capability restrictions
- **Observability**: Structured logging and monitoring integration
- **Automation**: Timer-based maintenance and cleanup operations

## 🚦 Getting Started

### Prerequisites
- NixOS with flakes enabled
- Jujutsu (jj) for version control
- Colmena for deployment (automatically installed)

### Quick Start
```bash
# Clone and navigate
git clone <repository-url>
cd nixos-config

# Run intelligent update
./update.py

# Or deploy manually
colmena apply --on <machine-name>
```

### Development Workflow
```bash
# Search NixOS options (AI-optimized)
./tools-for-claude/search-options.sh search networking

# Add packages with auto-classification
./add-package.sh neovim

# Lint and format before committing
./tools-for-claude/lint.sh

# Commit with Jujutsu
jj commit -m "feat(desktop): Add development tools"
```

## 🔍 Learning Opportunities

This configuration serves as a reference implementation for:

- **Hardware Optimization**: Real-world gaming performance tuning
- **AI Integration**: Practical AI-assisted development workflows  
- **Advanced NixOS**: Enterprise patterns and best practices
- **Service Architecture**: Microservice design with Rust and systemd
- **Deployment Automation**: Multi-machine configuration management
- **Security**: Secrets management and service hardening

## 📋 Migration Status

This repository is actively evolving from a previous configuration. See `MIGRATION_PLANS.md` for detailed tracking of features being migrated, including priority levels and implementation notes.

## 🤝 Contributing

This configuration is designed for learning and adaptation. The AI development infrastructure makes it particularly suitable for AI-assisted modifications and improvements.

### Key Files
- `CLAUDE.md`: Comprehensive guide for AI-assisted development
- `flake.nix`: Main system definitions and deployment configuration
- `modules/default.nix`: Core module aggregation point
- `update.py`: Intelligent update automation

### Development Tools
- `tools-for-claude/search-options.sh`: NixOS option discovery
- `tools-for-claude/lint.sh`: Code quality enforcement
- `add-package.sh`: Intelligent package management

## 🏷️ License

This configuration demonstrates advanced NixOS patterns and is shared for educational purposes. Adapt and modify as needed for your own systems.