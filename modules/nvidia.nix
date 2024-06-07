{
  config,
  pkgs,
  ...
}: {
  hardware.nvidia.package = config.boot.kernelPackages.nvidiaPackages.beta;
  # Work around mouse stuttering & firefox crash
  boot.kernelParams = [
    "nvidia.NVreg_EnableGpuFirmware=0"
  ];

  #hardware.nvidia.open = true;
  hardware.nvidia.modesetting.enable = true;
  #hardware.nvidia.powerManagement.enable = true;
  services.xserver.videoDrivers = ["nvidia"];
  environment.systemPackages = [pkgs.nvtopPackages.nvidia];
  services.xserver.displayManager.gdm.wayland = true;
  hardware.opengl.enable = true;
}
