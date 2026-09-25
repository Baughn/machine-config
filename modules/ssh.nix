{ lib, pkgs, ... }:

{
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "no";
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      X11Forwarding = false;
      MaxAuthTries = 20;
      LoginGraceTime = 30;
      ClientAliveInterval = 300;
      ClientAliveCountMax = 2;
    };
  };

  # OpenSSH prefers chacha20-poly1305 for the sake of CPUs without AES
  # instructions; every machine here has them, and AES-GCM is ~45% faster per
  # stream (measured saya->tsugumi, Sept 2026). Chacha stays as a fallback.
  programs.ssh.ciphers = [
    "aes128-gcm@openssh.com"
    "aes256-gcm@openssh.com"
    "chacha20-poly1305@openssh.com"
  ];

  programs.ssh.askPassword = lib.mkForce "${pkgs.x11_ssh_askpass}/libexec/x11-ssh-askpass";
}
