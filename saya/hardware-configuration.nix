# Do not modify this file!  It was generated by ‘nixos-generate-config’
# and may be overwritten by future invocations.  Please make changes
# to /etc/nixos/configuration.nix instead.
{ config, lib, pkgs, modulesPath, ... }:

{
  imports =
    [ (modulesPath + "/installer/scan/not-detected.nix")
    ];

  boot.initrd.availableKernelModules = [ "nvme" "xhci_pci" "ahci" "usbhid" "uas" "sd_mod" ];
  boot.initrd.kernelModules = [ ];
  boot.kernelModules = [ "kvm-amd" ];
  boot.extraModulePackages = [ ];

  fileSystems."/" =
    { device = "/dev/disk/by-uuid/1d9b697e-c76a-4b69-87b0-52f0e923127d";
      fsType = "ext4";
    };

  fileSystems."/home/svein/AI" =
    { device = "bulk/AI";
      fsType = "zfs";
    };

  fileSystems."/tsugumi" = {
    device = "svein@tsugumi.local:";
    fsType = "fuse.sshfs";
    options = [
      "noauto"
      "x-systemd.automount"
      "_netdev"
      "users"
      "idmap=user"
      "IdentityFile=/home/svein/.ssh/id_ed25519"
      "allow_other"
      "default_permissions"
      "uid=1000"
      "gid=100"
      "exec"
      "reconnect"
      "ServerAliveInterval=15"
      "ServerAliveCountMax=3"
    ];
  };

  fileSystems."/srv/web" =
    { device = "/home/svein/web";
      depends = [ "/home" ];
      options = [ "bind" ];
    };

  fileSystems."/boot" =
    { device = "/dev/disk/by-uuid/48DE-02A2";
      fsType = "vfat";
    };

  swapDevices =
    [ { device = "/dev/disk/by-uuid/881b9caa-9c10-4d5e-8f86-33eecaa3fff8"; }
    ];

  # Enables DHCP on each ethernet and wireless interface. In case of scripted networking
  # (the default) this is the recommended approach. When using systemd-networkd it's
  # still possible to use this option, but it's recommended to use it in conjunction
  # with explicit per-interface declarations with `networking.interfaces.<interface>.useDHCP`.
  networking.useDHCP = lib.mkDefault true;
  # networking.interfaces.enp12s0.useDHCP = lib.mkDefault true;
  # networking.interfaces.wlp13s0.useDHCP = lib.mkDefault true;

  nixpkgs.hostPlatform = lib.mkDefault "x86_64-linux";
  hardware.cpu.amd.updateMicrocode = lib.mkDefault config.hardware.enableRedistributableFirmware;
}
