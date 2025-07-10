{
  config,
  pkgs,
  ...
}: {
  networking.firewall.allowedTCPPorts = [873];
  services.rsyncd = {
    enable = true;
    motd = ''      Welcome to Factorial Productions

          By accessing this site on the first day of the fourth month of the year 2018
          Anno Domini, you agree to grant Us a non transferable option to claim, for
          now and for ever more, your immortal soul. Should We wish to exercise this
          option, you agree to surrender your immortal soul, and any claim you may
          have on it, within 5 (five) working days of receiving written notification
          from "Baughn", or one of its duly authorised minions.

    '';
    modules = let
      module = config: ({
          "read only" = "yes";
          "use chroot" = "true";
          "uid" = "nobody";
          "gid" = "nobody";
        }
        // config);
    in {
      factorio = module {
        comment = "Factorio";
        path = "/home/svein/rsync/factorio";
      };
      incoming = module {
        comment = "Drop box";
        path = "/home/svein/rsync/incoming";
        "read only" = "false";
        "write only" = "true";
      };
    };
  };
}
