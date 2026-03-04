## Evaluation under network jitter

Under the network jitter condition, we assume nodes fail to perform the direct rules for a while, leading to backlog blocks that cannot be decided. 

To simulate such failure conditions (i.e., the direct rules failed to be applied), in our evaluation, we add some latency between some connections among nodes, such that a leader might delay sending its blocks to *m* nodes (where *n/3 < m < 2n/3* to avoid the direct commit or skip rule).

```
cd mysticeti/scripts
bash jitterrun.sh 
# with parameters [running_time] [committee_size] [jitter_node_num] [delay_connection_num] [delay_per_connection_in_millisecond] [jitter_start_time] [jitter_duration_in_seconds]
```
For example, by executing:
```
bash jitterrun.sh 90 7 2 4 2500 10 50
```

It will run 7 nodes with 2 nodes that experiences jitter (randomly choosing 4 connections and add 2500 ms delay) for 50 seconds after the node is running 10 seconds. The evaluation will run 90 seconds.
> Note that to guarantee the failure of direct decision, the delay should be set larger than the leader timeout (2s by default).

```
2026-01-22T21:49:35.798548Z DEBUG mysticeti_core::core: Created block A61:[A60,D60,B60,G60,E60,](statements(0))

2026-01-22T21:49:35.802359Z DEBUG try_commit{last_decided=E53}: mysticeti_core::consensus::universal_committer: Decided Skip(F54)

2026-01-22T21:49:35.802417Z DEBUG try_commit{last_decided=E53}: mysticeti_core::consensus::universal_committer: Decided Skip(G55)

2026-01-22T21:49:35.802449Z DEBUG try_commit{last_decided=E53}: mysticeti_core::consensus::universal_committer: Decided Commit(A56)

2026-01-22T21:49:35.802478Z DEBUG try_commit{last_decided=E53}: mysticeti_core::consensus::universal_committer: Decided Commit(B57)

2026-01-22T21:49:35.802527Z DEBUG try_commit{last_decided=E53}: mysticeti_core::consensus::universal_committer: Decided Commit(C58)
```

Above is the log, showing that many leaders fail to be directly decided; instead, they are decided indirectly from C58.

## Plot the latency figure
The latency data is stored in `*-validator-*/storage-*/latency.csv`, to plot the latency figure, run
```
python3 plot_latency.py --csv [latency file] --output [output name]
# E.g., python3 plot_latency.py --csv jitterrun-validator-0/storage-0/latency.csv --output jitter_latency.pdf
```