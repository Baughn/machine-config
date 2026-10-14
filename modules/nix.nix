{ flakeSelf, config, lib, pkgs, ... }:

{
  # nix < 2.34.9 lets any local user become root through the daemon
  # (GHSA-jwvx-xq5j-gc56). Take nix from the pinned nixpkgs-master input
  # until our nixpkgs catches up; this falls back to pkgs.nix by itself then.
  nix.package =
    if lib.versionAtLeast pkgs.nix.version "2.34.9" then pkgs.nix
    else flakeSelf.inputs.nixpkgs-master.legacyPackages.${pkgs.stdenv.hostPlatform.system}.nix;

  nix.settings = {
    experimental-features = [ "nix-command" "flakes" ];
    trusted-users = [ "@wheel" ];
  };

  nix.optimise = {
    automatic = true;
    dates = [ "weekly" ];
  };

  nixpkgs.config.allowUnfree = lib.mkForce true;

  nix.channel.enable = false;
  nix.nixPath = [
    "nixpkgs=${flakeSelf.inputs.nixpkgs}"
  ];

  # nix-ld: allows running unpatched ELF binaries by providing a dynamic linker.
  # Security note: enables execution of arbitrary downloaded binaries.
  # Accepted risk for a developer workstation; consider disabling on servers.
  programs.nix-ld.enable = true;
  programs.nix-ld.libraries = [
    # Add any missing dynamic libraries for unpackaged programs here
  ];
}
