{
  description = "Machine configs";

  inputs.nixpkgs.url = "flake:nixpkgs/nixos-unstable";
  inputs.nixos-hardware.url = "flake:nixos-hardware";
  inputs.home-manager.url = "github:nix-community/home-manager";
  inputs.openwrt = {
    url = "path:../openwrt";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, openwrt, nixos-hardware, home-manager }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      devShell.${system} = import ./shell.nix { inherit pkgs; };
      nixosConfigurations.tsugumi = nixpkgs.lib.nixosSystem {
        inherit system;

        modules = [
          nixos-hardware.nixosModules.common-pc
          nixos-hardware.nixosModules.common-cpu-amd
          nixos-hardware.nixosModules.common-gpu-amd
          openwrt.nixosModule
          home-manager.nixosModules.home-manager
          {
            home-manager = {
              useGlobalPkgs = true;
              useUserPackages = true;
              users.svein = import ./home/home.nix;
            };
          }
          ./tsugumi/configuration.nix
        ];
      };
    };
}
