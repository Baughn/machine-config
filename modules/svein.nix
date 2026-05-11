let
  sshKeys = import ../lib/ssh-keys.nix;
in
{
  users.users.svein = {
    isNormalUser = true;
    extraGroups = [ "wheel" ];
    openssh.authorizedKeys.keys = sshKeys.svein;
  };
}

