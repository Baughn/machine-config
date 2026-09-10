{ config, ... }:
{
  me.punch = {
    enable = true;
    frontendEnable = false;
    groups.stationeers = import ../../lib/stationeers-access.nix;
    listenAddress = "10.171.0.6";
    tokenFile = config.age.secrets.punch-saya-token.path;
  };
  systemd.services.punch-firewall = {
    requires = [ "wireguard-wg1.service" ];
    after = [ "wireguard-wg1.service" ];
  };
  networking.firewall.interfaces.wg1.allowedTCPPorts = [ 9781 ];
  age.secrets.punch-saya-token.file = ../../secrets/punch-saya-token.age;
}
