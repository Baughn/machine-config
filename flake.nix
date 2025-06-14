{
  description = "Machine configurations for all my machines";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";

    nix-index-database.url = "github:nix-community/nix-index-database";
    nix-index-database.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, nix-index-database, ... }: {
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
        }
      ];
    };

    packages.x86_64-linux.options = (import (nixpkgs.outPath + "/nixos/release.nix") { }).options;
  };
}
