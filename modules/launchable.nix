{ config, lib, pkgs, ... }:

{
  options.environment.launchable = with lib; with types; {
    systemPackages = mkOption {
      type = listOf package;
      default = [];
      description = ''
        Packages which should be downloaded & installed on first use.

        This requires either that the main binary has the same name as the package,
        or else that meta.mainProgram is set.
      '';
    };

    installMenuEntries = mkEnableOption {
      description = "Install a desktop entry for each launchable.";
      default = true;
    };
  };

  config = let
    mkLaunchable = p: let
      name = p.pname or (builtins.head (builtins.split "-" p.name));
      executable = p.meta.mainProgram or "${name}";

      # This is needed to avoid pulling down every build-time dependency.
      # The .drv file itself will still be in the closure.
      drv = builtins.unsafeDiscardStringContext
        (builtins.toJSON p.drvPath);

    in pkgs.writeTextFile {
        name = "${name}-launcher";
        executable = true;
        destination = "/bin/${executable}";

        text = ''
          #!${pkgs.runtimeShell}

          set -o errexit
          set -o nounset
          set -o pipefail

          exec "$(nix-store -r "${drv}" 2>/dev/null)/bin/${executable}" "$@"
        '';
      };

  in {
    environment.systemPackages = map mkLaunchable config.environment.launchable.systemPackages;
  };
}
