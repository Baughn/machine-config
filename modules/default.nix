{ lib, platform, ... }:

let
  mylib = import ../lib { inherit lib; };
  mod = mylib.mkPlatformModule platform;
in
{
  imports = [
    (mod ./agenix)
    (mod ./shell)
    (mod ./cli-tools)
    (mod ./nix)
    (mod ./dns)
    (mod ./ssh)
    (mod ./wireguard)
    (mod ./mdns)
    (mod ./security)
    (mod ./firejail)
    ./home-manager
  ];
}
