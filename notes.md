# Running hl mainnet node

## DISABLE IPv6

```
thread 'tokio-runtime-worker' panicked at /home/ubuntu/hl/code_Mainnet/node/src/node.rs:487:6:
Could not parse home public ip: sleep_retry retried home_node_public_ip for sleep times [Duration(1.0), Duration(2.0), Duration(4.0)] last err invalid IPv4 address syntax
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```
## Useful files

- `/data/hl/hyperliquid_data/visor_abci_state.json`
```
{
  "initial_height": 622175000,
  "height": 628178000,
  "scheduled_freeze_height": null,
  "consensus_time": "2025-06-13T19:59:46.930709602"
}
```

- `/data/hl/data/node_logs/gossip_rpc/hourly/**/*`
    - Handled
- `/data/hl/data/node_logs/gossip_connections/hourly/**/*`
    - Handled
- `/data/hl/data/visor_child_stderr/(?<ymd>[0-9]+)/(?<hardfork_version>.+)/*`
    - Handled
- `/data/hl/data/node_logs/validator_connections/hourly/**/*`
    - Haven't seen anything happening there


## Pruning

Finding files older than 4 hours to delete, skipping visor stderr logs (as they're kept longer and node usually runs longer than 4h):
- `find /data/hl/data -mindepth 1 -depth -mmin +240 -type f -not -name "visor_child_stderr"`
