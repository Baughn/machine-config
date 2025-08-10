{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.me.sshAuth;
in
{
  options.me.sshAuth = {
    enable = mkOption {
      type = types.bool;
      default = true;
      description = "Enable enhanced SSH authentication with OTP for password logins";
    };

    requireOTPForPassword = mkOption {
      type = types.bool;
      default = true;
      description = "Require OTP (Google Authenticator) for password-based SSH authentication";
    };
  };

  config = mkIf cfg.enable {
    # Install Google Authenticator PAM module and command-line tool
    environment.systemPackages = with pkgs; [
      google-authenticator
    ];

    # Configure SSH service
    services.openssh = {
      enable = true;
      settings = {
        # Enable keyboard-interactive authentication for OTP prompts
        KbdInteractiveAuthentication = true;
        # Keep password authentication enabled (will be combined with OTP via PAM)
        PasswordAuthentication = true;
        # Ensure public key authentication is enabled (default)
        PubkeyAuthentication = true;
        # Configure authentication methods:
        # - First try public key authentication (no OTP required)
        # - If that fails, use keyboard-interactive with PAM (password + OTP)
        AuthenticationMethods = "publickey keyboard-interactive:pam";
        # Enable challenge-response authentication for PAM
        ChallengeResponseAuthentication = true;
      };
    };

    # Configure PAM for SSH with Google Authenticator
    security.pam.services.sshd = mkIf cfg.requireOTPForPassword {
      # Enable Google Authenticator module
      googleAuthenticator = {
        enable = true;
        # Allow users without OTP configured to still log in with just password
        # Set to false to require OTP for all password logins
        allowNullOTP = false;
        # Don't use forward pass - we want separate password and OTP prompts
        forwardPass = false;
      };
    };

    # Note: The PAM configuration will automatically be set up to:
    # 1. Skip OTP for public key authentication (handled by SSH before PAM)
    # 2. Require OTP for password authentication (via keyboard-interactive)
    #
    # Users need to run 'google-authenticator' to set up their OTP:
    # - This creates ~/.google_authenticator with the secret key
    # - Shows a QR code to scan with an authenticator app
    # - Provides emergency scratch codes
  };
}
