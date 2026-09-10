{ ... }:
{
  imports = [ ./minecraft-storage.nix ];

  me.punch.groups.minecraft = {
    label = "Minecraft";
    roleIds = [ "480078714709737473" ];
    ports.tcp = [ 25565 25566 25575 ];
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
