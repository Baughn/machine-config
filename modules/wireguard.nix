{ config, lib, ... }:

let
  host = config.networking.hostName;
  guarded = {
    tsugumi.address = "10.40.0.1";
    saya.address = "10.40.0.2";
  };
in

{
#  networking.wg-quick = lib.mkIf (lib.any (h: h == host) guarded) {
#    interfaces.wg0 = {
#      addresses = "${guarded.${host}.address}/16";
#      listenPort = 10400;
#
#  }
}
