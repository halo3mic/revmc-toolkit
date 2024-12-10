#!/bin/bash

exec &> script.log

# sizes=(10 100 500 1000 5000 10000)
# Default: 5, 50, 250, 750, 2500, 7500 @ 3 runs
sizes=(10 100 500 1000 2000 4000 6000 8000 10000)
block0=21100000
sample_size=6000

for i in $(seq 0 4)
do
    echo "Iteration $i of 4"
    start_block=$((block0 + i * 6000))
    end_block=$((start_block + 6000))
    gg_start_block=$((start_block - 6000))

    for size in "${sizes[@]}"
    do
        echo "Running bench for size $size, iteration $i: $start_block..$end_block"

        RUST_LOG=info cargo run --release -p revmc-toolkit-bench bench block-range \
            $start_block..$end_block "gg-${size}-aggressive-${i}" \
            --out-dir "./.data/measurements-public/gg_exe_compare_9" \
            --comp-opt-level 3 gas-guzzlers --size-limit $size \
            --start-block $gg_start_block \
            --end-block $start_block \
            --sample-size $sample_size \
            --blacklist "0xc919b535f7bbbaa57f32b602f26000e3997c22c933efbb92a36e053b20b2b4b1,0x96f2c555a4541525d77fbe720763a255ea6d306254305ed45ee28f0cde7d4fee,0x5b98f7f184bb404df45ac98096ed0e87b4383ac50753a60cf543fba5562108f5,0x48a2b70c96c5638b5603f2fa3ef3d82382c02c003ed73cf4e0ab679c2a92de2f,0xe337f15a521fa93eae7bc4fe0069ffe664adbad4df2d8a544ca38d56c35ad372"
    done
done

echo "Loop completed."