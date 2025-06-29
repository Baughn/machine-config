{ config, lib, pkgs, ... }:

let
  python = pkgs.python310;
  cudaLibPath = "/run/opengl-driver/lib:" + (with pkgs; lib.makeLibraryPath [
    cudatoolkit_11
    cudaPackages.cudnn
    stdenv.cc.cc.lib
    libGL
    glib
  ]);
  COMFYUI_DIR = "/home/svein/AI/image-generation/ComfyUI";
  BOT_DIR = "/home/svein/AI/image-generation/sd-bot-2";

  comfyui = {
    description = "ComfyUI";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    path = [ pkgs.uv pkgs.git pkgs.gcc ];
    serviceConfig = {
      User = "svein";
      WorkingDirectory = COMFYUI_DIR;
      Type = "simple";
      Restart = "always";
      ExecStart = "${pkgs.steam-run}/bin/steam-run " + COMFYUI_DIR + "/bwrapped.sh";
      MemoryMax = "64G";
      RuntimeMaxSec = "72h";
      # LD_PRELOAD tcmalloc.
      Environment = "LD_PRELOAD=${pkgs.gperftools}/lib/libtcmalloc.so";
    };
  };

  # The bot's a simple rust app.
  bot = {
    description = "sd-bot";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    path = [ pkgs.nix pkgs.cached-nix-shell ];
    serviceConfig = {
      User = "svein";
      WorkingDirectory = BOT_DIR;
      Type = "simple";
      Restart = "always";
      RestartSec = 10;
      Environment = "NIX_PATH=nixpkgs=/etc/nixpkgs";
      ExecStart = "${BOT_DIR}/start.sh";
    };
  };
in
{
  systemd.services.comfyui = comfyui;
  #systemd.services.sd-bot = bot;

  networking.firewall.allowedTCPPorts = [ 8188 ];
}
