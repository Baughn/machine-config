{ pkgs, ... }:

# Keep build output, code checkouts and game data out of Baloo's index.
#
# baloofilerc stays a mutable file (Baloo writes dbVersion and the filter
# version into it), so the keys we own are rewritten on activation.
# "exclude filters" replaces Baloo's built-in default list rather than
# extending it (its in-memory merge doesn't survive a config reload), so we
# write the installed defaults plus our additions.
# Restarting the indexer makes it purge entries the new config excludes.
let
  # Directory/file names excluded at any depth. Baloo ignores CACHEDIR.TAG.
  excludeFilters = [
    "target"                # Cargo build output
    "OpenTerrainGenerator"  # 66k worldgen files per Minecraft instance
  ];

  # Relative to $HOME.
  excludeFolders = [
    "dev"                   # code; use ripgrep
    "src"
    "old/Games"             # game installs
    "PrismLauncher/assets"
    "PrismLauncher/libraries"
    "PrismLauncher/java"
  ];
in
{
  home-manager.users.svein = { config, lib, ... }:
    let
      home = config.home.homeDirectory;
      rc = "${config.xdg.configHome}/baloofilerc";
      kwrite = "${pkgs.kdePackages.kconfig}/bin/kwriteconfig6 --file ${rc} --group General";
      folders = map (d: "${home}/${d}/") excludeFolders;
      filtersArg = lib.concatStringsSep "," excludeFilters;
      foldersArg = lib.concatStringsSep "," folders;
      # Baloo rewrites the merged filter list itself, so diffing the rc file
      # would restart it on every activation; compare against a stamp instead.
      stamp = "${config.xdg.stateHome}/baloo-excludes.stamp";
      # Includes baloo itself, whose default filter list we copy in.
      wanted = builtins.hashString "sha256"
        "2\n${pkgs.kdePackages.baloo}\n${filtersArg}\n${foldersArg}";
    in
    {
      home.activation.balooExcludes = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
        if [ "$(cat ${stamp} 2>/dev/null)" != "${wanted}" ]; then
          # With the key absent, balooctl reports the installed defaults.
          run ${kwrite} --key "exclude filters" --delete
          defaults=$(${pkgs.kdePackages.baloo}/bin/balooctl6 config list excludeFilters \
            | tail -n +2 | sed 's/^ *//' | paste -sd,)
          if [ -z "$defaults" ]; then
            echo "baloo: could not read default exclude filters" >&2
            exit 1
          fi
          run ${kwrite} --key "exclude filters" "$defaults,"${lib.escapeShellArg filtersArg}
          run ${kwrite} --key "exclude filters version" --delete
          run ${kwrite} --key "exclude folders" ${lib.escapeShellArg foldersArg}
          run mkdir -p ${config.xdg.stateHome}
          run ${pkgs.systemd}/bin/systemctl --user try-restart kde-baloo.service || true
          if [ -z "''${DRY_RUN:-}" ]; then echo ${wanted} > ${stamp}; fi
        fi
      '';
    };
}
