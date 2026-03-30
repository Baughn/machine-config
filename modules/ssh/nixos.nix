{ lib, pkgs, ... }:

{
  services.openssh.enable = true;
  security.sudo.wheelNeedsPassword = false;

  programs.ssh.askPassword = lib.mkForce "${pkgs.x11_ssh_askpass}/libexec/x11-ssh-askpass";
}
