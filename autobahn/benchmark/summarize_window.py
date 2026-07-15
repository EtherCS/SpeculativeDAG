#!/usr/bin/env python3
"""Summarize an Autobahn run over a client-active time range."""

import argparse
import re
from glob import glob
from os.path import basename, join
from statistics import mean

from benchmark.logs import LogParser


class WindowLogParser(LogParser):
    """Use a send/proposal cohort while retaining later completion events."""

    def __init__(self, clients, primaries, workers, start, end, faults=0):
        super().__init__(clients, primaries, workers, faults=faults)
        client_start = min(self.start)
        self.window_start = client_start + start
        self.window_end = client_start + end
        self.window = end - start

    @classmethod
    def process(cls, directory, start, end, faults=0):
        def read_logs(pattern):
            logs = []
            for filename in sorted(glob(join(directory, pattern))):
                with open(filename, 'r', encoding='utf-8') as log_file:
                    logs.append(log_file.read())
            return logs

        return cls(
            read_logs('client-*.log'),
            read_logs('primary-*.log'),
            read_logs('worker-*.log'),
            start,
            end,
            faults=faults,
        )

    def _in_window(self, timestamp):
        return self.window_start <= timestamp < self.window_end

    def _window_commits(self):
        return {
            digest: timestamp
            for digest, timestamp in self.commits.items()
            if self._in_window(timestamp)
        }

    def _consensus_throughput(self):
        commits = self._window_commits()
        total_bytes = sum(self.sizes.get(digest, 0) for digest in commits)
        bps = total_bytes / self.window
        return bps / self.size[0], bps, self.window

    def _consensus_latency(self):
        latencies = [
            self.commits[digest] - proposed
            for digest, proposed in self.proposals.items()
            if self._in_window(proposed) and digest in self.commits
        ]
        return mean(latencies) if latencies else 0

    def _end_to_end_throughput(self):
        commits = self._window_commits()
        total_bytes = sum(self.sizes.get(digest, 0) for digest in commits)
        bps = total_bytes / self.window
        return bps / self.size[0], bps, self.window

    def _end_to_end_latency(self):
        latencies = []
        completion_times = self.executions or self.commits
        for sent, received in zip(self.sent_samples, self.received_samples):
            for transaction_id, batch_id in received.items():
                start = sent.get(transaction_id)
                if (
                    start is not None
                    and self._in_window(start)
                    and batch_id in completion_times
                ):
                    latencies.append(completion_times[batch_id] - start)
        return mean(latencies) if latencies else 0


def infer_faults(directory):
    match = re.match(r'bench-(\d+)-', basename(directory.rstrip('/')))
    return int(match.group(1)) if match else 0


def parse_args():
    parser = argparse.ArgumentParser(
        description=(
            'Print Autobahn metrics for transactions and proposals created '
            'during [START, END) seconds of client activity.'
        )
    )
    parser.add_argument('log_directory', help='directory containing client, primary, and worker logs')
    parser.add_argument('start', type=float, help='range start in seconds after clients begin sending')
    parser.add_argument('end', type=float, help='exclusive range end in seconds after clients begin sending')
    parser.add_argument(
        '--faults',
        type=int,
        help='fault count; inferred from a bench-FAULTS-... folder name or defaults to 0',
    )
    args = parser.parse_args()
    if args.start < 0:
        parser.error('START must be non-negative')
    if args.end <= args.start:
        parser.error('END must be greater than START')
    if args.faults is not None and args.faults < 0:
        parser.error('--faults must be non-negative')
    return args


def main():
    args = parse_args()
    faults = args.faults if args.faults is not None else infer_faults(args.log_directory)
    result = WindowLogParser.process(
        args.log_directory,
        args.start,
        args.end,
        faults=faults,
    ).result()
    print(result, end='')


if __name__ == '__main__':
    main()
