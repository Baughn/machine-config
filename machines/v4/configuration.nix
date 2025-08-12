{ pkgs, lib, ... }: {
  imports = [
    ./hardware-configuration.nix
    ../../modules/v4proxy.nix
    ../../modules
  ];

  environment.systemPackages = with pkgs; [
  ];

  systemd.network = {
    enable = true;
    networks."10-wan" = {
      matchConfig.Type = "ether";
      networkConfig.Address = [ "51.75.169.212/24" "2001:41d0:801:1000::22d7/64" ];
      networkConfig.Gateway = [ "51.75.169.1" "2001:41d0:801:1000::1" ];
    };
  };

  networking.useNetworkd = true;

  zramSwap.enable = true;
  networking.hostName = "v4";
  networking.domain = "brage.info";

  # Additional users for v4
  users.include = [ "minecraft" ];

  # Disable linger for minecraft user (proxy jumps only, no services)
  users.users.minecraft.linger = lib.mkForce false;

  system.stateVersion = "23.11";
}
