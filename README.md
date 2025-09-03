# hyperliquid-node

Containerised Hyperliquid node meant primarily for RPC usage

## What?

Turns out hl-node & hl-visor have quite many assumptions about its runtime environment, which might not always be true on containerised workload.
This sets up everything needed to run an RPC node in Europe or Japan regions.

### hl-bootstrap

Container image ships with hl-bootstrap utility to help setting up hl-node with necessary configuration to
sync with the network reliably, ensuring it has reasonable peers set up.

Features:
- Enforces correct network name check
- Sets up non-validating peer IPs for gossip from known source
  - Requests gossip IPs via `{"type": "gossipRootIps"}` method from Hyperliquid API for mainnet
  - Uses [Imperator](https://www.imperator.co/)'s peers json endpoint for testnet
  - Measures, filters and orders obtained seed peers by latency (default threshold is 80ms to avoid cross-continent connections)
- Checks for common runtime environment misconfigurations
  - IPv6 enabled check (see [notes](notes.md))

## Running

Build the image, run with binding 4000-4010/tcp to public interface. Hyperliquid RPC will be exposed on port 3001, serving both /evm and /info endpoints.
