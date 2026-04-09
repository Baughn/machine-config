{ pkgs, ... }:

{
  nix.settings = {
    experimental-features = [ "nix-command" "flakes" ];
  };

  nixpkgs.config.allowUnfree = true;

  # nix-ld: allows running unpatched ELF binaries by providing a dynamic linker.
  # Security note: enables execution of arbitrary downloaded binaries.
  # Accepted risk for a developer workstation; consider disabling on servers.
  programs.nix-ld.enable = true;
  programs.nix-ld.libraries = [
    # Add any missing dynamic libraries for unpackaged programs here
  ];
}
