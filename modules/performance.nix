{ config, lib, pkgs, ... }:

{
  # AIDEV-NOTE: Performance optimizations for AMD 7950X3D + RTX 4090 gaming system

  # Use zen kernel for better gaming performance
  boot.kernelPackages = pkgs.linuxPackages_zen;

  # CPU optimizations for 7950X3D
  boot.kernelParams = [
    "preempt=full" # Minimize latency
    "threadirqs"
    "amd_pstate=active" # Use CPPC-based driver for faster response
    "amd_prefcore=1" # Prefer V-Cache CCD for latency-sensitive threads
  ];

  # Memory management with zram
  zramSwap = {
    enable = true;
    algorithm = "zstd";
    memoryPercent = 50; # 50% of RAM for compressed swap
    priority = 100;
  };

  # Disk schedulers optimized for different storage types
  services.udev.extraRules = ''
    # Set the 'kyber' I/O scheduler for NVMe SSDs. This is optimized for the
    # low latency and high parallelism of modern NVMe drives.
    ACTION=="add|change", KERNEL=="nvme?n?", ATTR{queue/rotational}=="0", ATTR{queue/scheduler}="kyber"

    # Set the 'bfq' I/O scheduler for SATA SSDs and rotational HDDs.
    # This scheduler is optimized for desktop responsiveness on these device types.
    ACTION=="add|change", KERNEL=="sd*", ATTR{queue/rotational}=="0", ATTR{queue/scheduler}="bfq"
    ACTION=="add|change", KERNEL=="sd*", ATTR{queue/rotational}=="1", ATTR{queue/scheduler}="bfq"
  '';

  # VM tweaks for better responsiveness
  boot.kernel.sysctl = {
    "vm.swappiness" = 10; # Prefer zram, avoid SSD wear
    "vm.dirty_background_ratio" = 5; # Write-back latency optimization
    "vm.dirty_ratio" = 20;
  };

  # GameMode for per-game performance governor switching
  programs.gamemode.enable = true;
  powerManagement.cpuFreqGovernor = "performance";
}
