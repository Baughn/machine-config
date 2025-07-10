{
  config,
  pkgs,
  lib,
  ...
}: {
  imports = [
    ../secrets
    ./basics.nix
    ./launchable.nix
    ./nginx.nix
    ./resilience.nix
    ./virtualisation.nix
    ./monitoring.nix
    ./nix-serve.nix
    ./exceptions.nix
    ./ipblock.nix
  ];

  me.monitoring.enable = lib.mkDefault true;
  me.monitoring.zfs = lib.mkDefault false;

  # Use nix-index instead of command-not-found, for flake support.
  programs.nix-index.enable = true;
  programs.command-not-found.enable = false;

  # Setup cachix
  nix.settings.substituters = [
    "https://cuda-maintainers.cachix.org"
    "https://cache.flox.dev"
  ];
  nix.settings.trusted-public-keys = [
    "cuda-maintainers.cachix.org-1:0dq3bujKpuEPMCX6U4WylrUDZ9JyUG0VpVZa7CNfq5E="
    "flox-cache-public-1:7F4OyH7ZCnFhcze3fJdfyXYLQw/aV7GEed86nQ7IsOs="
  ];
}
