{ config, lib, pkgs, ... }:

let
  cfg = config.me.sshAuth;
in
{
  options.me.sshAuth.enable = lib.mkEnableOption "OTP-gated SSH password auth (Google Authenticator)";

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ pkgs.google-authenticator ];

    # modules/ssh.nix hard-disables password + keyboard-interactive; flip back on for OTP path.
    services.openssh.settings = {
      KbdInteractiveAuthentication = lib.mkForce true;
      PasswordAuthentication = lib.mkForce true;
      AuthenticationMethods = "publickey keyboard-interactive:pam";
      ChallengeResponseAuthentication = true;
    };

    security.pam.services.sshd.googleAuthenticator = {
      enable = true;
      allowNullOTP = false;
      forwardPass = false;
    };
  };
}
