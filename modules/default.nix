{ pkgs, ... }:

{
  imports = [
    ./zsh.nix
    ./desktop.nix
    ./nvidia.nix
  ];

  # Software that I use virtually everywhere
  environment.systemPackages = with pkgs; [
    neovim
    wget
    restic
    sshfs
    jujutsu
    nodejs
    git
    rustup
    ripgrep
    fd
  ];
}
