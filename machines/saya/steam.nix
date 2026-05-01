{ ... }:

{
  programs.steam.enable = true;

  # Steam launches transient app-steam@<id>.scope user units that fail routinely as
  # games exit; without this filter every shell prompt would flag them as failures.
  me.shell.userFailedUnitsExclude = [ "^app-steam[@-]" ];
}
