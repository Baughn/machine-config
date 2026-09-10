{ config, ... }:
{
  me.punch.groups.stationeers = import ../../lib/stationeers-access.nix;
  me.punch.remoteBrokers.saya = {
    url = "http://10.171.0.6:9781";
    tokenFile = config.age.secrets.punch-saya-token.path;
  };
  age.secrets.punch-saya-token.file = ../../secrets/punch-saya-token.age;
}
