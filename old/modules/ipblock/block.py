#!/usr/bin/env python3
#
# Reads blocklist.txt and acceptlist.txt, then creates block (all) and accept (ssh) ipsets.
#
# Usage: python3 block.py

import glob
import os
import sys
import subprocess

# Change to the directory of this script
os.chdir(os.path.dirname(os.path.abspath(__file__)))

# Check if the script is being run as root
if os.geteuid() != 0:
    print("This script must be run as root.")
    sys.exit()

# Get the locations of ipset and iptables from argv.
# Because nixos.
IPTABLES = sys.argv[1]
IPSET = sys.argv[2]

# Delete the ipsets, if they exist
for s in ("blocklist", "acceptlist"):
    subprocess.call([IPSET, "destroy", s])

# Create the ipset
subprocess.call([IPSET, "create", "blocklist", "hash:net"])
subprocess.call([IPSET, "create", "acceptlist", "hash:net"])

# Add all the IPs from the txt files to the ipsets
for s in ("blocklist", "acceptlist"):
    with open(s + ".txt") as f:
        for line in f:
            subprocess.call([IPSET, "add", s, line.strip()])
# Also add private networks to the acceptlist
for line in ("10.0.0.0/8", "192.168.0.0/16"):
    subprocess.call([IPSET, "add", "acceptlist", line])


# Drop all packets from the blocklist
subprocess.call([IPTABLES, "-D", "INPUT", "-m", "set", "--match-set", "blocklist", "src", "-j", "DROP"])
subprocess.call([IPTABLES, "-I", "INPUT", "-m", "set", "--match-set", "blocklist", "src", "-j", "DROP"])

# Drop packets to SSH other than from the acceptlist
subprocess.call([IPTABLES, "-D", "INPUT", "-p", "tcp", "--dport", "22", "-m", "set", "!", "--match-set", "acceptlist", "src", "-j", "DROP"])
subprocess.call([IPTABLES, "-I", "INPUT", "-p", "tcp", "--dport", "22", "-m", "set", "!", "--match-set", "acceptlist", "src", "-j", "DROP"])
