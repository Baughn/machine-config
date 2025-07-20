{ config, lib, pkgs, ... }:

{
  # AIDEV-NOTE: Performance optimizations for AMD 7950X3D + RTX 4090 gaming system

  # Use zen kernel for better gaming performance
  boot = {
    kernelPackages = pkgs.linuxPackages_zen;

    # CPU optimizations for 7950X3D
    kernelParams = [
      "preempt=full" # Minimize latency
      "threadirqs"
      "amd_pstate=active" # Use CPPC-based driver for faster response
      "amd_prefcore=1" # Prefer V-Cache CCD for latency-sensitive threads
      "mitigations=off"
    ];
  };

  # Services configuration
  services = {
    # System76 scheduler for better desktop responsiveness
    system76-scheduler.enable = true;
  };

  # GameMode for per-game performance governor switching
  programs.gamemode.enable = true;
}
