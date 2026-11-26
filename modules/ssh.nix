{ lib, pkgs, ... }:

let
  # A leading '-' removes the matching algorithms from OpenSSH's default set.
  noRsa = "-ssh-rsa*,rsa-sha2-*";
in
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
      # No RSA anywhere (Baughn, Discord 1575621553968910419: rumours that
      # factoring is solved). Refuse RSA user keys, RSA host keys and
      # RSA-signed certificates, including the -cert variants.
      PubkeyAcceptedAlgorithms = noRsa;
      HostKeyAlgorithms = noRsa;
      CASignatureAlgorithms = noRsa;
      HostbasedAcceptedAlgorithms = noRsa;
      # Finite-field DH falls to the same number-field-sieve family as RSA;
      # every current client has curve25519 or ML-KEM.
      KexAlgorithms = [
        "mlkem768x25519-sha256"
        "sntrup761x25519-sha512"
        "sntrup761x25519-sha512@openssh.com"
        "curve25519-sha256"
        "curve25519-sha256@libssh.org"
      ];
    };
    # The NixOS default also generates and serves a 4096-bit RSA host key.
    hostKeys = [
      { path = "/etc/ssh/ssh_host_ed25519_key"; type = "ed25519"; }
      { path = "/etc/ssh/ssh_host_mldsa44_ed25519_key"; type = "mldsa44-ed25519"; }
    ];
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

  programs.mosh.enable = true;
  # Reviewed mosh 1.4.0 (Sept 2026): nothing short of the session key reaches
  # a parser. The one pre-auth issue is a DoS (oversized datagrams make
  # mosh-server sleep in spin()), accepted. Receive path fuzzed against
  # master, clean before authentication (~/dev/mosh, src/fuzz).
  programs.mosh.openFirewall = true;
}
