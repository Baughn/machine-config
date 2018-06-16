# This file defines what machines are available, and what 'containers' (which
# have nothing to do with Docker or LXC) they are running. It also tells Disnix
# how to use them: The attributes set here are descriptive, not prescriptive.
#
# Disnix is smart enough that doing it this way isn't necessary, if I link it to
# the NixOS configuration. Haven't yet.
#
# See tsugumi-config.nix.

{
  tsugumi = {
    properties = {
      hostname = "brage.info";
    };

    containers = {
      apache-webapplication = {
        apachePort = 3300;
      };

      mysql-database = {
        mysqlPort = 3306;
        mysqlUsername = "root";
        mysqlPassword = builtins.readFile ../../secrets/mysql-pw;
      };
    };

    numOfCores = 4;
  };
}
