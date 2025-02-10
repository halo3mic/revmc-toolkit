#!/bin/bash

if [[ $# -ne 4 ]]; then
    echo "Usage: $0 <start_block> <batch_size> <runs> <gg_take>"
    exit 1
fi

start_block=$1
batch_size=$2
runs=$3
gg_take=$4

if ! [[ $start_block =~ ^[0-9]+$ && $runs =~ ^[0-9]+$ ]]; then
    echo "Error: Both start_block and runs must be numeric."
    exit 1
fi

for i in $(seq 1 "$runs"); do
    echo "Iteration $i of $runs"

    end_block=$((start_block + batch_size))

    output_file=".data/measurements-public/gg_coverage/f${start_block}_t${end_block}_s_${gg_take}.json"

    mkdir -p "$(dirname "$output_file")"

    cargo run --release --package gas-guzzlers --bin gas-guzzlers \
        -- --start-block "$start_block" --end-block "$end_block" \
        --sample-size 6000 --take $gg_take --hashed \
        > "$output_file"

    start_block=$end_block
done
