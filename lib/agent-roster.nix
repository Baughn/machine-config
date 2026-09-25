# Who is who in the agent channel. None of this is secret: Discord IDs are
# visible to every guild member. See docs/agent-channel-design.md.
# Agent discordIds are the bots' user IDs, which equal their application IDs.
{
  guildId = "153634590190206977";
  channels = {
    main = "1553121660532432926"; # #auto-admin
    test = "1553402794197655582";
  };
  adminRoleId = "280158066195038208"; # Discord role that marks server admins
  watchdogWebhookId = "1553401745449812120"; # its messages are context, never triggers
  humans = {
    baughn = { discordId = "236112302590394368"; role = "owner"; };
  };
  agents = {
    tsugumi-minecraft = {
      discordId = "1553405654218182708"; # ErisiAgent
      description = "Runs as `minecraft` on tsugumi; operates the live servers.";
    };
    tsugumi-lab = {
      discordId = "1553406246269489152"; # LabAgent
      description = "Runs as `mclab` on tsugumi; experiments on ZFS clones of worlds.";
    };
    saya = {
      discordId = "1553406970164281525"; # Saya
      description = "Baughn's agent on saya; edits machine-config; acts only for Baughn.";
    };
  };
}
