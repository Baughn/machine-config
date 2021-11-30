{ config, lib, pkgs, ... }:

{
  options.environment.launchable = with lib; with types; {
    systemPackages = mkOption {
      type = functionTo (listOf package);
      default = [];
      description = ''
        Packages which should be downloaded & installed on first use.

        This requires either that the main binary has the same name as the package,
        or else that meta.mainProgram is set.
      '';
      example = ''
        pkgs: with pkgs; [ btop nethack ]
      '';
    };

    installMenuEntries = mkEnableOption {
      description = "Install a desktop entry for each launchable.";
      default = true;
    };
  };

  config = let
    deferPackageTree = subTree: path: builtins.mapAttrs (name: value:
      let nextPath = path ++ [ name ];
          attrPath = lib.concatStringsSep "." nextPath;
      in
      if lib.isDerivation value then mkLaunchable value attrPath
      else throw "Subsets not implemented."
    ) subTree;

    deferredPackageTree = deferPackageTree pkgs [];

    mkLaunchable = pkg: path: let
      name = pkg.pname or (builtins.head (builtins.split "-" pkg.name));
      executable = pkg.meta.mainProgram or "${name}";
      substitutable = builtins.unsafeDiscardStringContext pkg.outPath;

    in pkgs.writeTextFile {
        name = "${name}-launcher";
        executable = true;
        destination = "/bin/${executable}";

        text = ''
          #!${pkgs.runtimeShell}

          set -o errexit
          set -o nounset
          set -o pipefail

          GCDIR="$(mktemp -d)"
          trap "rm -r $GCDIR" EXIT

          # Attempt to just run it. This will work if it already exists,
          # or if it can be substituted.
          EXE="$(nix-store -r "${substitutable}" --add-root "$GCDIR/exe")" || {
            # Otherwise, evaluate from nixpkgs.
            DRV="$(nix-instantiate -E "((import ${pkgs.path} {}).${path})" --add-root "$GCDIR/drv")"
            EXE="$(nix-store -r "$DRV" --add-root "$GCDIR/exe")"
          }

          if [[ ! -e "$EXE/bin/${executable}" ]]; then
            echo "$(readlink -f "$EXE/bin")/bin/${executable} was not found." >/dev/stderr
            echo "This may be due to a missing or incorrect meta.mainProgram attribute for ${path}." >/dev/stderr
            echo >/dev/stderr
            echo "To launch nix-shell with the requested package, run:" >/dev/stderr
            echo "  nix-shell ${pkgs.path} -A ${path}" >/dev/stderr
            exit 100
          fi

          "$EXE/bin/${executable}" "$@"
        '';
      };

  in {
    environment.systemPackages = config.environment.launchable.systemPackages deferredPackageTree;
  };
}