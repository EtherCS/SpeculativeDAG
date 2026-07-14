'grafana-dashboard-old.json' might record inaccuracy commit latency. It records **the average of each replica’s latest one-second p99 latency**. It works if the latency is stable or the Prometheus scrapes is frequent.
'''
'''

We use 'grafana-dashboard.json' for metrics collection in attack evaluation. It records **real-time rolling-window p99 commit latency**.