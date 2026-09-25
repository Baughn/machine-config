{
  description = "NixOS configuration for saya (desktop) and tsugumi (server)";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/DeterminateSystems/nixpkgs-weekly/0.1";
    # Tracks nixos-unstable directly, for packages where the weekly cooldown
    # is too slow (currently: google-chrome, security updates).
    nixpkgs-fast.url = "github:NixOS/nixpkgs/nixos-unstable";
    nix-cachyos-kernel.url = "github:xddxdd/nix-cachyos-kernel/release";
    nix-cachyos-kernel.inputs.nixpkgs.follows = "nixpkgs";
    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
    nix-darwin.url = "github:nix-darwin/nix-darwin/master";
    nix-darwin.inputs.nixpkgs.follows = "nixpkgs";
    crane.url = "github:ipetkov/crane";
    ganbot.url = "github:Baughn/ganbot";
    ganbot.inputs.nixpkgs.follows = "nixpkgs";
    ganbot.inputs.crane.follows = "crane";
    dessplay.url = "github:baughn/dessplay";
    dessplay.inputs.nixpkgs.follows = "nixpkgs";
    agenix.url = "github:ryantm/agenix";
    agenix.inputs.nixpkgs.follows = "nixpkgs";
    nix-index-database.url = "github:nix-community/nix-index-database";
    nix-index-database.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, nixpkgs-fast, nix-cachyos-kernel, home-manager, nix-darwin, crane, dessplay, ganbot, agenix, nix-index-database, ... }:
  let
    system = "x86_64-linux";

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
      "tools/aniwatch/Cargo.toml"
      "tools/game-watcher/Cargo.toml"
      "tools/irc-tool/Cargo.toml"
      "tools/magic-reboot/sender/Cargo.toml"
      "tools/nix-build-balancer/Cargo.toml"
      "tools/nix-deploy/Cargo.toml"
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
              (final: prev: {
                # Browser security updates can't wait for the weekly cooldown.
                google-chrome = (import nixpkgs-fast {
                  inherit (prev.stdenv.hostPlatform) system;
                  inherit (prev) config;
                }).google-chrome;
                kdePackages = prev.kdePackages.overrideScope (kfinal: kprev: {
                  kwin = kprev.kwin.overrideAttrs (old: {
                    # patches = (old.patches or []) ++ [ ./kwin.patch ];
                    #src = ./kwin;
                  });
                });
              })
            ];
          })
        ];
      };

      tsugumi = {
        modules = [
          ./machines/tsugumi
          {
            nixpkgs.overlays = [
              (final: prev: {
                # The agent bridge's CLI: new models need a newer Claude Code
                # than the weekly cooldown gives (Opus 5.5 needs >= 2.1.280).
                claude-code = (import nixpkgs-fast {
                  inherit (prev.stdenv.hostPlatform) system;
                  inherit (prev) config;
                }).claude-code;
              })
            ];
          }
        ];
      };
    };

    nixosMachines = builtins.mapAttrs
      (name: machine: nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = { inherit agenix dessplay ganbot; flakeSelf = self; };
        modules = commonModules ++ [
          { nixpkgs.overlays = [ nix-cachyos-kernel.overlays.default ]; }
        ] ++ machine.modules;
      })
      machineConfigs;
  in
  rec {
    packages.x86_64-linux = rustPackages // {
      agent-bridge = pkgs.callPackage ./tools/agent-bridge { };
      all-systems =
        pkgs.linkFarm "all-systems"
          (builtins.map
            (name: {
              inherit name;
              path = nixosMachines.${name}.config.system.build.toplevel;
            })
            (builtins.attrNames machineConfigs));

      default = packages.x86_64-linux.all-systems;
    };

    checks.x86_64-linux = {
      # pytest and mypy --strict run in the package's checkPhase.
      agent-bridge = packages.x86_64-linux.agent-bridge;
      punch = pkgs.runCommand "punch-tests" {
        nativeBuildInputs = [ (pkgs.python3.withPackages (p: [ p.aiohttp ])) ];
      } ''
        export PYTHONDONTWRITEBYTECODE=1
        cd ${pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            ./modules/punch
            ./tests/test_punch.py
          ];
        }}
        python3 -m unittest discover -s tests -p 'test_punch.py' -v
        touch "$out"
      '';
      punch-vm = import ./tests/punch-vm.nix { inherit pkgs; };
      local-web-access = import ./tests/local-web-access-vm.nix { inherit pkgs; };
      minecraft-storage-vm = import ./tests/minecraft-storage-vm.nix { inherit pkgs; };
      minecraft-servers-vm = import ./tests/minecraft-servers-vm.nix { inherit pkgs; };
      minecraft-watch-vm = import ./tests/minecraft-watch-vm.nix { inherit pkgs; };
      agent-channel-vm = import ./tests/agent-channel-vm.nix {
        # The bridge's CLI, claude-code, is unfree.
        pkgs = import nixpkgs { inherit system; config.allowUnfree = true; };
      };
      security-scripts = pkgs.runCommand "security-script-tests" {
        nativeBuildInputs = [ (pkgs.python3.withPackages (p: [ p.javaproperties ])) ];
      } ''
        export PYTHONDONTWRITEBYTECODE=1
        cd ${pkgs.lib.fileset.toSource {
          root = ./.;
          fileset = pkgs.lib.fileset.unions [
            ./machines/tsugumi/minecraft-storage.py
            ./machines/tsugumi/minecraft-snapshot.py
            ./machines/tsugumi/minecraft-watch.py
            ./machines/tsugumi/starlink-prefixes.py
            ./tests/test_minecraft_storage.py
            ./tests/test_minecraft_snapshot.py
            ./tests/test_minecraft_watch.py
            ./tests/test_starlink_prefixes.py
          ];
        }}
        python3 -m unittest discover -s tests -p 'test_*.py' -v
        touch "$out"
      '';
      saya-installer-vm =
        let
          testInstaller = nixpkgs.lib.nixosSystem {
            inherit system;
            specialArgs = {
              inherit agenix nix-cachyos-kernel;
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

    darwinConfigurations.kaho = nix-darwin.lib.darwinSystem {
      modules = [
        ./machines/kaho
        home-manager.darwinModules.home-manager
        craneModule
      ];
      specialArgs = { inherit agenix; flakeSelf = self; };
    };

    nixosConfigurations = nixosMachines // {
      saya-installer = nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = {
          inherit agenix nix-cachyos-kernel;
          sshKeys = import ./lib/ssh-keys.nix;
          flakeSelf = self;
        };
        modules = [ ./machines/saya-installer ];
      };
    };
  };
}
