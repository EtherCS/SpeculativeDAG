#!/bin/bash

# Configuration
COMMITTEE_SIZE=${1:-4}
DURATION=${2:-15}  # Run duration in seconds

cargo build

export RUST_LOG=warn,mysticeti_core::consensus=debug,mysticeti_core::net_sync=DEBUG,mysticeti_core::core=DEBUG,mysticeti_core::validator=DEBUG,mysticeti_core::transactions_generator=INFO,mysticeti_core::executor=INFO,pevm=INFO,mysticeti_core::executor=DEBUG,mysticeti_core::block_handler=INFO,

tmux kill-server || true

echo "Starting validators..."

for i in $(seq 0 $((COMMITTEE_SIZE - 1))); do
    tmux new -d -s "v${i}" "cargo run --bin mysticeti -- dry-run --committee-size ${COMMITTEE_SIZE} --authority ${i} > v${i}.log.ansi"
done

sleep ${DURATION}

# report the metrics
curl http://0.0.0.0:1504/metrics > ./log0.txt
curl http://0.0.0.0:1505/metrics > ./log1.txt

echo "Stopping validators..."
tmux kill-server