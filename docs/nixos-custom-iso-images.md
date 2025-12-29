# NixOS Custom Installation Images Guide

This guide provides comprehensive documentation for creating custom NixOS installation images (ISOs), from basic configurations to advanced customizations.

## Table of Contents

1. [Overview](#overview)
2. [Basic ISO Configuration](#basic-iso-configuration)
3. [Customizing the Installer Environment](#customizing-the-installer-environment)
4. [Adding Custom Installation Scripts](#adding-custom-installation-scripts)
5. [Hardware-Specific Configurations](#hardware-specific-configurations)
6. [Network Configuration](#network-configuration)
7. [Creating Specialized ISOs](#creating-specialized-isos)
8. [Building and Testing ISOs](#building-and-testing-isos)
9. [Advanced Customizations](#advanced-customizations)
10. [Flakes-Based ISO Generation](#flakes-based-iso-generation)
11. [Common Pitfalls and Troubleshooting](#common-pitfalls-and-troubleshooting)

## Overview

NixOS ISO generation creates bootable installation media with customizable configurations. The process leverages NixOS's declarative configuration system to build reproducible installation images.

### Key Components

- **iso-image.nix**: Core module that handles ISO generation
- **Installation profiles**: Pre-configured bases (minimal, graphical, etc.)
- **Boot loaders**: Support for both BIOS (syslinux) and UEFI (GRUB2)
- **Squashfs**: Compressed filesystem containing the Nix store

### Use Cases

- Custom installation media with pre-configured settings
- Rescue/recovery disks with specific tools
- Live systems for demonstrations or testing
- Deployment images with organization-specific configurations
- Hardware-specific installers with proprietary drivers

## Basic ISO Configuration

### Minimal Example

The simplest custom ISO extends the minimal installer:

```nix
# custom-iso-minimal.nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Custom system packages
  environment.systemPackages = with pkgs; [
    vim
    git
    htop
  ];

  # Set a custom hostname
  networking.hostName = "nixos-installer";
}
```

### Building the ISO

```bash
# Clone nixpkgs
git clone https://github.com/NixOS/nixpkgs.git
cd nixpkgs/nixos

# Build the ISO
nix-build -A config.system.build.isoImage \
  -I nixos-config=./custom-iso-minimal.nix \
  default.nix
```

The resulting ISO will be at `./result/iso/`.

## Customizing the Installer Environment

### Adding Packages

Include additional packages in the installation environment:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # System administration tools
  environment.systemPackages = with pkgs; [
    # Editors
    vim
    neovim
    emacs
    
    # Network tools
    wget
    curl
    nmap
    tcpdump
    
    # System tools
    htop
    iotop
    lsof
    strace
    
    # File systems
    ntfs3g
    exfatprogs
    btrfs-progs
    
    # Hardware tools
    pciutils
    usbutils
    dmidecode
  ];
}
```

### Configuring Services

Enable and configure services in the installer:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Enable SSH for remote installation
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "yes";
      PasswordAuthentication = true;
    };
  };

  # Start SSH automatically
  systemd.services.sshd.wantedBy = [ "multi-user.target" ];

  # Enable NetworkManager for easier network configuration
  networking.networkmanager.enable = true;

  # Enable ntp for time synchronization
  services.chrony.enable = true;
}
```

### User Configuration

Configure users with pre-set passwords:

```nix
{ config, pkgs, lib, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Create a custom user
  users.users.installer = {
    isNormalUser = true;
    extraGroups = [ "wheel" "networkmanager" ];
    # Password: installer (hashed)
    hashedPassword = "$6$rounds=50000$7n5hoAW$1nA3Gs5XL5.L3SWXqNnfQrTkXkuuXkY5rlhLNJMvGXz3u5kCPLqWqIBLVWpJPl8XZY8RP/duw7mEYCuE5U8hl0";
  };

  # Enable sudo without password for wheel group
  security.sudo.wheelNeedsPassword = false;

  # Set root password (password: root)
  users.users.root.hashedPassword = "$6$rounds=50000$uiRKuG$bHxfQPT6Y8w2VYGJKdKVJy1m5HyWDhZPHfF0YXmyNcL5XFn9oGCJLGMrwMb7qgJx6h/9Kz1dw1Tt8wXqvTt7C1";
}
```

## Adding Custom Installation Scripts

### Automated Installation Script

Create an automated installation helper:

```nix
{ config, pkgs, ... }:

let
  autoInstallScript = pkgs.writeScriptBin "auto-install" ''
    #!${pkgs.bash}/bin/bash
    set -e

    echo "=== NixOS Automated Installation ==="
    
    # Detect disks
    echo "Available disks:"
    lsblk -d -o NAME,SIZE,TYPE | grep disk
    
    read -p "Enter target disk (e.g., sda): " DISK
    
    # Partition disk
    echo "Partitioning /dev/$DISK..."
    parted /dev/$DISK -- mklabel gpt
    parted /dev/$DISK -- mkpart ESP fat32 1MiB 512MiB
    parted /dev/$DISK -- set 1 esp on
    parted /dev/$DISK -- mkpart primary 512MiB 100%
    
    # Format partitions
    echo "Formatting partitions..."
    mkfs.fat -F32 /dev/''${DISK}1
    mkfs.ext4 /dev/''${DISK}2
    
    # Mount partitions
    echo "Mounting partitions..."
    mount /dev/''${DISK}2 /mnt
    mkdir -p /mnt/boot
    mount /dev/''${DISK}1 /mnt/boot
    
    # Generate configuration
    echo "Generating configuration..."
    nixos-generate-config --root /mnt
    
    # Install
    echo "Installing NixOS..."
    nixos-install --no-root-passwd
    
    echo "Installation complete! Reboot to start using NixOS."
  '';
in
{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  environment.systemPackages = [ autoInstallScript ];
}
```

### Configuration Template Installer

Include pre-made configuration templates:

```nix
{ config, pkgs, ... }:

let
  configTemplates = pkgs.stdenv.mkDerivation {
    name = "nixos-config-templates";
    src = ./templates;
    installPhase = ''
      mkdir -p $out/share/nixos-templates
      cp -r * $out/share/nixos-templates/
    '';
  };

  templateInstaller = pkgs.writeScriptBin "install-template" ''
    #!${pkgs.bash}/bin/bash
    
    TEMPLATE_DIR="${configTemplates}/share/nixos-templates"
    
    echo "Available templates:"
    ls "$TEMPLATE_DIR"
    
    read -p "Select template: " TEMPLATE
    
    if [ -f "$TEMPLATE_DIR/$TEMPLATE/configuration.nix" ]; then
      cp -r "$TEMPLATE_DIR/$TEMPLATE"/* /mnt/etc/nixos/
      echo "Template $TEMPLATE installed to /mnt/etc/nixos/"
    else
      echo "Template not found!"
    fi
  '';
in
{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  environment.systemPackages = [ templateInstaller ];
}
```

## Hardware-Specific Configurations

### Adding Proprietary Drivers

For systems requiring proprietary drivers:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-graphical-gnome.nix>
  ];

  # Allow unfree packages
  nixpkgs.config.allowUnfree = true;

  # NVIDIA drivers
  services.xserver.videoDrivers = [ "nvidia" ];
  hardware.nvidia = {
    modesetting.enable = true;
    package = config.boot.kernelPackages.nvidiaPackages.stable;
  };

  # Broadcom WiFi for MacBooks
  boot.initrd.kernelModules = [ "wl" ];
  boot.kernelModules = [ "kvm-intel" "wl" ];
  boot.extraModulePackages = [ config.boot.kernelPackages.broadcom_sta ];

  # Additional firmware
  hardware.enableAllFirmware = true;
  hardware.enableRedistributableFirmware = true;
}
```

### Custom Kernel Configuration

Use a specific kernel version or configuration:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Use latest kernel
  boot.kernelPackages = pkgs.linuxPackages_latest;

  # Or use a specific kernel
  # boot.kernelPackages = pkgs.linuxPackages_6_6;

  # Custom kernel parameters
  boot.kernelParams = [
    "nomodeset"  # Disable kernel mode setting
    "intel_pstate=disable"  # Disable Intel P-state driver
    "acpi_osi=Linux"  # ACPI compatibility
  ];

  # Additional kernel modules
  boot.initrd.availableKernelModules = [
    "ahci"
    "xhci_pci"
    "virtio_pci"
    "sr_mod"
    "virtio_blk"
  ];
}
```

## Network Configuration

### Static Network Configuration

Configure static networking for environments without DHCP:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Disable NetworkManager
  networking.networkmanager.enable = false;

  # Configure static IP
  networking.interfaces.eth0 = {
    useDHCP = false;
    ipv4.addresses = [{
      address = "192.168.1.100";
      prefixLength = 24;
    }];
  };

  networking.defaultGateway = "192.168.1.1";
  networking.nameservers = [ "8.8.8.8" "8.8.4.4" ];

  # Enable SSH with specific configuration
  services.openssh = {
    enable = true;
    ports = [ 22 ];
    settings = {
      PermitRootLogin = "yes";
      PasswordAuthentication = false;
    };
  };

  # Add SSH keys
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC..."
  ];
}
```

### WiFi Configuration

Pre-configure WiFi networks:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Enable NetworkManager
  networking.networkmanager.enable = true;

  # Pre-configure WiFi networks
  networking.wireless = {
    enable = true;
    networks = {
      "HomeNetwork" = {
        psk = "secretpassword";
      };
      "WorkNetwork" = {
        auth = ''
          key_mgmt=WPA-EAP
          eap=PEAP
          identity="user@company.com"
          password="password"
        '';
      };
    };
  };

  # Include wireless tools
  environment.systemPackages = with pkgs; [
    wirelesstools
    wpa_supplicant
    iw
  ];
}
```

## Creating Specialized ISOs

### Rescue Disk

A comprehensive rescue/recovery ISO:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Rescue tools
  environment.systemPackages = with pkgs; [
    # Disk recovery
    testdisk
    photorec
    ddrescue
    safecopy
    
    # Partition management
    gparted
    gnome.gnome-disk-utility
    
    # File system tools
    e2fsprogs
    xfsprogs
    btrfs-progs
    ntfs3g
    dosfstools
    
    # Network tools
    nmap
    tcpdump
    wireshark-cli
    netcat
    socat
    
    # System analysis
    htop
    iotop
    sysstat
    lsof
    strace
    
    # Data recovery
    foremost
    scalpel
    
    # Backup tools
    rsync
    rclone
    restic
  ];

  # Enable useful services
  services.gpm.enable = true;  # Mouse in console
  
  # Boot options for problematic hardware
  boot.kernelParams = [
    "nomodeset"
    "noapic"
    "noacpi"
  ];
}
```

### Live Desktop System

A full desktop environment on a live ISO:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-graphical-gnome.nix>
  ];

  # Additional desktop software
  environment.systemPackages = with pkgs; [
    firefox
    chromium
    libreoffice
    gimp
    vlc
    vscode
    
    # Development tools
    git
    docker
    nodejs
    python3
    gcc
  ];

  # Enable Docker
  virtualisation.docker.enable = true;
  
  # Persistence for live system
  # Files in /home/nixos/persistent will survive reboots
  fileSystems."/home/nixos/persistent" = {
    device = "/dev/disk/by-label/PERSISTENCE";
    fsType = "ext4";
    options = [ "defaults" ];
    autoMount = true;
  };

  # Auto-login
  services.displayManager.autoLogin = {
    enable = true;
    user = "nixos";
  };
}
```

### Deployment Image

An ISO for automated deployment:

```nix
{ config, pkgs, ... }:

let
  deploymentConfig = pkgs.writeText "deployment.nix" ''
    { config, pkgs, ... }:
    {
      imports = [ ./hardware-configuration.nix ];
      
      boot.loader.systemd-boot.enable = true;
      boot.loader.efi.canTouchEfiVariables = true;
      
      networking.hostName = "deployed-system";
      
      services.openssh.enable = true;
      
      users.users.admin = {
        isNormalUser = true;
        extraGroups = [ "wheel" ];
        openssh.authorizedKeys.keys = [
          "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC..."
        ];
      };
      
      system.stateVersion = "24.05";
    }
  '';

  deployScript = pkgs.writeScriptBin "deploy-system" ''
    #!${pkgs.bash}/bin/bash
    set -e
    
    echo "=== Automated NixOS Deployment ==="
    
    # Auto-detect primary disk
    DISK=$(lsblk -d -o NAME,SIZE,TYPE | grep disk | head -1 | awk '{print $1}')
    
    echo "Deploying to /dev/$DISK"
    
    # Automated partitioning and formatting
    parted /dev/$DISK -- mklabel gpt
    parted /dev/$DISK -- mkpart ESP fat32 1MiB 512MiB
    parted /dev/$DISK -- set 1 esp on
    parted /dev/$DISK -- mkpart primary 512MiB 100%
    
    mkfs.fat -F32 /dev/''${DISK}1
    mkfs.ext4 /dev/''${DISK}2
    
    mount /dev/''${DISK}2 /mnt
    mkdir -p /mnt/boot
    mount /dev/''${DISK}1 /mnt/boot
    
    # Generate hardware config
    nixos-generate-config --root /mnt
    
    # Copy deployment config
    cp ${deploymentConfig} /mnt/etc/nixos/configuration.nix
    
    # Install
    nixos-install --no-root-passwd
    
    reboot
  '';
in
{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  environment.systemPackages = [ deployScript ];
  
  # Auto-run deployment on boot
  systemd.services.auto-deploy = {
    description = "Automatic NixOS deployment";
    after = [ "multi-user.target" ];
    wantedBy = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      ExecStart = "${deployScript}/bin/deploy-system";
      StandardInput = "tty";
      StandardOutput = "tty";
      StandardError = "tty";
      TTYPath = "/dev/tty1";
    };
  };
}
```

## Building and Testing ISOs

### Building with Specific Options

```bash
# Build with custom store paths included
nix-build -A config.system.build.isoImage \
  -I nixos-config=./custom-iso.nix \
  default.nix \
  --arg config '{ isoImage.includeSystemBuildDependencies = true; }'

# Build with compression disabled (faster builds, larger ISO)
nix-build -A config.system.build.isoImage \
  -I nixos-config=./custom-iso.nix \
  default.nix \
  --arg config '{ isoImage.squashfsCompression = null; }'
```

### Testing with QEMU

Test the ISO in a virtual machine:

```bash
# Simple test
qemu-system-x86_64 \
  -cdrom result/iso/nixos-*.iso \
  -boot d \
  -m 2048

# UEFI boot test
qemu-system-x86_64 \
  -cdrom result/iso/nixos-*.iso \
  -boot d \
  -m 2048 \
  -bios ${pkgs.OVMF.fd}/FV/OVMF.fd

# With networking
qemu-system-x86_64 \
  -cdrom result/iso/nixos-*.iso \
  -boot d \
  -m 2048 \
  -netdev user,id=net0,hostfwd=tcp::2222-:22 \
  -device e1000,netdev=net0
```

### Testing with KVM

For better performance with KVM:

```nix
{ config, pkgs, ... }:

let
  testIsoScript = pkgs.writeScriptBin "test-iso" ''
    #!${pkgs.bash}/bin/bash
    
    ISO="$1"
    if [ -z "$ISO" ]; then
      ISO=$(find result/iso -name "*.iso" | head -1)
    fi
    
    if [ ! -f "$ISO" ]; then
      echo "ISO not found: $ISO"
      exit 1
    fi
    
    echo "Testing ISO: $ISO"
    
    ${pkgs.qemu}/bin/qemu-system-x86_64 \
      -enable-kvm \
      -cdrom "$ISO" \
      -boot d \
      -m 4096 \
      -cpu host \
      -smp 2 \
      -vga virtio \
      -display gtk \
      -netdev user,id=net0,hostfwd=tcp::2222-:22 \
      -device virtio-net,netdev=net0
  '';
in
{
  environment.systemPackages = [ testIsoScript ];
}
```

## Advanced Customizations

### Custom Boot Menu

Customize the boot menu appearance and options:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Custom boot menu labels
  isoImage.prependToMenuLabel = "MyOrg ";
  isoImage.appendToMenuLabel = " Custom Installer";

  # Custom GRUB theme
  isoImage.grubTheme = pkgs.fetchFromGitHub {
    owner = "myorg";
    repo = "grub-theme";
    rev = "main";
    sha256 = "...";
  };

  # Custom splash images
  isoImage.splashImage = ./splash-bios.png;
  isoImage.efiSplashImage = ./splash-efi.png;

  # Force text mode
  isoImage.forceTextMode = false;

  # Custom volume ID
  isoImage.volumeID = "MYORG-NIXOS-24.05";
}
```

### Including Custom Content

Add custom files to the ISO:

```nix
{ config, pkgs, ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Add files to specific locations on the ISO
  isoImage.contents = [
    {
      source = ./README.txt;
      target = "/README.txt";
    }
    {
      source = ./configs;
      target = "/configs";
    }
    {
      source = pkgs.writeText "install-guide.html" ''
        <html>
          <body>
            <h1>Installation Guide</h1>
            <p>Custom installation instructions...</p>
          </body>
        </html>
      '';
      target = "/install-guide.html";
    }
  ];

  # Include additional packages in the store
  isoImage.storeContents = with pkgs; [
    vim
    git
    # Any derivation that should be available offline
  ];
}
```

### Multi-Architecture Support

Build ISOs for different architectures:

```nix
{ config, pkgs, system ? "x86_64-linux", ... }:

{
  imports = [
    <nixpkgs/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix>
  ];

  # Architecture-specific configuration
  boot = if system == "aarch64-linux" then {
    loader.grub.enable = false;
    loader.generic-extlinux-compatible.enable = true;
  } else {
    loader.grub.enable = true;
  };

  # Include architecture-specific packages
  environment.systemPackages = with pkgs; [
    vim
    git
  ] ++ lib.optionals (system == "x86_64-linux") [
    # x86_64-specific packages
    memtest86plus
  ] ++ lib.optionals (system == "aarch64-linux") [
    # ARM-specific packages
    raspberrypi-tools
  ];
}
```

## Flakes-Based ISO Generation

### Basic Flake Configuration

Create a `flake.nix` for reproducible ISO builds:

```nix
{
  description = "Custom NixOS Installation ISO";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
  };

  outputs = { self, nixpkgs }: {
    nixosConfigurations = {
      custom-iso = nixpkgs.lib.nixosSystem {
        system = "x86_64-linux";
        modules = [
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
          ({ config, pkgs, ... }: {
            # ISO customizations
            environment.systemPackages = with pkgs; [
              vim
              git
              htop
            ];

            # Custom configuration
            networking.hostName = "nixos-installer";
            
            # Enable SSH
            services.openssh.enable = true;
          })
        ];
      };
    };

    # Convenience output for building ISO
    packages.x86_64-linux.default = self.nixosConfigurations.custom-iso.config.system.build.isoImage;
  };
}
```

Build with:
```bash
nix build .#nixosConfigurations.custom-iso.config.system.build.isoImage
```

### Advanced Flake with Multiple ISOs

```nix
{
  description = "Organization NixOS ISOs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, nixpkgs-unstable }: 
  let
    forAllSystems = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" ];
    
    mkISO = { system, pkgs, modules }: pkgs.lib.nixosSystem {
      inherit system;
      modules = modules;
    };
  in
  {
    nixosConfigurations = {
      # Minimal installer
      installer-minimal = mkISO {
        system = "x86_64-linux";
        pkgs = nixpkgs;
        modules = [
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
          ./modules/common.nix
          ./modules/minimal.nix
        ];
      };

      # Graphical installer
      installer-graphical = mkISO {
        system = "x86_64-linux";
        pkgs = nixpkgs;
        modules = [
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-graphical-gnome.nix"
          ./modules/common.nix
          ./modules/graphical.nix
        ];
      };

      # Rescue disk
      rescue-disk = mkISO {
        system = "x86_64-linux";
        pkgs = nixpkgs;
        modules = [
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
          ./modules/common.nix
          ./modules/rescue.nix
        ];
      };

      # Unstable channel ISO
      installer-unstable = mkISO {
        system = "x86_64-linux";
        pkgs = nixpkgs-unstable;
        modules = [
          "${nixpkgs-unstable}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
          ./modules/common.nix
          ./modules/unstable.nix
        ];
      };
    };

    # Convenience builders
    packages = forAllSystems (system: {
      minimal = self.nixosConfigurations.installer-minimal.config.system.build.isoImage;
      graphical = self.nixosConfigurations.installer-graphical.config.system.build.isoImage;
      rescue = self.nixosConfigurations.rescue-disk.config.system.build.isoImage;
      unstable = self.nixosConfigurations.installer-unstable.config.system.build.isoImage;
    });

    # Default package
    defaultPackage = forAllSystems (system: self.packages.${system}.minimal);
  };
}
```

## Common Pitfalls and Troubleshooting

### Build Failures

1. **Out of disk space**: ISO builds require significant space
   ```bash
   # Check available space
   df -h /tmp
   
   # Use different temp directory
   export TMPDIR=/path/to/larger/disk
   ```

2. **Missing kernel modules**: Ensure required modules are included
   ```nix
   boot.initrd.kernelModules = [ "module_name" ];
   boot.initrd.availableKernelModules = [ "module_name" ];
   ```

3. **Unfree packages**: Enable when needed
   ```bash
   export NIXPKGS_ALLOW_UNFREE=1
   ```

### Boot Issues

1. **UEFI not working**: Ensure EFI is enabled
   ```nix
   isoImage.makeEfiBootable = true;
   ```

2. **USB boot fails**: Enable USB boot support
   ```nix
   isoImage.makeUsbBootable = true;
   ```

3. **Graphics issues**: Try text mode
   ```nix
   isoImage.forceTextMode = true;
   # Or add kernel parameter
   boot.kernelParams = [ "nomodeset" ];
   ```

### Network Problems

1. **No network in installer**: Include network tools
   ```nix
   networking.networkmanager.enable = true;
   # or
   networking.wireless.enable = true;
   ```

2. **DNS not working**: Set nameservers
   ```nix
   networking.nameservers = [ "8.8.8.8" "1.1.1.1" ];
   ```

### ISO Size Issues

1. **ISO too large**: Adjust compression
   ```nix
   # Higher compression (slower build, smaller ISO)
   isoImage.squashfsCompression = "zstd -Xcompression-level 22";
   
   # Or use xz for maximum compression
   isoImage.squashfsCompression = "xz -Xdict-size 100%";
   ```

2. **Remove unnecessary packages**: Use minimal profile
   ```nix
   # Disable documentation
   documentation.enable = false;
   documentation.nixos.enable = false;
   ```

### Testing Issues

1. **VM crashes**: Increase memory
   ```bash
   qemu-system-x86_64 -m 4096 ...
   ```

2. **Slow performance**: Enable KVM
   ```bash
   qemu-system-x86_64 -enable-kvm ...
   ```

3. **No console output**: Check serial console
   ```nix
   boot.kernelParams = [ "console=ttyS0,115200" ];
   ```

## References

- [NixOS Manual - Building Images](https://nixos.org/manual/nixos/stable/#sec-building-image)
- [NixOS ISO Modules](https://github.com/NixOS/nixpkgs/tree/master/nixos/modules/installer/cd-dvd)
- [NixOS Wiki - Creating a NixOS live CD](https://nixos.wiki/wiki/Creating_a_NixOS_live_CD)
- [Nixpkgs Repository](https://github.com/NixOS/nixpkgs)