{ config, ... }:
{
  imports = [
    ./minecraft-storage.nix
    ./minecraft-lab.nix
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
      # Prometheus Integration mod. 15 s so tick spikes are visible to
      # tsugumi-minecraft's tick debugging, not just a 1-minute average.
      job_name = "erisia";
      scrape_interval = "15s";
      static_configs = [
        {
          targets = [ "127.0.0.1:1224" ];
        }
      ];
    }
  ];
}
