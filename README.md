# REVMC Toolkit

Tools for building, loading and integrating [revmc](https://github.com/paradigmxyz/revmc) JIT & AOT compiled functions in revm environment. 

Additionally, it provides a way to compare the performance of JIT and AOT compiled functions to native EVM execution.

## Usage of bencher

For transactions and benches you can specify which contracts will be compiled:
  * `selected` (default): All contracts that are called during EVM execution.
  * `gas-guzzlers`: Contracts that consumed the most gas in specified block range. Use `help` command to see the parameters.

### Run 
Run Fibonacci call
```bash
cargo run --release -p revmc-toolkit-bench run call --run-type {aot/jit/native}
```
Run Transaction
```bash
cargo run --release -p revmc-toolkit-bench run tx {tx-hash} --run-type {aot/jit/native}
```
Run Block
```bash
cargo run --release -p revmc-toolkit-bench run block {block-number} --run-type {aot/jit/native}
```

### Bench
#### Bench Fibonacci call
```
RUST_LOG=info cargo run --release -p revmc-toolkit-bench bench call
```
#### Bench Transaction
```bash
RUST_LOG=info cargo run --release -p revmc-toolkit-bench bench tx {tx-hash}
```
#### Bench Block
```bash
RUST_LOG=info cargo run --release -p revmc-toolkit-bench bench block {block-number}
```
#### Bench Block Range
```bash 
RUST_LOG=info cargo run --release -p revmc-toolkit-bench block-range {start-block}..{end-block} --sample-size {sample-size}
```
The results will be recorded in a file. See `--help` for more options.

## Gas Guzzlers

Find top bytecodes that consumed the most gas in specified block range.

```bash
cargo run --release --package gas-guzzlers --bin gas-guzzlers -- --start-block {start-block} --end-block {end-block} --sample-size {sample-size} --take {limit}
```