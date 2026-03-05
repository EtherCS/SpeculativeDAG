import pandas as pd
import matplotlib.pyplot as plt
import glob
import os
import argparse

parser = argparse.ArgumentParser(description='Plot latency comparison across multiple algorithms.')
parser.add_argument('input_path', help='Directory containing *.csv latency files')
parser.add_argument('output_file', help='Output file for the saved figure (e.g., comparison.pdf)')
args = parser.parse_args()

csv_files = sorted(glob.glob(os.path.join(args.input_path, '*.csv')))

if not csv_files:
    print(f'No CSV files found in: {args.input_path}')
    exit(1)

print(f'Found {len(csv_files)} CSV file(s): {[os.path.basename(f) for f in csv_files]}')

fig, ax = plt.subplots(figsize=(12, 6))

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
    ax.plot(x, df['p50_ms'].to_numpy(), linewidth=1.5, color=color, linestyle='-',  marker=marker, markersize=4, label=f'{label} (p50)')
    ax.plot(x, df['p90_ms'].to_numpy(), linewidth=1.5, color=color, linestyle='--', marker=marker, markersize=4, label=f'{label} (p90)')

ax.set_xlabel('Request start time (s)')
ax.set_ylabel('Transaction Committed Latency (ms)')
# ax.set_title('Latency Comparison')
ax.legend()
ax.grid(True, alpha=0.3)

plt.tight_layout()
plt.savefig(args.output_file)
print(f'Figure saved to: {args.output_file}')
plt.show()
