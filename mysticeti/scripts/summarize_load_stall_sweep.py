#!/usr/bin/env python3
"""Summarize boundary-transaction latency for the load-by-stall sweep."""

from __future__ import annotations

import argparse
import csv
import math
import re
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path


@dataclass
class Observation:
    boundary_latency_us: float | None
    validators: int


BOUNDARY_METRIC_RE = re.compile(
    r"^boundary_transaction_commit_latency_us\s+([0-9eE+.\-]+)$"
)


def boundary_latency(path: Path) -> float | None:
    with path.open(encoding="utf-8", errors="replace") as stream:
        for line in stream:
            match = BOUNDARY_METRIC_RE.match(line.strip())
            if match:
                value = float(match.group(1))
                return value if value > 0 else None
    return None


def run_observation(run_dir: Path) -> Observation:
    values = [
        value
        for metric_file in sorted(run_dir.glob("validator-*.metrics"))
        if (value := boundary_latency(metric_file)) is not None
    ]
    return Observation(
        boundary_latency_us=sum(values) / len(values) if values else None,
        validators=len(values),
    )


def discover(root: Path):
    grouped = defaultdict(list)
    for run_meta in root.glob("*/load-*/stall-*/repeat-*/run-meta.txt"):
        run_dir = run_meta.parent
        parts = run_dir.relative_to(root).parts
        mode = parts[0]
        load = int(parts[1].removeprefix("load-"))
        stall = int(parts[2].removeprefix("stall-"))
        grouped[(mode, load, stall)].append(run_observation(run_dir))
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
            min((item.validators for item in observations), default=0),
            mean(item.boundary_latency_us for item in observations),
        )

    with path.open("w", newline="") as stream:
        writer = csv.writer(stream)
        writer.writerow(
            [
                "mode",
                "load_tps",
                "stall_duration_s",
                "runs",
                "min_validators",
                "boundary_commit_latency_us",
                "boundary_latency_vs_eac",
            ]
        )
        for (mode, load, stall), (runs, validators, latency) in sorted(aggregate.items()):
            eac = aggregate.get(("eac", load, stall), (0, 0, None))[2]
            ratio = latency / eac if latency is not None and eac not in (None, 0) else None
            writer.writerow(
                [
                    mode,
                    load,
                    stall,
                    runs,
                    validators,
                    "" if latency is None else f"{latency:.3f}",
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
            full = aggregate.get(("full", load, stall), (0, 0, None))[2]
            eac = aggregate.get(("eac", load, stall), (0, 0, None))[2]
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
    ax.set_title("Boundary-transaction latency: Pufferfish / MysticetiEAC")
    for y, row in enumerate(matrix):
        for x, ratio in enumerate(row):
            ax.text(x, y, "NA" if not math.isfinite(ratio) else f"{ratio:.2f}",
                    ha="center", va="center", fontsize=9)
    colorbar = fig.colorbar(image, ax=ax)
    colorbar.set_label("Boundary latency ratio (<1 favors Pufferfish)")
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
