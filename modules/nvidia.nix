{ config, pkgs, ... }: {
  services.xserver.videoDrivers = [ "nvidia" ];
  services.xserver.screenSection = ''
    Option "metamodes" "nvidia-auto-select +0+0 { ForceCompositionPipeline = On }"
  '';
  environment.systemPackages = [ pkgs.nvtop ];
}
