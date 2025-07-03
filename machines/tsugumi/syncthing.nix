{ config, lib, pkgs, ... }:

{
  services.syncthing = {
    enable = true;
    openDefaultPorts = true;
    user = "svein";
    configDir = "/home/svein/.config/syncthing";
    dataDir = "/home/svein/Sync";
    settings = {
      devices = {
        kaho.id = "MD47JRV-UL5JHDJ-VHSSSEC-OPAQGRS-X5MEAH3-MBJUBCO-WG3XIZA-7ZX2KQU";
      };
      folders = {
        "/home/svein/Sync" = {
          id = "default";
          devices = [ "kaho" ];
        };
        "/home/svein/Music" = {
          id = "Music";
          devices = [ "kaho" ];
        };
        "/home/svein/Documents" = {
          id = "Documents";
          devices = [ "kaho" ];
        };
        "/home/svein/secure" = {
          id = "secure";
          devices = [ "kaho" ];
        };
      };
    };
  };
}
