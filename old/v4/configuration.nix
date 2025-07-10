{ pkgs, ... }: {
  imports = [
    ./hardware-configuration.nix
    ./v4proxy.nix
    ../modules
  ];

  environment.systemPackages = with pkgs; [
    neovim
  ];

  systemd.network = {
    enable = true;
    networks."10-wan" = {
      matchConfig.Type = "ether";
      networkConfig.Address = ["51.75.169.212/24" "2001:41d0:801:1000::22d7/64"];
      networkConfig.Gateway = ["51.75.169.1" "2001:41d0:801:1000::1"];
    };
  };

  boot.tmp.cleanOnBoot = true;
  zramSwap.enable = true;
  networking.hostName = "v4";
  networking.domain = "brage.info";
  services.openssh.enable = true;
  system.stateVersion = "23.11";
}
