{infrastructure}:

# Where each application should go, on our set of machines.
#
# Careful: In this current configuration, neither Apache nor MariaDB are
# configured on-demand. That's a possibility, but right now they're statically
# enabled through the tsugumi NixOS config.

with infrastructure;
{
  WikiDb = [ tsugumi ];
  Wiki = [ tsugumi ];
}
