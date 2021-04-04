#!/bin/sh
sudo encfs $HOME/nixos/secrets-encfs $HOME/nixos/secrets -S --public < $HOME/.nixos.secrets.pw
