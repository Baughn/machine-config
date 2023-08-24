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
    ./wireguard.nix
    ./monitoring.nix
  ];

  me.monitoring.enable = lib.mkDefault true;
  me.monitoring.zfs = lib.mkDefault false;

  # Setup cachix
  nix.binaryCaches = [
    "https://cuda-maintainers.cachix.org"
  ];
  nix.binaryCachePublicKeys = [
    "cuda-maintainers.cachix.org-1:0dq3bujKpuEPMCX6U4WylrUDZ9JyUG0VpVZa7CNfq5E="
  ];
}
