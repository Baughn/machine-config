{ ... }:
{
  imports = [
    ./minecraft-storage.nix
    ./minecraft-snapshot.nix
    ./minecraft-servers.nix
  ];

  me.minecraft.autostart = [ "erisia" ];

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
