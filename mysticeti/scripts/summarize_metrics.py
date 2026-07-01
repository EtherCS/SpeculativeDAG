#!/usr/bin/env python3

import csv
import glob
import os
import re
import sys


LINE_RE = re.compile(r'^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{([^}]*)\})?\s+([0-9eE+.\-]+)$')


def parse_labels(raw):
    labels = {}
    if not raw:
        return labels
    for item in raw.split(","):
        if "=" not in item:
            continue
        key, value = item.split("=", 1)
        labels[key.strip()] = value.strip().strip('"')
    return labels


def parse_metrics(path):
    rows = []
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            match = LINE_RE.match(line)
            if not match:
                continue
            name, _, raw_labels, value = match.groups()
            rows.append((name, parse_labels(raw_labels), float(value)))
    return rows


def main():
    if len(sys.argv) != 2:
        print("usage: summarize_metrics.py <result-dir>", file=sys.stderr)
        sys.exit(1)

    result_dir = sys.argv[1]
    metric_files = sorted(glob.glob(os.path.join(result_dir, "validator-*.metrics")))
    if not metric_files:
        print("run,validator,metric,labels,value")
        return

    run_name = os.path.basename(os.path.abspath(result_dir))
    writer = csv.writer(sys.stdout)
    writer.writerow(["run", "validator", "metric", "labels", "value"])

    interesting = {
        "transaction_committed_latency",
        "block_execution_latency",
        "block_consensus_latency",
        "submitted_transactions",
        "speculative_messages_total",
        "speculative_predictions_total",
        "speculative_snapshot_total",
        "speculative_execution_leaders_total",
        "speculative_prefix_matched_leaders_total",
        "speculative_reexecuted_leaders_total",
        "speculative_snapshot_window_size",
        "speculative_snapshot_store_size",
    }

    for metric_file in metric_files:
        validator = os.path.splitext(os.path.basename(metric_file))[0]
        for name, labels, value in parse_metrics(metric_file):
            if name not in interesting:
                continue
            label_str = ";".join(f"{k}={labels[k]}" for k in sorted(labels))
            writer.writerow([run_name, validator, name, label_str, value])


if __name__ == "__main__":
    main()
