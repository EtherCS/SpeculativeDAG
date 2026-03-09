import pandas as pd
import matplotlib.pyplot as plt
import matplotlib
import glob
import os
import argparse

# Typography settings for double-column paper (IEEE/ACM style)
matplotlib.rcParams.update({
    'font.size': 14,
    'axes.titlesize': 15,
    'axes.labelsize': 14,
    'xtick.labelsize': 13,
    'ytick.labelsize': 13,
    'legend.fontsize': 12,
    'lines.linewidth': 2,
    'font.family': 'serif',
})

parser = argparse.ArgumentParser(description='Plot latency comparison across multiple algorithms.')
parser.add_argument('input_path', help='Directory containing *.csv latency files')
parser.add_argument('output_file', help='Output file for the saved figure (e.g., comparison.pdf)')
args = parser.parse_args()

csv_files = sorted(glob.glob(os.path.join(args.input_path, '*.csv')))

if not csv_files:
    print(f'No CSV files found in: {args.input_path}')
    exit(1)

print(f'Found {len(csv_files)} CSV file(s): {[os.path.basename(f) for f in csv_files]}')

fig, ax = plt.subplots(figsize=(7, 3.5))

prop_cycle = plt.rcParams['axes.prop_cycle']
colors = [c['color'] for c in prop_cycle]
markers = ['s', '^', 'D', 'v', 'p', '*', 'h', 'o']

for i, csv_path in enumerate(csv_files):
    label = os.path.splitext(os.path.basename(csv_path))[0]
    df = pd.read_csv(csv_path)

    if len(df) == 0:
        print(f'Skipping empty file: {csv_path}')
        continue

    color = colors[i % len(colors)]
    marker = markers[i % len(markers)]
    x = df['elapsed_secs'].to_numpy()
    x = x - x.min() + 1  # offset so x-axis starts from 1
    ax.plot(x, df['p50_ms'].to_numpy() / 1000.0, linewidth=2, color=color, linestyle='-',  marker=marker, markersize=5, label=f'{label} (p50)')
    ax.plot(x, df['p90_ms'].to_numpy() / 1000.0, linewidth=2, color=color, linestyle='--', marker=marker, markersize=5, label=f'{label} (p90)')

ax.set_xlabel('Request start time (s)')
ax.set_ylabel('Latency (s)')

ax.legend()
ax.grid(True, alpha=0.3)

plt.tight_layout()
plt.savefig(args.output_file, bbox_inches='tight')
print(f'Figure saved to: {args.output_file}')
plt.show()