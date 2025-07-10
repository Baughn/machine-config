{ config
, pkgs
, ...
}:
let
  server = pkgs.stdenvNoCC.mkDerivation rec {
    name = "vintagestory-server-${version}";
    version = "1.16.5";
    src = pkgs.fetchurl {
      url = "https://cdn.vintagestory.at/gamefiles/stable/vs_server_${version}.tar.gz";
      sha256 = "1hq1zz05kwz68bl84n32nigk791lg2yb9mmq3q8ncjjs00caxrrg";
    };
    sourceRoot = ".";

    nativeBuildInputs = [ pkgs.autoPatchelfHook ];

    installPhase = ''
      mkdir $out
      cp -a ./* $out/
    '';
  };
in
{
  networking.firewall.allowedUDPPorts = [
    42420
  ];
  networking.firewall.allowedTCPPorts = [
    42420
  ];

  systemd.services.vintagestory = {
    description = "The Vintage Story dedicated server";
    wantedBy = [ "multi-user.target" ];
    after = [ "network-online.target" ];
    wants = [ "network-online.target" ];
    serviceConfig = {
      Restart = "always";
      StateDirectory = "vintagestory";
      DynamicUser = true;
    };
    restartTriggers = [ server ];
    script =
      let
        libraryPath = pkgs.lib.makeLibraryPath [ pkgs.sqlite ];
      in
      ''
        set -exu -o pipefail
        export HOME="$STATE_DIRECTORY"
        cd "$HOME"
        mkdir -p data
        rm -rf server
        ln -s ${server} server

        SERVERDIR=$HOME/server
        DATADIR=$HOME/data
        export LD_LIBRARY_PATH=${libraryPath}

        ${pkgs.mono}/bin/mono $SERVERDIR/VintagestoryServer.exe --dataPath $DATADIR "$@"
      '';
  };
}
