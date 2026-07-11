#!/usr/bin/env python3
"""Summarize repeated results produced by ablation_aps.sh."""
from __future__ import annotations

import argparse
import csv
import re
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

TIMESTAMP_RE = re.compile(r"^(?P<label>.+)-(?P<timestamp>\d{8}-\d{6})$")
HITS = ("skip_hit", "commit_hit")
MISSES = ("skip_miss", "commit_miss")

@dataclass
class RunMetrics:
    hit_rate: float
    p99_latency: float

def load_run(path: Path) -> RunMetrics | None:
    counts = defaultdict(lambda: defaultdict(float))
    p99 = {}
    with path.open(newline="") as stream:
        for row in csv.DictReader(stream):
            validator = row.get("validator", "")
            metric = row.get("metric", "")
            labels = row.get("labels", "")
            try:
                value = float(row["value"])
            except (KeyError, TypeError, ValueError):
                continue
            if metric == "speculative_predictions_total":
                counts[validator][labels.removeprefix("outcome=")] += value
            elif metric == "transaction_committed_latency" and labels == "v=p99":
                p99[validator] = value
    hit = sum(counts[v][o] for v in counts for o in HITS)
    miss = sum(counts[v][o] for v in counts for o in MISSES)
    if hit + miss == 0 or not p99:
        return None
    return RunMetrics(hit / (hit + miss), sum(p99.values()) / len(p99))

def summarize(root: Path):
    runs = defaultdict(list)
    for kind in ("speculative", "jitter"):
        kind_root = root / kind
        if not kind_root.is_dir():
            continue
        for directory in kind_root.iterdir():
            if not directory.is_dir():
                continue
            match = TIMESTAMP_RE.match(directory.name)
            if not match:
                continue
            summary = directory / "summary.csv"
            metrics = load_run(summary) if summary.is_file() else None
            if metrics is not None:
                runs[f"{kind}-{match.group('label')}"].append(metrics)
    return runs

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        type=Path,
        default=Path("results/ablation-aps"),
        help="result directory (default: results/ablation-aps)",
    )
    args = parser.parse_args()
    runs = summarize(args.root)
    if not runs:
        raise SystemExit(f"No usable summary.csv files found under {args.root}")
    print("label,runs,hit_rate,p99_latency")
    for label in sorted(runs):
        values = runs[label]
        hit_rate = sum(item.hit_rate for item in values) / len(values)
        p99 = sum(item.p99_latency for item in values) / len(values)
        print(f"{label},{len(values)},{hit_rate:.6f},{p99:.2f}")

if __name__ == "__main__":
    main()
