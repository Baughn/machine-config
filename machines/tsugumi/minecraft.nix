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

  me.minecraft.autostart = [ "erisia" "incognito" ];
  me.minecraft.watch.webhookFile = config.age.secrets.minecraft-watch-webhook.path;

  # WireGuard peers (Baughn's own machines) skip Punch: joining over the router's public
  # address hairpins, and voice chat's UDP doesn't survive that (Baughn, msg 1562977732667768978).
  # Join via 10.171.0.1 from a WireGuard peer.
  networking.firewall.interfaces.wg0 = {
    allowedTCPPorts = config.me.punch.groups.minecraft.ports.tcp;
    allowedUDPPorts = config.me.punch.groups.minecraft.ports.udp;
  };

  me.punch.groups.minecraft = {
    label = "Minecraft";
    roleIds = [ "480078714709737473" ];
    ports.tcp = [ 25565 25566 ];
    ports.udp = [ 24454 ]; # Simple voice chat
    # Playing keeps access open: a login renews its address's grants (Baughn, 2026-10-05).
    minecraftLoginUnits = [ "minecraft@*.service" ];
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
