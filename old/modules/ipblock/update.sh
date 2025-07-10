# #!/usr/bin/env bash
# Updates the IP block list.

set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

BLOCK=(ru hk cn)
ACCEPT=(no ie)

# Download the latest block lists.
rm -f *.zone
for country in "${BLOCK[@]}" "${ACCEPT[@]}"; do
  wget https://www.ipdeny.com/ipblocks/data/aggregated/${country}-aggregated.zone
done

rm -f blocklist.txt acceptlist.txt
for country in "${BLOCK[@]}"; do
  cat "${country}-aggregated.zone" >> blocklist.txt
done
for country in "${ACCEPT[@]}"; do
  cat "${country}-aggregated.zone" >> acceptlist.txt
done

# Include google, 'coz work.
echo '34.64.0.0/10' >> acceptlist.txt
echo '35.192.0.0/12' >> acceptlist.txt
