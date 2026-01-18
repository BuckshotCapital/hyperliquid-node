# hyperliquid-node

Containerised Hyperliquid node meant primarily for RPC usage

Join the discussion: [@buckshotcapitaltech](https://t.me/buckshotcapitaltech)

## What?

Turns out hl-node & hl-visor have quite many assumptions about its runtime environment, which might not always be true on containerised workload.
This sets up everything needed to run an RPC node in Europe or Japan regions.

### hl-bootstrap

Container image ships with hl-bootstrap utility to help setting up hl-node with necessary configuration to
sync with the network reliably, ensuring it has reasonable peers set up.

Features:
- Enforces correct network name check
- Sets up non-validating peer IPs for gossip from known source
  - Requests gossip IPs via `{"type": "gossipRootIps"}` method from Hyperliquid API & uses [hyperliquid-dex/node README.md](https://github.com/hyperliquid-dex/node/blob/main/README.md#mainnet-non-validator-seed-peers) to extract possible non-validator seed peers for mainnet
  - Uses [Imperator](https://www.imperator.co/)'s peers json endpoint for testnet
  - Measures, filters and orders obtained seed peers by latency (default threshold is 80ms to avoid cross-continent connections)
- Checks for common runtime environment misconfigurations
  - IPv6 enabled check (see [notes](notes.md))

## Running

Build or obtain the image from [ghcr.io](https://github.com/BuckshotCapital/hyperliquid-node/pkgs/container/hyperliquid-node) (use either `mainnet` or `testnet` tag), run with binding 4000-4010/tcp to public interface. Hyperliquid RPC will be exposed on port 3001, serving both /evm and /info endpoints.

See also example [compose.yaml](compose.yaml)

### Overriding example compose.yaml entries

Easiest way to make changes to [compose.yaml](compose.yaml) is to create e.g. `compose.local.yaml`,
set environment variable `COMPOSE_FILE=compose.yaml:compose.local.yaml` and specify your overrides there.

Verify overrides using `docker compose config`

Note that `--serve-info` is required if you want Prometheus metrics from hl-bootstrap.

```yaml
---
services:
  node:
    command:
      - "run-non-validator"
      - "--batch-by-block"
      - "--disable-output-file-buffering"
      - "--serve-eth-rpc"
      - "--serve-info"
      - "--write-fills"
      - "--write-hip3-oracle-updates"
      - "--write-misc-events"
      - "--write-order-statuses"
      - "--write-system-and-core-writer-actions"
      - "--write-trades"
    deploy:
      resources:
        limits:
          cpus: "8"
          memory: "34G"
          pids: 1024

volumes:
  node-data:
    driver: "local"
    driver_opts:
      type: "none"
      o: "bind"
      device: "/home/user/hl-data"
```

There are other methods to accomplish this as well - see https://docs.docker.com/compose/how-tos/multiple-compose-files/extends/
