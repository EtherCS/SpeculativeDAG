## Evaluation under network jitter

```
cd mysticeti/scripts
bash jitterrun.sh 30 4 1 2 100 15
```
It will run 4 nodes with 1 node experiencing jitter (randomly choosing 2 connections and add 100 ms delay) for 15 seconds. The evaluation will run 30 seconds.