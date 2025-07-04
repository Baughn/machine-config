# Do not modify this file!  It was generated by 'nixos-generate-config'
# and may be overwritten by future invocations.  Please make changes
# to /etc/nixos/configuration.nix instead.
{ config
, lib
, pkgs
, modulesPath
, ...
}: {
  imports = [
    (modulesPath + "/installer/scan/not-detected.nix")
  ];

  boot.initrd.availableKernelModules = [ "xhci_pci" "ahci" "nvme" "sd_mod" ];
  boot.initrd.kernelModules = [ ];
  boot.kernelModules = [ "kvm-amd" ];
  boot.extraModulePackages = [ ];

  fileSystems."/" = {
    device = "rpool/root";
    fsType = "zfs";
  };

  fileSystems."/etc" = {
    device = "rpool/root/etc";
    fsType = "zfs";
  };

  fileSystems."/nix" = {
    device = "rpool/root/nix";
    fsType = "zfs";
  };

  fileSystems."/nix/store" = {
    device = "rpool/root/nix/store";
    fsType = "zfs";
  };

  fileSystems."/var" = {
    device = "rpool/root/var";
    fsType = "zfs";
  };

  fileSystems."/home" = {
    device = "rpool/home";
    fsType = "zfs";
  };

  fileSystems."/home/svein" = {
    device = "rpool/home/svein";
    fsType = "zfs";
  };

  fileSystems."/home/svein/dev" = {
    device = "rpool/home/svein/dev";
    fsType = "zfs";
  };

  fileSystems."/home/svein/factorio" = {
    device = "rpool/home/svein/factorio";
    fsType = "zfs";
  };

  fileSystems."/boot" = {
    device = "/dev/disk/by-uuid/445F-2C55";
    fsType = "vfat";
  };

  fileSystems."/home/sh" = {
    device = "stash/encrypted/bulk/sh";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/aquagon" = {
    device = "stash/encrypted/backed-up/aquagon";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/web" = {
    device = "stash/encrypted/bulk/web";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/minecraft" = {
    device = "stash/minecraft";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/minecraft/testing" = {
    device = "rpool/minecraft/testing";
    fsType = "zfs";
  };

  fileSystems."/home/minecraft/erisia" = {
    device = "rpool/minecraft/erisia";
    fsType = "zfs";
  };

  fileSystems."/home/minecraft/incognito" = {
    device = "rpool/minecraft/incognito";
    fsType = "zfs";
  };

  fileSystems."/home/minecraft/testing/dynmap" = {
    device = "rpool/minecraft/testing/dynmap";
    fsType = "zfs";
  };

  fileSystems."/home/minecraft/erisia/dynmap" = {
    device = "rpool/minecraft/erisia/dynmap";
    fsType = "zfs";
  };

  fileSystems."/home/minecraft/incognito/dynmap" = {
    device = "rpool/minecraft/incognito/dynmap";
    fsType = "zfs";
  };

  fileSystems."/home/svein/Games" = {
    device = "stash/encrypted/bulk/Games";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/Media" = {
    device = "stash/encrypted/bulk/Media";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/Sync" = {
    device = "stash/encrypted/backed-up/Sync";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/dcc" = {
    device = "stash/encrypted/bulk/incoming";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/secure" = {
    device = "stash/encrypted/backed-up/core";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/short-term" = {
    device = "stash/encrypted/short-term";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/Sync/Watched" = {
    device = "stash/encrypted/short-term/Syncwatch";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  fileSystems."/home/svein/win" = {
    device = "stash/encrypted/bulk/win";
    fsType = "zfs";
    depends = [ "/key" ];
  };

  swapDevices = [
    { device = "/dev/disk/by-uuid/cd7e6f53-0a73-42dc-987e-7dc3e751ebfc"; }
  ];
}
