{ config, lib, pkgs, ... }:

{
  # VM tweaks for better responsiveness
  boot.kernel.sysctl = {
    "vm.swappiness" = 10; # Prefer zram, avoid SSD wear
    "vm.dirty_background_ratio" = 5; # Write-back latency optimization
    "vm.dirty_ratio" = 20;
  };

  # Memory management with zram
  zramSwap = {
    enable = true;
    algorithm = "zstd";
    memoryPercent = 75;
    priority = 100;
  };

  # Services configuration
  services = {
    # Kill applications on OOM... prior to the desktop locking up.
    earlyoom = {
      enable = true;
      enableNotifications = true;
      freeMemThreshold = 20;
      extraArgs = [
        # Avoid killing important system and desktop processes
        "--avoid"
        "^(systemd|kernel|init|dbus|NetworkManager|pipewire|wireplumber|pulse)$"
        "--avoid"
        "^(gnome-shell|plasmashell|kwin|xorg|wayland|sway|hyprland)$"
        "--avoid"
        "^(gdm|sddm|lightdm|greetd)$"
        # Prefer killing these types of processes first
        "--prefer"
        "^(chrome|chromium|firefox|electron)$"
        "--prefer"
        "^(java|node|python|ruby)$"
      ];
    };

    # Disk schedulers optimized for different storage types
    udev.extraRules = ''
      # Set the 'kyber' I/O scheduler for NVMe SSDs. This is optimized for the
      # low latency and high parallelism of modern NVMe drives.
      ACTION=="add|change", KERNEL=="nvme?n?", ATTR{queue/rotational}=="0", ATTR{queue/scheduler}="kyber"

      # Set the 'bfq' I/O scheduler for SATA SSDs and rotational HDDs.
      # This scheduler is optimized for desktop responsiveness on these device types.
      ACTION=="add|change", KERNEL=="sd*", ATTR{queue/rotational}=="0", ATTR{queue/scheduler}="bfq"
      ACTION=="add|change", KERNEL=="sd*", ATTR{queue/rotational}=="1", ATTR{queue/scheduler}="bfq"
    '';
  };
}
