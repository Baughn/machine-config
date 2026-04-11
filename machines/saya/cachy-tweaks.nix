# CachyOS-inspired system tweaks for NixOS
#
# Derived from CachyOS's default configuration (cachyos-settings, udev rules,
# sysctl tunables, systemd overrides). These are safe, well-tested defaults
# used by CachyOS on tens of thousands of desktop/gaming systems.
#
# The CachyOS kernel itself is provided by xddxdd/nix-cachyos-kernel via flake.nix.

{ config, lib, pkgs, ... }:

{
  imports = [
    # Use the Zen 4 optimized CachyOS kernel variant
    ./znver4.nix
  ];

  # === Boot parameters =======================================================
  boot.kernelParams = [
    "nowatchdog"
    "zswap.enabled=1" # enables zswap
    "zswap.compressor=zstd" # compression algorithm
    "zswap.max_pool_percent=20" # maximum percentage of RAM that zswap is allowed to use
    "zswap.shrinker_enabled=1" # whether to shrink the pool proactively on high memory pressure
  ];

  # Disable hardware watchdog modules (Intel iTCO, AMD sp5100)
  # And amdgpu, since we have a dGPU and its presence breaks CP2099
  boot.blacklistedKernelModules = [ "iTCO_wdt" "sp5100_tco" "amdgpu" ];

  # === Sysctl tunables =======================================================
  boot.kernel.sysctl = {
    # -- Memory management --
    "vm.swappiness" = 100;
    "vm.vfs_cache_pressure" = 50;
    "vm.dirty_bytes" = 268435456;           # 256 MB
    "vm.dirty_background_bytes" = 67108864; # 64 MB
    "vm.dirty_writeback_centisecs" = 1500;

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
  services.scx = {
    enable = true;
    scheduler = "scx_bpfland";
  };
}
