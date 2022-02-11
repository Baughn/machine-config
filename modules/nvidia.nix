{ config, pkgs, ... }: {
  hardware.nvidia.modesetting.enable = true;
  services.xserver.videoDrivers = [ "nvidia" ];
  services.xserver.screenSection = ''
    Option "metamodes" "nvidia-auto-select +0+0 { ForceCompositionPipeline = On, ForceFullCompositionPipeline=On, AllowGSYNCCompatible=On }"
  '';
  environment.systemPackages = [ pkgs.nvtop ];
#  services.xserver.displayManager.gdm.wayland = false;
}
