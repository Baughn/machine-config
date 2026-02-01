{ pkgs, lib, config, ... }: {
  imports = [
    ./hardware-configuration.nix
    ../../modules/v4proxy.nix
    ../../modules/rendezvous.nix
    ../../modules
  ];

  # IPv4 to IPv6 proxy
  services.v4proxy = {
    enable = true;
    defaultTarget = "direct.brage.info";
    mappings = [
      # Minecraft
      { localPort = 25565; }
      { localPort = 25566; }
      # Stationeers
      { protocol = "udp"; localPort = 27015; target = "saya.brage.info"; }
      { protocol = "udp"; localPort = 27016; target = "saya.brage.info"; }
    ];
  };

  # DessPlay rendezvous server
  services.rendezvous = {
    enable = true;
    passwordFile = config.age.secrets."rendezvous.key".path;
  };

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
