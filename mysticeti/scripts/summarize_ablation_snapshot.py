#!/usr/bin/env python3
"""Summarize repeated snapshot-policy ablation results."""

from __future__ import annotations

import argparse
import csv
import re
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path


TIMESTAMP_RE = re.compile(r"^(?P<label>.+)-(?P<timestamp>\d{8}-\d{6})$")


@dataclass
class RunMetrics:
    snapshot_store_size: float
    p99_latency: float
    avg_cpu_percent: float
    avg_rss_mb: float


def load_summary(path: Path) -> tuple[float, float] | None:
    snapshot_sizes = []
    p99_latencies = []
    with path.open(newline="") as stream:
        for row in csv.DictReader(stream):
            metric = row.get("metric")
            labels = row.get("labels")
            try:
                value = float(row["value"])
            except (KeyError, TypeError, ValueError):
                continue
            if metric == "speculative_snapshot_store_size":
                snapshot_sizes.append(value)
            elif metric == "transaction_committed_latency" and labels == "v=p99":
                p99_latencies.append(value)
    if not snapshot_sizes or not p99_latencies:
        return None
    return (
        sum(snapshot_sizes) / len(snapshot_sizes),
        sum(p99_latencies) / len(p99_latencies),
    )


def load_resources(path: Path) -> tuple[float, float] | None:
    """Average CPU and RSS across authority rows, excluding the all row."""
    cpu = []
    rss = []
    with path.open(newline="") as stream:
        for row in csv.DictReader(stream):
            if row.get("authority", "").strip().lower() == "all":
                continue
            try:
                cpu.append(float(row["avg_cpu_percent"]))
                rss.append(float(row["avg_rss_mb"]))
            except (KeyError, TypeError, ValueError):
                continue
    if not cpu or not rss:
        return None
    return sum(cpu) / len(cpu), sum(rss) / len(rss)


def load_run(directory: Path) -> RunMetrics | None:
    summary = load_summary(directory / "summary.csv")
    resources = load_resources(directory / "resource-summary.csv")
    if summary is None or resources is None:
        return None
    return RunMetrics(*summary, *resources)


def summarize(root: Path) -> dict[str, list[RunMetrics]]:
    repeated: dict[str, list[RunMetrics]] = defaultdict(list)
    for kind in ("speculative", "jitter"):
        kind_root = root / kind
        if not kind_root.is_dir():
            continue
        for directory in kind_root.iterdir():
            if not directory.is_dir():
                continue
            match = TIMESTAMP_RE.match(directory.name)
            if match is None:
                continue
            metrics = load_run(directory)
            if metrics is not None:
                repeated[f"{kind}-{match.group('label')}"].append(metrics)
    return repeated


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        type=Path,
        default=Path("results/ablation-snapshot"),
        help="result directory (default: results/ablation-snapshot)",
    )
    args = parser.parse_args()
    repeated = summarize(args.root)
    if not repeated:
        raise SystemExit(f"No usable results found under {args.root}")

    print("label,runs,snapshot_store_size,p99_latency,avg_cpu_percent,avg_rss_mb")
    for label in sorted(repeated):
        runs = repeated[label]
        count = len(runs)
        print(
            f"{label},{count},"
            f"{sum(run.snapshot_store_size for run in runs) / count:.2f},"
            f"{sum(run.p99_latency for run in runs) / count:.2f},"
            f"{sum(run.avg_cpu_percent for run in runs) / count:.3f},"
            f"{sum(run.avg_rss_mb for run in runs) / count:.3f}"
        )


if __name__ == "__main__":
    main()
