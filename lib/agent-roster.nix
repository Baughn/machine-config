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
  # Other webhooks whose messages are context, like the watchdog's.
  contextWebhookIds = [
    "1549730422131527760" # "Crash analysis": crash_analysis.py's notice (URL in ~minecraft/.config)
  ];
  humans = {
    baughn = { discordId = "236112302590394368"; role = "owner"; };
  };
  agents = {
    tsugumi-minecraft = {
      discordId = "1553405654218182708";
      displayName = "ErisiAgent"; # the bot's Discord username
      description = "Runs as `minecraft` on tsugumi; operates the live servers.";
    };
    tsugumi-lab = {
      discordId = "1553406246269489152";
      displayName = "LabAgent";
      description = "Runs as `mclab` on tsugumi; experiments on ZFS clones of worlds.";
    };
    saya = {
      discordId = "1553406970164281525";
      displayName = "Saya";
      description = "Runs as `saya-agent` on saya; edits machine-config; acts only for Baughn.";
    };
    tsugumi-sec = {
      discordId = "1558449510089359371";
      displayName = "Garibaldi";
      description = "Runs as `tsugumi-sec` on tsugumi; daily security watch over tsugumi's network-facing code, sandboxed.";
      # It reads hostile text all day: its messages are context for every bridge, never triggers.
      contextOnly = true;
    };
    saya-client = {
      discordId = "1555949975672987688";
      displayName = "AdventurAgent";
      description = "Runs as `saya-client` on saya; tests client-side fixes with a headless Minecraft client against lab servers.";
    };
  };
}
