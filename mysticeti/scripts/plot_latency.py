import pandas as pd
import matplotlib.pyplot as plt
import glob
import os
import re
import argparse

parser = argparse.ArgumentParser(description='Plot transaction committed latency from validator CSV files.')
parser.add_argument('--csv', default=None,
                    help='Glob pattern or explicit path to latency CSV file(s). '
                         'Default: searches dryrun-validator-*/storage-*/latency.csv then jitterrun-validator-*/storage-*/latency.csv')
parser.add_argument('--output', default='transaction_latency.pdf',
                    help='Output file name for the saved figure (default: transaction_latency.pdf)')
args = parser.parse_args()

# Collect latency CSV files written by MetricReporter (one per validator)
# Searches in dryrun/jitterrun validator storage directories
if args.csv:
    csv_files = sorted(glob.glob(args.csv))
else:
    csv_files = sorted(glob.glob('dryrun-validator-*/storage-*/latency.csv') +
                       glob.glob('jitterrun-validator-*/storage-*/latency.csv'))

if not csv_files:
    # Fallback: look in current directory
    csv_files = glob.glob('latency*.csv')

if not csv_files:
    print('No latency CSV files found. Run the validators first.')
    exit(1)

print(f'Found {len(csv_files)} CSV file(s): {csv_files}')

# Use the first validator's data (validator 0)
csv_path = csv_files[0]
print(f'Plotting from: {csv_path}')

# Extract validator index from path to locate its log file
validator_idx_match = re.search(r'validator-(\d+)', csv_path)
tps_label = ''
if validator_idx_match:
    v_idx = validator_idx_match.group(1)
    log_file = f'v{v_idx}.log.ansi'
    if os.path.exists(log_file):
        with open(log_file, 'r', errors='replace') as f:
            for line in f:
                m = re.search(r'Starting generator with (\d+) transactions per second', line)
                if m:
                    tps_label = f'  (workload: {m.group(1)} tx/s)'
                    break

df = pd.read_csv(csv_path)
# Columns: elapsed_secs, p50_ms, p90_ms, p99_ms

if len(df) == 0:
    print('CSV file is empty. No data to plot.')
    exit(1)

# Plot
fig, ax = plt.subplots(figsize=(12, 6))
ax.plot(df['elapsed_secs'].to_numpy(), df['p50_ms'].to_numpy(),
        marker='o', markersize=3, linewidth=1.5, label='p50')
ax.plot(df['elapsed_secs'].to_numpy(), df['p90_ms'].to_numpy(),
        marker='s', markersize=3, linewidth=1.5, label='p90')
ax.plot(df['elapsed_secs'].to_numpy(), df['p99_ms'].to_numpy(),
        marker='^', markersize=3, linewidth=1.5, label='p99')
ax.set_xlabel('Running Time (seconds)')
ax.set_ylabel('Transaction Committed Latency (ms)')
ax.set_title(f'Transaction Latency Over Time{tps_label}\n({os.path.basename(csv_path)})')
ax.legend()
ax.grid(True, alpha=0.3)
plt.tight_layout()
plt.savefig(args.output, dpi=300)
print(f'Figure saved to: {args.output}')
plt.show()