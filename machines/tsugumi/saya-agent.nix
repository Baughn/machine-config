{ config, pkgs, ... }:
# A read-only login for the saya agent (saya-agent on saya), so it can debug
# tsugumi's units without an admin pasting logs: journal access, and a key
# forced through saya-agent-gate.py (journalctl and read-only systemctl, no
# shell, no forwarding). Asked for by Baughn on 2026-10-01.
let
  gate = pkgs.writeScript "saya-agent-gate" (
    "#!${pkgs.python3}/bin/python3 -I\n"
    + builtins.replaceStrings
      [ "@journalctl@" "@systemctl@" ]
      [ "${config.systemd.package}/bin/journalctl" "${config.systemd.package}/bin/systemctl" ]
      (builtins.readFile ./saya-agent-gate.py)
  );
  sshKeys = import ../../lib/ssh-keys.nix;
in
{
  users.users.saya-agent = {
    isSystemUser = true;
    group = "saya-agent";
    extraGroups = [ "systemd-journal" ];
    # sshd runs the forced command through the login shell.
    shell = pkgs.bashInteractive;
    openssh.authorizedKeys.keys = map (k: ''restrict,command="${gate}" ${k}'') sshKeys.saya-agent;
  };
  users.groups.saya-agent = { };
}
