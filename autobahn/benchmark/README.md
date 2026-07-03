# Autobahn Benchmark Guide

This benchmark harness can run Autobahn locally with or without EVM execution enabled, collect per-node logs, and print an aggregate performance summary.

## Quick Start

Run the benchmark from the `autobahn/benchmark` directory:

```bash
fab local
```

This command:

1. cleans old benchmark state,
2. recompiles the node with the `benchmark` feature,
3. launches clients, primaries, and workers in tmux sessions,
4. waits for the configured benchmark duration,
5. parses the generated logs and prints a summary.

## Common Commands

Run the default ordered execution test on the ERC20 workload:

```bash
fab local --execution=ordered --workload=erc20
```

Run speculative execution on Uniswap with the parallel PEVM executor:

```bash
fab local --execution=speculative --workload=uniswap --executor=parallel
```

Run without EVM execution:

```bash
fab local --execution=none
```

Print the summary for the latest local run:

```bash
fab logs
```

## Parameters

`fab local` currently accepts these parameters:

- `debug`: enable more verbose node logging. Default: `True`.
- `execution`: EVM execution mode. Supported values: `none`, `ordered`, `speculative`.
- `workload`: workload used by the benchmark client. Supported values: `erc20`, `weth`, `uniswap`.
- `executor`: PEVM executor mode. Supported values: `sequential`, `parallel`.
- `artifacts`: directory containing workload artifacts. Default: `.`.
- `num_clusters`: synthetic account/workload generation parameter. Default: `5`.
- `families_per_cluster`: synthetic account/workload generation parameter. Default: `5`.
- `people_per_family`: synthetic account/workload generation parameter. Default: `8`.

The benchmark configuration embedded in `fabfile.py` currently uses:

- `4` nodes
- `1` worker per node
- `400` tx/s total input rate
- `512` bytes per transaction
- `60` seconds benchmark duration
- `0` faults

## Log Layout

Each benchmark run writes logs into its own folder:

```text
autobahn/benchmark/logs/[execution]-[workload]-[executor]-[timestamp]/
```

Example:

```text
autobahn/benchmark/logs/speculative-uniswap-sequential-20260703-153012/
```

Each run directory contains:

- `primary-*.log`: primary node logs
- `worker-*-*.log`: worker logs
- `client-*-*.log`: benchmark client logs
- `latencies.txt`: sampled end-to-end latency records written by the parser

`fab logs` parses the latest run folder under `autobahn/benchmark/logs`.

## Reading the Summary

The printed summary contains:

- `Faults`, `Committee size`, `Worker(s) per node)`: deployment shape
- `Execution`, `Workload`, `Executor`: the execution configuration used for this run
- `Input rate`, `Transaction size`, `Execution time`: client-side benchmark settings
- `Header size`, `Max header delay`, `GC depth`, `Sync retry delay`, `Sync retry nodes`, `Batch size`, `Max batch delay`: protocol configuration parsed from the node logs
- `Consensus TPS/BPS/latency`: ordering-layer throughput and latency
- `End-to-end TPS/BPS/latency`: client-visible throughput and latency

The parser measures end-to-end latency using sampled transactions that are logged by the clients and matched against the batches observed by the workers and the commits observed by the primaries.

## Notes

- `fab local` suppresses compiler warnings during the build step to keep benchmark output readable.
- If you want to inspect raw process output while a benchmark is running, use `tmux ls` and open the corresponding session.
