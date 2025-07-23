#!/usr/bin/env bash

exec hl-bootstrap --override-gossip-config-path=/data/override_gossip_config.json \
    -- hl-visor run-non-validator --write-trades --write-fills --write-order-statuses --serve-eth-rpc --serve-info
