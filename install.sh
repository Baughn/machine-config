#!/usr/bin/env bash

set -ue

cd "$(dirname "$(readlink -f "$0")")"

if ! (uname -a | grep -q NixOS); then
  PACKAGES=(
    # XMonad deps & desktop experience
    compton dmenu haskell-platform xscreensaver
    redshift trayer yakuake xmobar workrave
    libghc-xmonad-contrib-dev xmonad
    gnome-screensaver xfonts-terminus
    # Desktop applications
    ark kdiff3 gwenview gimp konsole gnome-terminal xterm
    # KDE deps
    kde-plasma-desktop plasma-nm
    # Music!
    mpc mpd mpv ncmpcpp pavucontrol alsa-utils
    # IRC
    znc irssi
    # Utilities
    moreutils parallel stow fortune sshfs
    mosh atop iotop fortunes git vlock
    # OS debugging
    memtester smartmontools
    # CUDA and such
    libcupti-dev nvidia-cuda-doc nvidia-cuda-dev nvidia-cuda-toolkit
    nvidia-visual-profiler
    # Python
    python-pip python-dev python-virtualenv ipython
  )

  sudo apt install -y "${PACKAGES[@]}"

  for i in *; do
    if [[ "$i" != 'install.sh' ]]; then
      stow "$i"
    fi
  done

else

  for i in *; do
    if [[ "$i" != 'install.sh' ]]; then
      nix-shell -p stow --run "stow $i"
    fi
  done

fi

xmonad --recompile


echo "Now you may want to enable backups."
