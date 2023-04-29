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
  BOT_DIR = "/home/svein/AI/";

  bot = personality: {
    description = personality;
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    path = [ python pkgs.bash pkgs.git pkgs.openssh ];
    serviceConfig = {
      User = "svein";
      WorkingDirectory = BOT_DIR + personality;
      Type = "simple";
      Restart = "on-failure";
      Environment = "LD_LIBRARY_PATH=${cudaLibPath}";
      ExecStart = BOT_DIR + personality + "/webui.sh";
    };
  };
in
{
  systemd.services.sd-bot = bot "sd-bot";
  systemd.services.sd-personal = bot "sd-personal";

  networking.firewall.allowedTCPPorts = [ 7860 7861 ];
}
