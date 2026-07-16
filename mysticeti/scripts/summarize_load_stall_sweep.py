#!/usr/bin/env python3
"""Summarize and plot the load-by-stall-duration operating envelope."""

from __future__ import annotations

import argparse
import csv
import math
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path


@dataclass
class Observation:
    p50_us: float | None
    p99_us: float | None


def metric_value(path: Path, metric: str, label: str) -> float | None:
    values = []
    with path.open(newline="") as stream:
        for row in csv.DictReader(stream):
            if (
                row.get("validator") == "average"
                and row.get("metric") == metric
                and row.get("labels") == label
            ):
                values.append(float(row["value"]))
    return sum(values) / len(values) if values else None


def discover(root: Path):
    grouped = defaultdict(list)
    for summary in root.glob("*/load-*/stall-*/repeat-*/summary.csv"):
        parts = summary.relative_to(root).parts
        mode = parts[0]
        load = int(parts[1].removeprefix("load-"))
        stall = int(parts[2].removeprefix("stall-"))
        grouped[(mode, load, stall)].append(
            Observation(
                metric_value(summary, "transaction_committed_latency", "v=p50"),
                metric_value(summary, "transaction_committed_latency", "v=p99"),
            )
        )
    return grouped


def mean(values):
    present = [value for value in values if value is not None]
    return sum(present) / len(present) if present else None


def write_csv(path: Path, grouped):
    path.parent.mkdir(parents=True, exist_ok=True)
    aggregate = {}
    for key, observations in grouped.items():
        aggregate[key] = (
            len(observations),
            mean(item.p50_us for item in observations),
            mean(item.p99_us for item in observations),
        )

    with path.open("w", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(
            ["mode", "load_tps", "stall_duration_s", "runs", "p50_us", "p99_us", "p99_vs_eac"]
        )
        for (mode, load, stall), (runs, p50, p99) in sorted(aggregate.items()):
            eac = aggregate.get(("eac", load, stall), (0, None, None))[2]
            ratio = p99 / eac if p99 is not None and eac not in (None, 0) else None
            writer.writerow(
                [
                    mode,
                    load,
                    stall,
                    runs,
                    "" if p50 is None else f"{p50:.3f}",
                    "" if p99 is None else f"{p99:.3f}",
                    "" if ratio is None else f"{ratio:.6f}",
                ]
            )
    return aggregate


def plot(path: Path, aggregate):
    try:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        from matplotlib.colors import TwoSlopeNorm
    except ImportError as error:
        raise SystemExit("matplotlib is required to generate the heatmap") from error

    loads = sorted({load for mode, load, _ in aggregate if mode == "full"})
    stalls = sorted({stall for mode, _, stall in aggregate if mode == "full"})
    if not loads or not stalls:
        raise SystemExit("No full-mode results found")

    matrix = []
    finite = []
    for stall in stalls:
        row = []
        for load in loads:
            full = aggregate.get(("full", load, stall), (0, None, None))[2]
            eac = aggregate.get(("eac", load, stall), (0, None, None))[2]
            ratio = full / eac if full is not None and eac not in (None, 0) else math.nan
            row.append(ratio)
            if math.isfinite(ratio):
                finite.append(ratio)
        matrix.append(row)
    if not finite:
        raise SystemExit("No matched full/EAC cells found")

    low, high = min(finite), max(finite)
    if low < 1 < high:
        norm = TwoSlopeNorm(vmin=low, vcenter=1.0, vmax=high)
    else:
        norm = None

    fig, ax = plt.subplots(figsize=(6.4, 3.6))
    image = ax.imshow(matrix, origin="lower", aspect="auto", cmap="RdYlBu_r", norm=norm)
    ax.set_xticks(range(len(loads)), loads)
    ax.set_yticks(range(len(stalls)), stalls)
    ax.set_xlabel("Offered load (tx/s)")
    ax.set_ylabel("Stall duration (s)")
    ax.set_title("Pufferfish p99 latency / MysticetiEAC p99 latency")
    for y, row in enumerate(matrix):
        for x, ratio in enumerate(row):
            ax.text(x, y, "NA" if not math.isfinite(ratio) else f"{ratio:.2f}",
                    ha="center", va="center", fontsize=9)
    colorbar = fig.colorbar(image, ax=ax)
    colorbar.set_label("Latency ratio (<1 favors Pufferfish)")
    fig.tight_layout()
    path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(path, bbox_inches="tight")
    png_path = path.with_suffix(".png")
    fig.savefig(png_path, dpi=220, bbox_inches="tight")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--csv", type=Path)
    parser.add_argument("--figure", type=Path)
    args = parser.parse_args()
    grouped = discover(args.root)
    if not grouped:
        raise SystemExit(f"No completed runs found under {args.root}")
    csv_path = args.csv or args.root / "load-stall-summary.csv"
    figure_path = args.figure or args.root / "load-stall-heatmap.pdf"
    aggregate = write_csv(csv_path, grouped)
    plot(figure_path, aggregate)
    print(f"Wrote {csv_path}")
    print(f"Wrote {figure_path} and {figure_path.with_suffix('.png')}")


if __name__ == "__main__":
    main()
