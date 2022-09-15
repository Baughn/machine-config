{
  config,
  pkgs,
  ...
}: {
  hardware.nvidia.modesetting.enable = true;
  #hardware.nvidia.powerManagement.enable = true;
  #boot.kernelParams = [
  #  "nvidia.NVreg_EnableS0ixPowerManagement=1"
  #];
  services.xserver.videoDrivers = ["nvidia"];
  services.xserver.screenSection = ''
    Option "metamodes" "nvidia-auto-select +0+0 { ForceCompositionPipeline = On, ForceFullCompositionPipeline=On, AllowGSYNCCompatible=On }"
  '';
  environment.systemPackages = [pkgs.nvtop];
  services.xserver.displayManager.gdm.wayland = false;
  hardware.opengl.enable = true;
}
