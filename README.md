## REVMC Toolkit

Tools for comparing execution of calls, transactions and blocks with [revmc](https://github.com/paradigmxyz/revmc) JIT and AOT compiled functions. 

Very much work in progress! 🚧

## Usage


### Bench a single block or tx
```bash
cargo run --release -p revmc-toolkit-bench bench --block-num {block-number} 
```

```bash
cargo run --release -p revmc-toolkit-bench bench --tx-hash {tx-hash}
```


### Measure and record performance within a block range 
```bash 
RUST_LOG=info cargo run --release -p revmc-toolkit-bench block-range 20307900..20347900 f20307900t20347900s50 --sample-size 10
```