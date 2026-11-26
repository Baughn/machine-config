rec {
  svein = [
    "cert-authority ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGfsmAbJ1GKytVA71izC3xvIFYDQVHT2Q5CZPaIA6WqS svein@tsugumi"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMWPdtIOeHF3TElJxL8gQGyZMErJHY0OdqrRFZFlFdP0 svein@svein-mac.roam.internal"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICeiqQnylmCVUzTNNcYRWKp/38dB5i3aGBs7ZB11MjkS svein@kaho.local"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGppkBITukYVejPl3BiRmCDSfdrItzM59XpwwK7W/mXH svein@saya"
  ];

  # The saya agent's read-only login on tsugumi (machines/tsugumi/saya-agent.nix).
  saya-agent = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHIj6Cs5uJ+h6Z+l/pIZGoqLv42kpPbquWEfE/PYB8xT saya-agent@saya"
  ];

  # RSA keys removed 2026-11-26 (Baughn, Discord 1575621553968910419); sshd refuses RSA now.
  # Their owners need an ed25519 key to log in again: lridge@kirin.local, darqen@MAINCENTER, darqen27@freebsd, buizerd@localhost, prestonfenette@gmail.com, (no comment, mQkBu9zF/W8=), rsa-key-20161217, lucca@kuu, clever@nixos.
  minecraft = [
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPi0pCZqmvbObrDkAg28EBwt/hriKcCXRlEreexhoNJd bloxgate-ed25519-2018"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICGxbfR3P4jby0eDYiB1+/0uq1OvNQWXHTAET+YYlDNC"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIIv3vZpMdKR4VCF9jkVigFhRUId+iRjoKcTXPmdkpbAE vindex@big"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOghAMI7V97vZudyc9fl4csb8VOILUhvKgt8ebk29cJU lucca@atropos"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMaaKLw4S7s6RjvFoeN+fRNmmaUeSEvdqWzL/bUy5SaF jared@arch01d.ctha.ja4.org"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBpUHboM5fNLogflF/9EEklCgAvmE08L1lmT696UIwSW jared@arch01l.ctha.ja4.org"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG5nvmj3DlRnRJqsvdizUOMcQH71XQxew5jI6WW65Gpv jared@win01d.ctha.ja4.org"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKXD8KKr1XyV3aOsb9eeagSrLY3A5L1nPgXnLO6XpSwc maxwell.lt@maxwell-nixos"
  ] ++ svein;
}
