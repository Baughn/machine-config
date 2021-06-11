{
  description = "Machine configs";

  inputs.nixpkgs.url = "flake:nixpkgs/nixos-unstable";
  inputs.nixos-hardware.url = "flake:nixos-hardware";
  inputs.openwrt = {
    url = "path:../openwrt";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, openwrt, nixos-hardware }:
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
          ./tsugumi/configuration.nix
        ];
      };
    };
}
