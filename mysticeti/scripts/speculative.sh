#!/bin/bash

# Configuration
COMMITTEE_SIZE=${1:-4}
DURATION=${2:-15}  # Run duration in seconds

cargo build 2>&1 >/dev/null | tail -n 10

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::net_sync=DEBUG,mysticeti_core::core=DEBUG,mysticeti_core::validator=DEBUG,mysticeti_core::transactions_generator=INFO,mysticeti_core::executor=INFO,pevm=INFO,mysticeti_core::speculative_executor=DEBUG,mysticeti_core::block_handler=INFO,

tmux kill-server || true

echo "Starting validators..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cargo run --bin mysticeti -- dry-run --committee-size ${COMMITTEE_SIZE} --authority ${i} > v${i}.log.ansi"
done

sleep ${DURATION}

# report the metrics
curl http://0.0.0.0:$((1500 + COMMITTEE_SIZE + 1))/metrics > ./log0.txt
curl http://0.0.0.0:$((1500 + COMMITTEE_SIZE + 2))/metrics > ./log1.txt

echo "Stopping validators..."
tmux kill-server