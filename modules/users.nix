{ lib, ... }:

let
  sshKeys = import ./sshKeys.nix;
  users = {
    svein = {
      uid = 1000;
      extraGroups = [ "wheel" "docker" "wireshark" ];
    };
    bloxgate.uid = 1001;
    kim.uid = 1002;
    jmc = {
      uid = 1003;
      shell = "/run/current-system/sw/bin/bash";
    };
    david.uid = 1005;
    luke.uid = 1006;
    darqen27.uid = 1007;
    simplynoire.uid = 1009;
    buizerd.uid = 1010;
    vindex.uid = 1011;
    xgas.uid = 1012;
    einsig.uid = 1014;
    prospector.uid = 1015;
    mei.uid = 1017;
    minecraft = {
      uid = 1018;
      openssh.authorizedKeys.keys = builtins.concatLists (lib.attrValues sshKeys);
    };
    will.uid = 1050;
    pl.uid = 1051;
    aquagon.uid = 1052;
    # Next free ID: 1054
  };
  includeUser = username: ({
    isNormalUser = true;
    openssh.authorizedKeys.keys = sshKeys.${username} or [];
  } // users.${username});
in

{
  include = usernames: {
    extraUsers = lib.genAttrs usernames includeUser;
  };
}
