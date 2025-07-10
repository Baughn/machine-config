{ pkgs, ... }:
let
  netboot = (import (pkgs.path + "/nixos/release.nix") { }).netboot.x86_64-linux;
  cmdLine = builtins.readFile (pkgs.runCommandNoCCLocal "cmdline" { } ''
    grep '^kernel' ${netboot}/netboot.ipxe | sed -r 's/.*(init=[^ ]+).*/\1/' | tr -d '\n' > $out
  '');
in
{
  services.pixiecore = {
    enable = true;
    inherit cmdLine;
    kernel = "${netboot}/bzImage";
    initrd = "${netboot}/initrd";
    dhcpNoBind = true;
    port = 888;
    statusPort = 888;
    listen = "10.0.0.1";
  };
  networking.firewall.interfaces.internal = {
    allowedUDPPorts = [ 67 69 4011 ];
    allowedTCPPorts = [ 888 ];
  };
}
