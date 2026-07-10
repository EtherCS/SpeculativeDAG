#!/usr/bin/env python3

import csv
import glob
import os
import re
import sys
from collections import defaultdict


LINE_RE = re.compile(r'^([a-zA-Z_:][a-zA-Z0-9_:]*)(\{([^}]*)\})?\s+([0-9eE+.\-]+)$')
TIMESTAMP_SUFFIX_RE = re.compile(r'-\d{8}-\d{6}$')


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


def parse_run_meta(path):
    meta = {}
    if not os.path.exists(path):
        return meta
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line or "=" not in line:
                continue
            key, value = line.split("=", 1)
            meta[key.strip()] = value.strip()
    return meta


def canonical_group_name(run_name):
    return TIMESTAMP_SUFFIX_RE.sub("", run_name)


def canonical_group_key(run_dir, run_name):
    meta = parse_run_meta(os.path.join(run_dir, "run-meta.txt"))
    if meta:
        ordered = tuple(sorted((k, v) for k, v in meta.items()))
        return ordered, canonical_group_name(run_name)
    return (("run_prefix", canonical_group_name(run_name)),), canonical_group_name(run_name)


def discover_run_dirs(root_dir):
    direct_metric_files = sorted(glob.glob(os.path.join(root_dir, "validator-*.metrics")))
    if direct_metric_files:
        return [os.path.abspath(root_dir)]

    run_dirs = []
    for current_root, _, files in os.walk(root_dir):
        if any(name.startswith("validator-") and name.endswith(".metrics") for name in files):
            run_dirs.append(os.path.abspath(current_root))
    return sorted(set(run_dirs))


def main():
    if len(sys.argv) != 2:
        print("usage: summarize_metrics.py <result-dir>", file=sys.stderr)
        sys.exit(1)

    result_dir = os.path.abspath(sys.argv[1])
    run_dirs = discover_run_dirs(result_dir)

    writer = csv.writer(sys.stdout)
    writer.writerow(["run", "validator", "metric", "labels", "value"])

    if not run_dirs:
        return

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
        "speculative_snapshot_non_window_peak_size",
        "speculative_snapshot_store_size",
    }

    repeat_aggregates = defaultdict(list)

    for run_dir in run_dirs:
        metric_files = sorted(glob.glob(os.path.join(run_dir, "validator-*.metrics")))
        if not metric_files:
            continue

        run_name = os.path.basename(run_dir)
        run_aggregates = defaultdict(list)
        group_key, group_name = canonical_group_key(run_dir, run_name)

        for metric_file in metric_files:
            validator = os.path.splitext(os.path.basename(metric_file))[0]
            for name, labels, value in parse_metrics(metric_file):
                if name not in interesting:
                    continue
                label_str = ";".join(f"{k}={labels[k]}" for k in sorted(labels))
                writer.writerow([run_name, validator, name, label_str, value])
                run_aggregates[(name, label_str)].append(value)

        for (name, label_str), values in sorted(run_aggregates.items()):
            avg_value = sum(values) / len(values)
            writer.writerow([run_name, "average", name, label_str, avg_value])
            repeat_aggregates[(group_key, group_name, name, label_str)].append(avg_value)

    for (group_key, group_name, name, label_str), values in sorted(repeat_aggregates.items(), key=lambda item: (item[0][1], item[0][2], item[0][3])):
        if len(values) < 2:
            continue
        avg_value = sum(values) / len(values)
        writer.writerow([group_name, "repeat_average", name, label_str, avg_value])


if __name__ == "__main__":
    main()
