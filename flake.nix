{
  description = "Machine configurations for all my machines";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
  };

  outputs = { self, nixpkgs }: {
    nixosConfigurations.saya = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./saya/configuration.nix
      ];
    };
  };
}
