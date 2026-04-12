let
  # Host keys — each machine's /etc/ssh/ssh_host_ed25519_key.pub
  saya = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHkaJd61/WV8hrah8wsuuTVmTBM4JsU1UWJMQyABaHVY root@saya";

  # User keys — for encrypting secrets during development
  svein = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGppkBITukYVejPl3BiRmCDSfdrItzM59XpwwK7W/mXH svein@saya";

  allKeys = [ svein saya ];
in
{
  "test.age".publicKeys = allKeys;
}
