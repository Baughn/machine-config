#!/bin/bash

set -ue

if [[ `pwd` != "$HOME/dotfiles" ]]; then
  echo Wrong location
  exit 1
fi

PACKAGES=(
  # XMonad deps & desktop experience
  compton dmenu haskell-platform xscreensaver
  redshift trayer yakuake xmobar workrave
  libghc-xmonad-contrib-dev xmonad
  # Desktop applications
  ark kdiff3 gwenview gimp
  # KDE deps
  kde-plasma-desktop plasma-nm
  # Music!
  mpc mpd mpv ncmpcpp pavucontrol alsa-utils
  # IRC
  znc irssi
  # Utilities
  moreutils parallel stow fortune sshfs
  mosh atop iotop fortunes git
)

sudo apt install -y "${PACKAGES[@]}"

for i in *; do
  if [[ "$i" != 'install.sh' ]]; then
    stow "$i"
  fi
done

xmonad --recompile

echo "Now you may want to enable backups."
