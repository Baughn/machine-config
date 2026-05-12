{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nix-cachyos-kernel.url = "github:xddxdd/nix-cachyos-kernel/release";
    nix-cachyos-kernel.inputs.nixpkgs.follows = "nixpkgs";
    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
    codex-cli-nix.url = "github:sadjow/codex-cli-nix";
    codex-cli-nix.inputs.nixpkgs.follows = "nixpkgs";
    crane.url = "github:ipetkov/crane";
    ganbot.url = "git+file:/home/svein/dev/ganbot?ref=HEAD";
    ganbot.inputs.nixpkgs.follows = "nixpkgs";
    ganbot.inputs.crane.follows = "crane";
    dessplay.url = "git+file:/home/svein/dev/dessplay?ref=HEAD";
    dessplay.inputs.nixpkgs.follows = "nixpkgs";
    agenix.url = "github:ryantm/agenix";
    agenix.inputs.nixpkgs.follows = "nixpkgs";
    colmena.url = "github:zhaofengli/colmena";
    colmena.inputs.nixpkgs.follows = "nixpkgs";
    nix-index-database.url = "github:nix-community/nix-index-database";
    nix-index-database.inputs.nixpkgs.follows = "nixpkgs";
    disko.url = "github:nix-community/disko";
    disko.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, nix-cachyos-kernel, home-manager, codex-cli-nix, crane, ganbot, dessplay, agenix, colmena, nix-index-database, disko, ... }:
  let
    system = "x86_64-linux";

    diskoInstall = disko.packages.${system}.disko-install;

    craneOverlay = final: prev: {
      craneLib = crane.mkLib final;
      mkCranePackage = final.callPackage ./lib/mk-crane-package.nix { };
    };

    pkgs = import nixpkgs {
      inherit system;
      overlays = [ craneOverlay ];
    };

    craneModule = {
      nixpkgs.overlays = [ craneOverlay ];
    };

    rustManifestPaths = [
      "machines/v4/v4proxy/Cargo.toml"
      "tools/aniwatch/Cargo.toml"
      "tools/game-watcher/Cargo.toml"
      "tools/irc-tool/Cargo.toml"
      "tools/magic-reboot/sender/Cargo.toml"
      "tools/nix-build-balancer/Cargo.toml"
      "tools/rolebot/Cargo.toml"
      "tools/victron-monitor/Cargo.toml"
    ];

    checkRustTools = pkgs.writeShellApplication {
      name = "check-rust-tools";
      runtimeInputs = [
        pkgs.cargo
        pkgs.cmake
        pkgs.git
        pkgs.pkg-config
        pkgs.rustc
        pkgs.stdenv.cc
      ];
      text = ''
        root="$(git rev-parse --show-toplevel)"
        cd "$root"

        export CARGO_TARGET_DIR="''${CARGO_TARGET_DIR:-$root/target/rust-tools}"

        for manifest in ${pkgs.lib.escapeShellArgs rustManifestPaths}; do
          echo "==> cargo test --manifest-path $manifest"
          cargo test --manifest-path "$manifest"
        done
      '';
    };

    rustPackages = {
      aniwatch = pkgs.callPackage ./tools/aniwatch { };
      game-watcher = pkgs.mkCranePackage {
        pname = "game-watcher";
        version = "0.1.0";
        src = ./tools/game-watcher;
      };
      irc-tool = pkgs.callPackage ./tools/irc-tool { };
      magic-reboot-send = pkgs.callPackage ./tools/magic-reboot/sender { };
      nix-build-balancer = pkgs.callPackage ./tools/nix-build-balancer { };
      rolebot = pkgs.callPackage ./tools/rolebot { };
      v4proxy = pkgs.mkCranePackage {
        pname = "v4proxy";
        version = "0.1.0";
        src = ./machines/v4/v4proxy;
      };
      victron-monitor = pkgs.callPackage ./tools/victron-monitor { };
    };

    commonModules = [
      home-manager.nixosModules.home-manager
      nix-index-database.nixosModules.nix-index
      craneModule
      {
        programs.nix-index-database.comma.enable = true;
      }
    ];

    machineConfigs = {
      saya = {
        modules = [
          ./machines/saya
          ({ pkgs, ... }: {
            nixpkgs.overlays = [
              nix-cachyos-kernel.overlays.default
              (final: prev: {
                codex = codex-cli-nix.packages.${prev.stdenv.hostPlatform.system}.default;
                kdePackages = prev.kdePackages.overrideScope (kfinal: kprev: {
                  kwin = kprev.kwin.overrideAttrs (old: {
                    # patches = (old.patches or []) ++ [ ./kwin.patch ];
                    src = ./kwin;
                  });
                });
              })
            ];
          })
        ];
        deployment = {
          targetHost = "localhost";
          allowLocalDeployment = true;
          tags = [ "local" ];
        };
      };

      tsugumi = {
        modules = [ ./machines/tsugumi ];
        deployment = {
          targetHost = "tsugumi.local";
          tags = [ "remote" ];
        };
      };

      v4 = {
        modules = [ ./machines/v4 ];
        deployment = {
          targetHost = "v4.brage.info";
          tags = [ "remote" ];
        };
      };
    };
  in
  rec {
    packages.x86_64-linux = rustPackages // {
      all-systems =
        pkgs.linkFarm "all-systems"
          (builtins.map
            (name: {
              inherit name;
              path = colmenaHive.nodes.${name}.config.system.build.toplevel;
            })
            (builtins.attrNames machineConfigs));

      default = packages.x86_64-linux.all-systems;
    };

    checks.x86_64-linux = {
      saya-installer-vm =
        let
          testInstaller = nixpkgs.lib.nixosSystem {
            inherit system;
            specialArgs = {
              inherit agenix nix-cachyos-kernel diskoInstall;
              sshKeys = import ./lib/ssh-keys.nix;
              flakeSelf = self;
            };
            modules = [
              ./machines/saya-installer
              ({ lib, ... }: {
                # The production activation script blocks on
                # systemd-ask-password --timeout=0 waiting for the host
                # key passphrase. There is no operator in the VM test, so
                # neuter the script and let the system boot to login.
                system.activationScripts.decryptHostKey.text = lib.mkForce ''
                  : "saya-installer VM test: host key decryption disabled"
                '';
              })
            ];
          };
        in
        import ./tests/saya-installer-vm.nix {
          inherit pkgs;
          inherit (nixpkgs) lib;
          installerCfg = testInstaller.config;
        };
    };

    devShells.x86_64-linux.default = pkgs.mkShell {
      packages = [
        checkRustTools
        pkgs.cargo
        pkgs.clippy
        pkgs.cmake
        pkgs.pkg-config
        pkgs.rust-analyzer
        pkgs.rustc
        pkgs.rustfmt
        pkgs.sqlite
        pkgs.openssl
      ];

      RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
    };

    colmenaHive = colmena.lib.makeHive ({
      meta = {
        nixpkgs = import nixpkgs {
          inherit system;
          overlays = [ colmena.overlays.default ];
        };
        specialArgs = { inherit agenix dessplay ganbot diskoInstall; flakeSelf = self; };
      };

      defaults = { ... }: {
        imports = commonModules;

        deployment = {
          targetUser = "svein";
          buildOnTarget = false;
          replaceUnknownProfiles = true;
        };
      };
    } // (builtins.mapAttrs
      (name: machine: { ... }: {
        imports = machine.modules;
        deployment = machine.deployment;
      })
      machineConfigs));

    nixosConfigurations = colmenaHive.nodes // {
      saya-installer = nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = {
          inherit agenix nix-cachyos-kernel diskoInstall;
          sshKeys = import ./lib/ssh-keys.nix;
          flakeSelf = self;
        };
        modules = [ ./machines/saya-installer ];
      };
    };
  };
}
