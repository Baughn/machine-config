{
  description = "Machine configs";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.hercules-ci-agent.url = "github:hercules-ci/hercules-ci-agent/nixos-unstable";
  inputs.openwrt = {
    url = "path:../openwrt";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, hercules-ci-agent, openwrt }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      devShell.${system} = import ./shell.nix { inherit pkgs; };
      nixosConfigurations.tsugumi = nixpkgs.lib.nixosSystem {
        inherit system;

        modules = [
          openwrt.nixosModule
          ./tsugumi/configuration.nix
        ];
      };
    };
}
