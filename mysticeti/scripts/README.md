## Fast test

```
cd mysticeti/scripts
bash speculative.sh 4 60
```

It will run 4 nodes for 60s locally. After the process is end, the (latency) metrics can be found in `./scripts/log*.txt` as follows:
```
block_execution_latency{v="count"} 6570
block_execution_latency{v="p50"} 530
block_execution_latency{v="p90"} 2848
block_execution_latency{v="p99"} 5233
block_execution_latency{v="sum"} 6989851
...
transaction_committed_latency{v="count"} 47200
transaction_committed_latency{v="p50"} 62850
transaction_committed_latency{v="p90"} 82400
transaction_committed_latency{v="p99"} 752120
transaction_committed_latency{v="sum"} 3690243727
```