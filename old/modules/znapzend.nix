{
  config,
  pkgs,
  ...
}: {
  services.znapzend = {
    enable = true;
    autoCreation = true;
    pure = true;
    zetup = {
      "rpool/home" = {
        plan = "1d=>15min,1w=>1h";
        recursive = true;
        destinations.tsugumi = {
          host = "znapzend@brage.info";
          dataset = "stash/backups/${config.networking.hostName}/home";
          plan = "1w=>1h,12w=>1d";
        };
      };
    };
  };
}
