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
      else if builtins.isAttrs value then deferPackageTree value nextPath
      else throw "${attrPath} is not a package"
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

          EXE="${substitutable}/bin/${executable}"

          # If it already exists, just use it.
          if [[ ! -e "${substitutable}" ]]; then
            # Otherwise, try to download it.
            GCDIR="$(mktemp -d)"
            trap "rm -r $GCDIR" EXIT
            nix-store -r "${substitutable}" --add-root "$GCDIR/exe" || true
            # If all else fails, evaluate from nixpkgs.
            if [[ ! -e "$GCDIR/exe" ]]; then
              DRV="$(nix-instantiate -E "((import ${pkgs.path} {}).${path})" --add-root "$GCDIR/drv")"
              nix-store -r "$DRV" --add-root "$GCDIR/exe"
            fi
          fi

          if [[ ! -e "$EXE" ]]; then
            echo "$EXE was not found." >/dev/stderr
            echo "This may be due to a missing or incorrect meta.mainProgram attribute for ${path}." >/dev/stderr
            echo >/dev/stderr
            echo "To launch nix-shell with the requested package, run:" >/dev/stderr
            echo "  nix-shell ${pkgs.path} -A ${path}" >/dev/stderr
            exit 100
          fi

          "$EXE" "$@"
        '';
      };

  in {
    #environment.systemPackages = config.environment.launchable.systemPackages deferredPackageTree;
    environment.systemPackages = config.environment.launchable.systemPackages pkgs;
  };
}
