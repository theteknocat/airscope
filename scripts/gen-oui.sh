#!/bin/sh
# Regenerate the built-in vendor list from an IEEE oui.txt (https://standards-oui.ieee.org/oui/oui.txt).
# Usage: scripts/gen-oui.sh [oui.txt]   (default: /usr/share/hwdata/oui.txt)
awk -F'\t+' '/\(hex\)/ { split($1, a, " "); gsub("-", "", a[1]); sub(/\r$/, "", $2); print a[1] "\t" $2 }' "${1:-/usr/share/hwdata/oui.txt}" | LC_ALL=C sort -u > "$(dirname "$0")/../src/oui.tsv"
