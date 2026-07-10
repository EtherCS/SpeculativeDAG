#!/bin/bash

set -u

OUTPUT_DIR=$1
COMMITTEE_SIZE=$2
INTERVAL=${3:-1}
USAGE_FILE="${OUTPUT_DIR}/resource-usage.csv"
SUMMARY_FILE="${OUTPUT_DIR}/resource-summary.csv"
START_TIME=$(date +%s)

summarize() {
    awk -F, '
        NR == 1 { next }
        {
            authority = $3
            cpu = $5 + 0
            rss = $6 + 0
            elapsed = $2

            count[authority]++
            cpu_sum[authority] += cpu
            rss_sum[authority] += rss
            if (cpu > cpu_peak[authority]) cpu_peak[authority] = cpu
            if (rss > rss_peak[authority]) rss_peak[authority] = rss

            aggregate_cpu[elapsed] += cpu
            aggregate_rss[elapsed] += rss
        }
        END {
            print "authority,samples,avg_cpu_percent,peak_cpu_percent,avg_rss_mb,peak_rss_mb"
            for (authority in count) {
                printf "%s,%d,%.3f,%.3f,%.3f,%.3f\n", authority, count[authority],
                    cpu_sum[authority] / count[authority], cpu_peak[authority],
                    rss_sum[authority] / count[authority] / 1024, rss_peak[authority] / 1024
            }

            aggregate_count = 0
            aggregate_cpu_sum = 0
            aggregate_rss_sum = 0
            aggregate_cpu_peak = 0
            aggregate_rss_peak = 0
            for (elapsed in aggregate_cpu) {
                aggregate_count++
                aggregate_cpu_sum += aggregate_cpu[elapsed]
                aggregate_rss_sum += aggregate_rss[elapsed]
                if (aggregate_cpu[elapsed] > aggregate_cpu_peak) {
                    aggregate_cpu_peak = aggregate_cpu[elapsed]
                }
                if (aggregate_rss[elapsed] > aggregate_rss_peak) {
                    aggregate_rss_peak = aggregate_rss[elapsed]
                }
            }
            if (aggregate_count > 0) {
                printf "all,%d,%.3f,%.3f,%.3f,%.3f\n", aggregate_count,
                    aggregate_cpu_sum / aggregate_count, aggregate_cpu_peak,
                    aggregate_rss_sum / aggregate_count / 1024, aggregate_rss_peak / 1024
            }
        }
    ' "${USAGE_FILE}" > "${SUMMARY_FILE}"
}

trap 'summarize; exit 0' TERM INT

printf 'timestamp_epoch,elapsed_seconds,authority,pid,cpu_percent,rss_kb\n' > "${USAGE_FILE}"

while true; do
    now=$(date +%s)
    elapsed=$((now - START_TIME))
    for authority in $(seq 0 $((COMMITTEE_SIZE - 1))); do
        pid_file="${OUTPUT_DIR}/validator-${authority}.pid"
        if [[ ! -s "${pid_file}" ]]; then
            continue
        fi

        pid=$(<"${pid_file}")
        sample=$(ps -p "${pid}" -o %cpu= -o rss= 2>/dev/null || true)
        if [[ -z "${sample}" ]]; then
            continue
        fi

        read -r cpu_percent rss_kb <<< "${sample}"
        printf '%s,%s,%s,%s,%s,%s\n' \
            "${now}" "${elapsed}" "${authority}" "${pid}" \
            "${cpu_percent}" "${rss_kb}" >> "${USAGE_FILE}"
    done
    sleep "${INTERVAL}"
done
