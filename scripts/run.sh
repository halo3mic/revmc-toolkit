#!/bin/bash

num_iterations=100
count=0

for (( i=1; i<=num_iterations; i++ ))
do
    echo "Iteration $i of $num_iterations"
    RUST_LOG=debug ./target/release/revmc-sim-cli  block-range 18999999..20371443 f18999999t20371443s50r$i --sample-size 100 --rnd-seed sorandomm$i
done

echo "Loop completed."