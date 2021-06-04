{
  description = "Machine configs";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.hercules-ci-agent.url = "github:hercules-ci/hercules-ci-agent/nixos-unstable";

  outputs = { self, nixpkgs, hercules-ci-agent }: {
    nixosConfigurations.tsugumi = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./tsugumi/configuration.nix
      ];
    };
  };
}
