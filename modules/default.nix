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
    ./caddy.nix
    ./ipblock.nix
  ];

  me.monitoring.enable = lib.mkDefault true;
  me.monitoring.zfs = lib.mkDefault false;

  # Setup cachix
  nix.settings.substituters = [
    "https://cuda-maintainers.cachix.org"
  ];
  nix.settings.trusted-public-keys = [
    "cuda-maintainers.cachix.org-1:0dq3bujKpuEPMCX6U4WylrUDZ9JyUG0VpVZa7CNfq5E="
  ];
}
