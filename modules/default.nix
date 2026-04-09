{ lib, platform, ... }:

let
  mylib = import ../lib { inherit lib; };
  mod = mylib.mkPlatformModule platform;
in
{
  imports = [
    (mod ./shell)
    (mod ./cli-tools)
    (mod ./nix)
    (mod ./dns)
    (mod ./ssh)
    (mod ./wireguard)
    (mod ./mdns)
    (mod ./home-manager)
  ];
}
