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

Run a jitter/asynchrony evaluation with the helper script:

```bash
bash jitterrun.sh 60 4 1 400 10 50 ordered erc20 sequential
```

Run the deterministic consecutive order-stall attack:

```bash
fab attack --execution=ordered --workload=erc20 \
    --order-stall-start=10000 --order-stall-duration=50000 \
    --duration=60
```

Run the same attack on the configured remote testbed:

```bash
fab remote-attack --execution=speculative --workload=erc20 \
    --order-stall-start=10000 --order-stall-duration=50000 \
    --duration=60
```

During `[order_stall_start, order_stall_start + order_stall_duration)`, primaries withhold
outgoing `Prepare` consensus requests. The normal timeout and view-change protocol then runs;
the attack does not mark a consensus instance as undecided. At the end of the interval, the latest
still-current prepare for each slot is released and stale prepares are discarded.

## Parameters

`fab local` and `fab remote-attack` accept the common workload, execution,
executor, committee, rate, duration, and attack timing parameters listed below.
`fab remote` remains the asynchronous remote benchmark, while
`fab remote-attack` enables the deterministic order-stall attack automatically.

`fab local` currently accepts these parameters:

- `debug`: enable more verbose node logging. Default: `True`.
- `execution`: EVM execution mode. Supported values: `none`, `ordered`, `speculative`.
- `workload`: workload used by the benchmark client. Supported values: `erc20`, `weth`, `uniswap`.
- `executor`: PEVM executor mode. Supported values: `sequential`, `parallel`.
- `artifacts`: directory containing workload artifacts. Default: `.`.
- `num_clusters`: synthetic account/workload generation parameter. Default: `5`.
- `families_per_cluster`: synthetic account/workload generation parameter. Default: `5`.
- `people_per_family`: synthetic account/workload generation parameter. Default: `8`.
- `faults`: faulty nodes omitted from the local deployment. Default: `0`.
- `nodes`: committee size for local benchmarking. Default: `4`.
- `workers`: workers per node. Default: `1`.
- `rate`: total input rate in tx/s. Default: `400`.
- `tx_size`: transaction size in bytes. Default: `512`.
- `duration`: benchmark duration in seconds. Default: `60`.
- `runs`: number of benchmark repetitions encoded in the config. Default: `1`.
- `simulate_asynchrony`: enable the built-in Autobahn asynchrony simulation. Default: `False`.
- `asynchrony_start`: asynchrony start time in milliseconds after slot 1 commits. Default: `15000`.
- `asynchrony_duration`: asynchrony duration in milliseconds. Default: `3000`.
- `simulate_order_stall`: enable deterministic consecutive order-stall behavior. Default: `False`.
- `order_stall_start`: attack start time in milliseconds after primary startup. Default: `10000`.
- `order_stall_duration`: attack duration in milliseconds. Default: `50000`.

The dedicated `fab jitter` task enables `simulate_asynchrony=True` automatically and defaults to:

- `execution=ordered`
- `workload=erc20`
- `executor=sequential`
- `asynchrony_start=10000`
- `asynchrony_duration=50000`

The dedicated `fab attack` task enables `simulate_order_stall=True` automatically and defaults to:

- `execution=ordered`
- `workload=erc20`
- `executor=sequential`
- `order_stall_start=10000`
- `order_stall_duration=50000`

The dedicated `fab remote-attack` task has the same attack defaults as `fab attack`,
but starts the benchmark through the configured remote testbed. It writes the
retrieved remote logs and the parsed summary using the same remote benchmark
pipeline as `fab remote`.

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
autobahn/benchmark/logs/[experiment]-[execution]-[workload]-[executor]-[timestamp]/
```

Example:

```text
autobahn/benchmark/logs/jitter-ordered-erc20-sequential-20260705-101530/
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
- `Simulated asynchrony`, `Asynchrony start`, `Asynchrony duration`: whether the jitter/asynchrony mode was enabled and when it ran
- `Order-stall attack`, `Order-stall start`, `Order-stall duration`: whether the deterministic attack was enabled and its interval
- `Input rate`, `Transaction size`, `Execution time`: client-side benchmark settings
- `Header size`, `Max header delay`, `GC depth`, `Sync retry delay`, `Sync retry nodes`, `Batch size`, `Max batch delay`: protocol configuration parsed from the node logs
- `Consensus TPS/BPS/latency`: ordering-layer throughput and latency
- `End-to-end TPS/BPS/latency`: client-visible throughput and latency

The parser measures end-to-end latency using sampled transactions that are logged by the clients and matched against the batches observed by the workers and the commits observed by the primaries.

## Notes

- `fab local` suppresses compiler warnings during the build step to keep benchmark output readable.
- If you want to inspect raw process output while a benchmark is running, use `tmux ls` and open the corresponding session.
- `jitterrun.sh` stores a human-readable summary in `results/.../summary.txt` and symlinks the matching raw logs directory into `results/.../logs`.

## Evaluation Matrices

Run the local ordered/speculative matrix from this directory:

```bash
bash run_local_matrix.sh
```

Run the corresponding consecutive order-stall attack matrix:

```bash
bash run_attack_matrix.sh
```

Each script evaluates both execution modes, all three workloads (`erc20`, `weth`, and `uniswap`),
rates `400` and `4000`, and committee sizes `4` and `10`, with `runs=2`. The attack matrix uses
`order_stall_start=1000` and `order_stall_duration=40000`; other Fabric defaults are unchanged.

Each matrix writes all Fabric output, including `print(ret.result())`, to one timestamped log file.
Set `LOG_FILE=/path/to/output.log` to choose a different file.
