# CachyOS-inspired system tweaks for NixOS
#
# Derived from CachyOS's default configuration (cachyos-settings, udev rules,
# sysctl tunables, systemd overrides). These are safe, well-tested defaults
# used by CachyOS on tens of thousands of desktop/gaming systems.
#
# The CachyOS kernel itself is provided by xddxdd/nix-cachyos-kernel via flake.nix.

{ config, lib, pkgs, ... }:

{
  # === CachyOS Kernel ========================================================
  # Provides BORE scheduler, sched-ext, BBRv3, NTSync, CAKE qdisc, and more.
  # Binary cache available via xddxdd's attic and garnix CI.
  boot.kernelPackages = pkgs.cachyosKernels.linuxPackages-cachyos-latest;

  # === ZRAM ==================================================================
  # Compressed swap in RAM using zstd. With compression, effective capacity is
  # 2-3x the allocated size. CachyOS sizes ZRAM equal to physical RAM.
  zramSwap = {
    enable = true;
    algorithm = "zstd";
    memoryPercent = 100;
  };

  # === Boot parameters =======================================================
  boot.kernelParams = [ "nowatchdog" ];

  # Disable hardware watchdog modules (Intel iTCO, AMD sp5100)
  boot.blacklistedKernelModules = [ "iTCO_wdt" "sp5100_tco" ];

  # === Sysctl tunables =======================================================
  boot.kernel.sysctl = {
    # -- Memory management --
    # High swappiness is correct with ZRAM: decompressing from RAM is far
    # cheaper than dropping file caches and re-reading from disk.
    "vm.swappiness" = 150;
    "vm.vfs_cache_pressure" = 50;
    "vm.dirty_bytes" = 268435456;           # 256 MB
    "vm.dirty_background_bytes" = 67108864; # 64 MB
    "vm.dirty_writeback_centisecs" = 1500;
    "vm.page-cluster" = 0; # No read-ahead for ZRAM (no seek penalty)

    # -- Kernel --
    "kernel.nmi_watchdog" = 0;
    "kernel.printk" = "3 3 3 3";
    "kernel.kptr_restrict" = 2;
    "kernel.sysrq" = 128; # Allow only sync (emergency filesystem sync)
    "kernel.split_lock_mitigate" = 0;

    # -- File descriptors --
    "fs.file-max" = 2097152;

    # -- Network --
    "net.core.netdev_max_backlog" = 4096;
    "net.ipv4.tcp_congestion_control" = "bbr";
    "net.core.default_qdisc" = "cake"; # Available with CachyOS kernel
    "net.ipv4.tcp_fastopen" = 3;       # Client + server
    "net.core.rmem_max" = 16777216;    # 16 MB
    "net.core.wmem_max" = 16777216;    # 16 MB
    "net.core.somaxconn" = 8192;
    "net.ipv4.tcp_keepalive_time" = 60;
    "net.ipv4.tcp_keepalive_intvl" = 10;
    "net.ipv4.tcp_mtu_probing" = 1;
  };

  # === Transparent Huge Pages ================================================
  boot.kernel.sysfs = {
    kernel.mm.transparent_hugepage = {
      enabled = "always";
      defrag = "defer+madvise";
    };
  };

  # === I/O scheduler udev rules ==============================================
  services.udev.extraRules = ''
    # NVMe: no scheduler (hardware handles it)
    ACTION=="add|change", KERNEL=="nvme[0-9]*", ATTR{queue/scheduler}="none"
    # SSD (non-rotational): mq-deadline
    ACTION=="add|change", KERNEL=="sd[a-z]*|mmcblk[0-9]*", ATTR{queue/rotational}=="0", ATTR{queue/scheduler}="mq-deadline"
    # HDD (rotational): bfq
    ACTION=="add|change", KERNEL=="sd[a-z]*", ATTR{queue/rotational}=="1", ATTR{queue/scheduler}="bfq"
  '';

  # === Systemd ===============================================================
  systemd.settings.Manager = {
    DefaultTimeoutStartSec = "15s";
    DefaultTimeoutStopSec = "10s";
    DefaultLimitNOFILE = "2048:2097152";
  };

  services.journald.extraConfig = ''
    SystemMaxUse=50M
  '';

  # === NVIDIA module parameters ==============================================
  boot.extraModprobeConfig = ''
    options nvidia NVreg_UsePageAttributeTable=1
    options nvidia NVreg_InitializeSystemMemoryAllocations=0
    options nvidia NVreg_DynamicPowerManagement=0x02
  '';

  # === fstrim ================================================================
  services.fstrim.enable = true;

  # === Audio real-time priority ==============================================
  security.pam.loginLimits = [
    { domain = "@audio"; type = "-"; item = "rtprio";  value = "95"; }
    { domain = "@audio"; type = "-"; item = "memlock"; value = "unlimited"; }
    { domain = "@audio"; type = "-"; item = "nice";    value = "-19"; }
  ];

  users.users.svein.extraGroups = [ "audio" ];

  # === Optional: ananicy-cpp (automatic process priority management) =========
  # Uncomment to enable. Adjusts nice, ionice, scheduling class, OOM score
  # per-process based on CachyOS community rules.
  #
  services.ananicy = {
    enable = true;
    package = pkgs.ananicy-cpp;
    rulesProvider = pkgs.ananicy-cpp;
  };

  # === Optional: KSM (Kernel Same-page Merging) ==============================
  # Deduplicates identical memory pages. Useful for VMs and similar workloads.
  # Trades CPU for memory savings.
  #
  # hardware.ksm.enable = true;

  # === Optional: sched-ext (BPF CPU schedulers) ==============================
  # The CachyOS kernel includes sched-ext support. Install scx-scheds and run
  # scx_bpfland for improved desktop responsiveness under load:
  #
  # environment.systemPackages = [ pkgs.scx-scheds ];
  # Then run: sudo scx_bpfland
  # Or use scx_loader for D-Bus-managed scheduler switching.
}
