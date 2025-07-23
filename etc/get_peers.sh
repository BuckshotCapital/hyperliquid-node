#!/usr/bin/env bash
set -euo pipefail

chain="Mainnet"
try_new_peers="true"

curl -s -L https://github.com/hyperliquid-dex/node/raw/refs/heads/main/README.md | awk '
/```/ { 
    if (in_block && found) exit
    in_block = !in_block
    next
}
in_block && !found && /^operator_name,root_ips/ { 
    found = 1
    next  # Skip the header line
}
in_block && found { print }
' | jq --argjson try "${try_new_peers}" --arg chain "${chain}" -ceR '[inputs | select(length > 0) | split(",")[1] | {Ip: .}] as $ips | { root_node_ips: $ips, try_new_peers: $try, chain: $chain }'
