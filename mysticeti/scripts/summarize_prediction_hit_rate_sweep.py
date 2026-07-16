#!/usr/bin/env python3
"""Summarize controlled APS prediction errors and plot measured hit rate."""

from __future__ import annotations

import argparse
import csv
from collections import defaultdict
from pathlib import Path


HITS = {"commit_hit", "skip_hit"}
MISSES = {"commit_miss", "skip_miss"}


def load_run(path: Path):
    hit = 0.0
    miss = 0.0
    p99 = []
    replay = []
    with path.open(newline="") as stream:
        for row in csv.DictReader(stream):
            validator = row.get("validator", "")
            if not validator.startswith("validator-"):
                continue
            metric = row.get("metric")
            labels = row.get("labels", "")
            value = float(row["value"])
            if metric == "speculative_predictions_total":
                outcome = labels.removeprefix("outcome=")
                if outcome in HITS:
                    hit += value
                elif outcome in MISSES:
                    miss += value
            elif metric == "transaction_committed_latency" and labels == "v=p99":
                p99.append(value)
            elif metric == "speculative_reexecuted_leaders_total":
                replay.append(value)
    if hit + miss == 0 or not p99:
        return None
    return hit / (hit + miss), sum(p99) / len(p99), sum(replay) / len(replay) if replay else 0.0


def eac_p99(root: Path):
    values = []
    for summary in root.glob("eac/repeat-*/summary.csv"):
        with summary.open(newline="") as stream:
            for row in csv.DictReader(stream):
                if (
                    row.get("validator") == "average"
                    and row.get("metric") == "transaction_committed_latency"
                    and row.get("labels") == "v=p99"
                ):
                    values.append(float(row["value"]))
    return sum(values) / len(values) if values else None


def discover(root: Path):
    grouped = defaultdict(list)
    for summary in root.glob("error-*/repeat-*/summary.csv"):
        error_rate = int(summary.relative_to(root).parts[0].removeprefix("error-"))
        value = load_run(summary)
        if value is not None:
            grouped[error_rate].append(value)
    return grouped


def write_csv(path: Path, grouped, eac):
    rows = []
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(
            [
                "requested_error_rate",
                "runs",
                "measured_hit_rate",
                "p99_us",
                "eac_p99_us",
                "p99_vs_eac",
                "reexecuted_leaders",
            ]
        )
        for error_rate, observations in sorted(grouped.items()):
            runs = len(observations)
            hit_rate = sum(item[0] for item in observations) / runs
            p99 = sum(item[1] for item in observations) / runs
            replay = sum(item[2] for item in observations) / runs
            ratio = p99 / eac if eac not in (None, 0) else None
            row = (error_rate, runs, hit_rate, p99, eac, ratio, replay)
            rows.append(row)
            writer.writerow(
                [
                    error_rate,
                    runs,
                    f"{hit_rate:.6f}",
                    f"{p99:.3f}",
                    "" if eac is None else f"{eac:.3f}",
                    "" if ratio is None else f"{ratio:.6f}",
                    f"{replay:.3f}",
                ]
            )
    return rows


def plot(path: Path, rows):
    try:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError as error:
        raise SystemExit("matplotlib is required to generate the figure") from error

    rows_by_error = sorted(rows)
    rows_by_hit = sorted(rows, key=lambda row: row[2])
    fig, axes = plt.subplots(1, 2, figsize=(7.2, 3.0))

    axes[0].plot(
        [row[0] for row in rows_by_error],
        [100 * row[2] for row in rows_by_error],
        color="#4874CB",
        marker="o",
    )
    axes[0].set_xlabel("Injected prediction errors (%)")
    axes[0].set_ylabel("Measured hit rate (%)")
    axes[0].grid(alpha=0.25)

    axes[1].plot(
        [100 * row[2] for row in rows_by_hit],
        [row[3] / 1_000_000.0 for row in rows_by_hit],
        color="#C00000",
        marker="s",
    )
    axes[1].set_xlabel("Measured hit rate (%)")
    axes[1].set_ylabel("p99 confirmation latency (s)")
    eac = next((row[4] for row in rows_by_hit if row[4] is not None), None)
    if eac is not None:
        axes[1].axhline(
            eac / 1_000_000.0,
            color="#555555",
            linestyle="--",
            linewidth=1.5,
            label="MysticetiEAC",
        )
        axes[1].legend(frameon=False)
    axes[1].grid(alpha=0.25)

    fig.tight_layout()
    path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(path, bbox_inches="tight")
    fig.savefig(path.with_suffix(".png"), dpi=220, bbox_inches="tight")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--csv", type=Path)
    parser.add_argument("--figure", type=Path)
    args = parser.parse_args()
    grouped = discover(args.root)
    if not grouped:
        raise SystemExit(f"No completed runs found under {args.root}")
    csv_path = args.csv or args.root / "prediction-hit-rate-summary.csv"
    figure_path = args.figure or args.root / "prediction-hit-rate-sweep.pdf"
    rows = write_csv(csv_path, grouped, eac_p99(args.root))
    plot(figure_path, rows)
    print(f"Wrote {csv_path}")
    print(f"Wrote {figure_path} and {figure_path.with_suffix('.png')}")


if __name__ == "__main__":
    main()
