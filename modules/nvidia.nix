{ config, lib, pkgs, ... }:

{
  # AIDEV-NOTE: NVIDIA hardware configuration for desktop systems
  hardware.nvidia = {
    modesetting.enable = true;
    open = false;
  };
  services.xserver.videoDrivers = [ "nvidia" ];

  # Environment variables for G-Sync/VRR (explicit sync auto-enabled in Plasma 6.3+)
  environment.sessionVariables = {
    __GL_GSYNC_ALLOWED = "1";
    __GL_VRR_ALLOWED = "1";
  };

  # NVIDIA monitoring tools
  environment.systemPackages = with pkgs; [
    nvtopPackages.nvidia
  ];
}
