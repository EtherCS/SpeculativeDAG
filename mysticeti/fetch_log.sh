#!/bin/bash

# List of IP addresses
IPS=(
172.31.22.183
172.31.21.95
172.31.22.99
172.31.30.247
)

# Path to your SSH private key
KEY=~/.ssh/dakai_dev.pem

# Remote user (adjust if needed, e.g., ubuntu/ec2-user)
USER=ubuntu

# Destination folder for logs
DEST=./logs
mkdir -p "$DEST"

# Loop through IPs and fetch logs
for ip in "${IPS[@]}"; do
    echo "Fetching node.log from $ip..."
    scp -o StrictHostKeyChecking=no -i "$KEY" "$USER@$ip:~/node.log" "$DEST/node-$ip.log" &
done

wait

echo "✅ All logs fetched into $DEST/"
