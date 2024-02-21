{ pkgs, ... }:

let
  blocker = pkgs.stdenvNoCC.mkDerivation {
    name = "china-blocker";
    src = ./ipblock;
    installPhase = ''
      mkdir -p $out
      cp -r $src/* $out
    '';
  };
in

{
  networking.firewall.extraCommands = ''
    ${pkgs.python3}/bin/python3 ${blocker}/block.py ${pkgs.iptables}/sbin/iptables ${pkgs.ipset}/sbin/ipset
  '';
}
