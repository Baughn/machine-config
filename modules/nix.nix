{ flakeSelf, config, ... }:

{
  nix.settings = {
    experimental-features = [ "nix-command" "flakes" ];
    trusted-users = [ "@wheel" ];
  };

  nix.optimise = {
    automatic = true;
    dates = [ "weekly" ];
  };

  nixpkgs.config.allowUnfree = true;

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
