# market-rules

Sandboxed WebAssembly rule engine for market data. Runs user supplied rules over
a trade stream, with a per rule instruction budget and memory cap, so that
untrusted logic can be executed per event without being able to stall or crash
the host.

> **Status: work in progress.** Early development, nothing is stable yet. The
> API, the guest ABI and the CLI will change without notice, and the benchmark
> numbers below are not published yet. Not usable as a dependency.

## Why

Platforms that let customers describe their own logic have to execute code they
did not write. Doing that per market event puts two things in tension: the cost
of crossing into the sandbox on every trade, and the guarantees you get in
return. This project exists to measure that trade off on a concrete workload.

Rules are compiled to `wasm32-unknown-unknown` and run in
[wasmtime](https://wasmtime.dev). Each one gets its own instance per instrument,
a refilled fuel budget per call, and a hard memory limit. A rule that loops
forever or allocates without bound is stopped and disabled; the others keep
running.

## Input

Trades come from recordings produced by
[market-stream](https://github.com/paulcollin/market-stream), a Go market data
ingester. Replaying a recording is deterministic, which is what makes the
benchmarks reproducible and the tests stable. Prices and quantities are fixed
point integers throughout, never floats, so a rule returns the same signal on
any machine.

## Status

| Component                        | State       |
| -------------------------------- | ----------- |
| Fixed point conversion           | in progress |
| MSR recording reader             | in progress |
| wasmtime host and rule lifecycle | planned     |
| Fuel and memory limits           | planned     |
| Example rules                    | planned     |
| Benchmarks                       | planned     |

## Design

The reasoning behind the non obvious choices, including why rules are tick
driven rather than candle based and why everything is fixed point, is in
[docs/design.md](docs/design.md).

## License

MIT.
