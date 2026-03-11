## Simulate the Mysticeti under network jitter

```
cd mysticeti/scripts
bash jitterrun.sh 90 4 1 2 2500 10 20 10
```

Then, run the following command to get the latency figure:
```
python3 plot_latency.py --csv jitterrun-validator-0/storage-0/latency.csv --output bl_jitter_latency.pdf
```