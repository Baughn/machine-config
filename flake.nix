{
  description = "Machine configurations for all my machines";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    nixpkgs-kernel.url = "github:nixos/nixpkgs?ref=nixos-unstable";

    nix-index-database.url = "github:nix-community/nix-index-database";
    nix-index-database.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, nixpkgs-kernel, nix-index-database, ... }: {
    nixosConfigurations.saya = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./saya/configuration.nix
        nix-index-database.nixosModules.nix-index
        {
          # Setup nix-index
          programs.nix-index-database.comma.enable = true;
          # Propagate nixpkgs
          nix.nixPath = [ "nixpkgs=/etc/nixpkgs" ];
          environment.etc."nixpkgs".source = nixpkgs;
          nix.registry.nixpkgs.flake = nixpkgs;
          # Allow unfree packages
          nixpkgs.config.allowUnfree = true;
          # Overlay to use linuxPackages_zen from nixpkgs-kernel
          nixpkgs.overlays = [
            (final: prev: {
              linuxPackages_zen = (import nixpkgs-kernel {
                system = prev.system;
                config.allowUnfree = true;
              }).linuxPackages_zen;
            })
          ];
        }
      ];
    };

    packages.x86_64-linux.options = (import (nixpkgs.outPath + "/nixos/release.nix") { }).options;

    # AIDEV-NOTE: VM tests for sanity checking configurations
    checks.x86_64-linux.basic-boot = import ./tests/basic-desktop.nix {
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        config.allowUnfree = true;
      };
    };
  };
}
