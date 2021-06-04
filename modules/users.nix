{ lib, config, ... }:

let
  sshKeys = import ./sshKeys.nix;
  users = {
    svein = {
      uid = 1000;
      extraGroups = [ "wheel" "wireshark" "systemd-journal" "disnix" "networkmanager" ];
      inherit (import ../secrets) initialPassword;
      createHome = false;
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
    znapzend = {
      uid = 1054;
      openssh.authorizedKeys.keys = [
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAW37vjjfhK1hBwHO6Ja4TRuonXchlLVIYnA4Px9hTYD svein@madoka.brage.info"
      ] ++ sshKeys.svein;
    };
    lucca.uid = 1055;
    dusk.uid = 1056;
    ahigerd = {
      uid = 1057;
      openssh.authorizedKeys.keys = [
        "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAACAQDpbzzqjOgaBCsLV5YFWynecshLYEFAvC4ztKv/v3b0SSSXs6hUnzQ8Bg/sJdlsdaeQCUy3CocS4TAAc1Iw8AI+h4HnTE4cdYMOrKoy01Q9Tim1x0+pOvBpya28YtvWRkLv3uLP/Hyi8J2nbj00ToNQ5VcXJ61PAHQ5BOoLSSJwIIzOkTbqup1Gz5CqMhYZ0fQ9Xcx55VubTnqHqm2p0Y87ZgOZKSvT0b9/m2yhUUiCy/ady4ZhUI7fP60D5fCyDUuOejjmRGkhIUmgbxgJLxg8//X0bcNQXbwl4Sw+zhkAdhBC12woP3t1KCuoOT/2+TeQ5K5wZxtnBs+otCzl8GGkbMH2kRC/K7WxIVNbXWzU7Gn1LI22w+74kLFJwMjI6vWrznrIIWoJ8Hn3s6rhaW5obt7IBj+ObkQXHedOuUPsGHYotyOUXqfyPkMgYM0Zu4vVCeol3JfXhNiqLE39zJCtBztct0YtGomHjm0gcocmxngH2Q3xticmcWc+wSq4eRLqL3NP/cpUScc80ym/xz8AUwnH/3RGbhj/GcHRLm7Gn1hsHgPi4WOxG8QTkLTf2drO54wZyl1KoQisTYBiHpNuU3F2GsUb9SDAX/+YDrbVsQTortc60eVcG/ZTApJYTjvxsmp6TvbqWew3qk0WmVb9NvYJx/krGY6JuCTkwVVikQ== adam@wherefor.com"
      ];
    };
    jared.uid = 1058;
    snowfire = {
      uid = 1059;
      openssh.authorizedKeys.keys = [
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGQJY48dXt3SZd/ZVZHqX2hPoQGljrIGTQCbJbn6JtLa snowfirek@pop-os"
      ];
    };
    grissess = {
      uid = 1060;
      openssh.authorizedKeys.keys = [
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIByY7vM2HdrHD3z9vAikiT7H3A6anjSPyT27a1GEePFe grissess@ceres"
      ];
    };
    linuxgsm = {
      uid = 1061;
    };
    # Next free ID: 1062
    anne.uid = 1100;
  };
  includeUser = username: ({
    isNormalUser = true;
    openssh.authorizedKeys.keys = sshKeys.${username} or [];
  } // users.${username});
in

with lib; {
  options = {
    users.include = mkOption {
      type = types.listOf types.str;
      description = "Users to include on this system";
      default = [];
    };
  };

  config = {
    users.users = lib.genAttrs config.users.include includeUser;
  };
}
