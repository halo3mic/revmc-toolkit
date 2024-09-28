## REVMC Toolkit

Tools for comparing execution of calls, transactions and blocks with [revmc](https://github.com/paradigmxyz/revmc) JIT and AOT compiled functions. 

Very much work in progress! 🚧

## Example usage 

### Bench or run a call, tx or block

Run command for usage instructions:
```bash
cargo run -p revmc-toolkit-bench
```

### Measure and record performance within a block range 
```bash 
RUST_LOG=info cargo run --release -p revm-toolkit-bench block-range 20307900..20347900 f20307900t20347900s50 --sample-size 10
```