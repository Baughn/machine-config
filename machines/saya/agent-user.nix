# The saya agent's Unix user and unit sandbox, shared with tests/agent-channel-vm.nix
# so the VM test checks the real settings. See "The saya identity" in
# docs/agent-channel-design.md.
{
  user = "saya-agent";
  home = "/home/saya-agent";
  workdir = "/home/saya-agent/agent";
  # Where its commits leave: a git bundle, which Baughn fetches as a jj remote.
  outbox = "/home/saya-agent/outbox";

  users = {
    # Not in wheel, so neither sudo nor a trusted Nix user; no password or keys.
    users.saya-agent = {
      isSystemUser = true;
      group = "saya-agent";
      home = "/home/saya-agent";
      createHome = true;
      homeMode = "711";
    };
    groups.saya-agent = { };
  };

  serviceConfig = {
    NoNewPrivileges = true;
    # Its own home only; this also hides /run/user (session buses, Wayland, xauth).
    ProtectHome = "tmpfs";
    BindPaths = [ "/home/saya-agent" ];
    PrivateTmp = true;
    PrivateIPC = true;
  };
}
