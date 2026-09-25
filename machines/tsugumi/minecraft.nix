{ config, ... }:
{
  imports = [
    ./minecraft-storage.nix
    ./minecraft-snapshot.nix
    ./minecraft-servers.nix
    ./minecraft-watch.nix
    ./agents.nix
  ];

  me.minecraft.autostart = [ "erisia" ];
  me.minecraft.watch.webhookFile = config.age.secrets.minecraft-watch-webhook.path;

  me.punch.groups.minecraft = {
    label = "Minecraft";
    roleIds = [ "480078714709737473" ];
    ports.tcp = [ 25565 25566 ];
    ports.udp = [ 24454 ]; # Simple voice chat
  };
  services.prometheus.scrapeConfigs = [
    {
      job_name = "erisia";
      static_configs = [
        {
          targets = [ "localhost:1224" ];
        }
      ];
    }
  ];
}
