# Design notes

Rationale behind the non obvious choices in market-rules. Written before the
code, and meant to be corrected by it: if an entry here stops matching the
implementation, the entry is the bug.

## What this is

A host process that replays market data and runs user supplied rules over it.
Each rule is a WebAssembly module executed in a wasmtime sandbox with a bounded
instruction budget and a memory cap. A rule sees trades and returns signal
codes. It cannot allocate without limit, cannot loop forever, cannot touch the
host, and cannot slow down the rules running beside it.

The interesting problem is not the indicators. It is the boundary: how cheaply
can untrusted logic be invoked per event, and what does it cost to keep it
contained.

## What this is not

- Not a technical analysis library. The bundled rules are demonstration
  payloads, deliberately small.
- Not a trading system. No orders, no positions, no broker connectivity.
- Not a market data ingester. That is market-stream's job; this consumes its
  recordings.

## D1. Fixed point everywhere, no floats

Prices and quantities are `i64` in fixed point, with the exponent carried by
the instrument rather than by the value. `"104.48000000"` with a price exponent
of 2 is stored as `10448`.

Binary floating point cannot represent most decimal prices exactly, which makes
comparisons order dependent and results machine dependent. That is fatal here
for two reasons. Rules are compared native against WASM, so any divergence
makes the benchmark meaningless. And rules are replayed against a fixed corpus
in CI, so a signal that flips on a different host is an untrackable test
failure.

Consequences to respect:

- Two values of the same instrument share an exponent, so they compare and
  subtract directly.
- Multiplication adds exponents. `price * qty` overflows `i64` once accumulated,
  so VWAP style sums use `i128`.
- Division is avoided where a cross multiplication works. Comparing two moving
  averages of different lengths is `sum_a * n_b` against `sum_b * n_a`.
- Parsing rejects a non zero digit beyond the instrument's precision rather
  than rounding. Silent rounding would be a data quality bug pretending to be a
  value.

Exponents come from `exchangeInfo`, derived from `tickSize` and `stepSize` the
same way market-stream does it (`exchangeinfo.go:194-203`). The derivation is
duplicated rather than shared, since the two projects are in different
languages; a conformance test against recorded `exchangeInfo` keeps them
honest.

## D2. `.msr.zst` recordings are the primary source, not stdout

market-stream emits events as JSON lines on stdout, mixed with operational
logs. That channel is unusable as a foundation:

- No event timestamp. The `time` field is when the log line was written, not
  when the trade happened.
- Lossy and undetectably so. The subscriber queue drops the oldest event under
  backpressure, and trades carry no sequence number.
- Book deltas are reduced to level counts, so an order book cannot be rebuilt.

The `.msr.zst` recording format carries raw Binance frames with their event
time, plus the `exchangeInfo` needed for exponents. Replaying a file is
deterministic, which is what makes benchmarks reproducible and CI stable.
stdin remains supported for a live demo, with its limitations documented at the
call site.

Reading the format in Rust is also the right first exercise for this codebase:
little endian binary framing, zstd, borrowed deserialization, fixed point
conversion.

## D3. Rules are tick driven, with sliding windows

Rules see every trade. Windows are sliding over the last N trades or the last N
nanoseconds, never aligned to clock boundaries.

- Aligned candles would produce nothing on the reference corpus, which spans
  seconds. No test data, no demo.
- Aggregating before the guest call collapses the call rate from thousands per
  second to one per minute, which erases the one thing the benchmark is meant
  to measure.
- A platform sold on low latency cannot have a component that waits for a
  minute to close before reacting.

## D4. Bucketing, if added, stays on the host

`crates/host/src/bars.rs` holds the scaffolding for OHLCV aggregation. It is
not wired in. Two decisions are already frozen there.

Bucketing belongs to the host because time segmentation is a trusted decision.
A guest rule must not be able to redefine what a minute is, nor to hold
unbounded per bucket state.

Higher timeframes fold from lower ones (1m into 5m into 15m) rather than being
recomputed from trades. Cost then does not grow with the number of configured
horizons. The ladder is expressed as multipliers, so a non dividing series like
1m/5m/7m is impossible to construct.

## D5. Flat scalar ABI for v0

```
init(price_exp: i32, qty_exp: i32) -> i32
on_trade(ts_ns: i64, price: i64, qty: i64, is_buy: i32) -> i32
```

No imports, no memory sharing, no component model. Scalars cross the boundary
in registers, which keeps the per call cost close to the floor and makes the
benchmark measure the boundary rather than a serialization scheme.

This is a deliberate floor, not an end state. The component model becomes worth
its cost when arguments stop fitting comfortably in a signature, which happens
at the first `on_bar` (eight or nine scalars). Migrating then is a contained
change: the host already owns instance lifecycle, and rule logic already lives
in a separate crate from its ABI shim.

## D6. One instance per (rule, symbol)

Rule state is per instrument, and wasm32 guests are single threaded with a flat
linear memory. Giving each pair its own `Store` means a rule never has to
implement its own symbol keyed map, and a trap isolates to one instrument.

Modules are compiled once per rule and instantiated per symbol. Exported
functions are resolved to `TypedFunc` at instantiation and reused; a lookup per
call would dominate the measurement.

## D7. Containment is the feature, so hostile rules ship with it

Fuel is metered and the budget is refilled before each `on_trade`. Memory is
capped through `StoreLimits`. A trap, whether from exhausted fuel, a refused
allocation or a guest panic, disables that instance, increments a counter, and
never interrupts the other rules.

`rules/test-loop` (infinite loop) and `rules/test-mem-hog` (unbounded growth)
are part of the shipped rule set, not throwaway fixtures. Their test asserts
that the host survives, that the offending instance is disabled, and that no
trade is lost for the other rules. Without them the isolation claim is
unverified.

## D8. Benchmark methodology

Three measurements, all with criterion:

1. MSR read plus `aggTrade` decode, in records per second.
2. `sma-cross` native against WASM, in nanoseconds per call. Both sides call
   the same code: `rule-logic` is compiled into the host and into the wasm
   module, so the difference is the boundary and the fuel accounting, not two
   implementations.
3. Cost of fuel metering, WASM with and against without.

Numbers published in the README come from a local machine with its hardware
stated. CI runs `cargo bench --no-run` only: a shared runner produces noise, and
a benchmark nobody can reproduce is worse than none.

## Known limits

- A rule only runs when a trade arrives. Nothing can fire on the absence of
  events, which makes a `trade-gap` rule late by one trade and unbounded in
  wall clock time on an illiquid symbol. A periodic host tick is the fix and is
  not in v0.
- Order books are not available, since market-stream does not expose full
  depth. Spread and book imbalance rules are therefore out of scope. The
  upstream hook exists (a new `Subscriber`), but it is Go side work.
- Event time from Binance has millisecond resolution and may be absent, in
  which case market-stream leaves it at zero. Such trades are dropped from any
  time based path explicitly rather than bucketed at the epoch.
- Rule parameters are compile time constants in v0. Passing configuration
  through `init` is straightforward but adds surface before the core is proven.
- Trades arriving out of order are dropped and counted. A closed window is
  never reopened.

## Roadmap sketch

Not commitments, ordered by what the current design makes cheap:

1. Rule parameters through `init`.
2. Periodic host tick, unlocking time based rules.
3. `on_bar` and the bar builder, moving to the component model at the same
   time.
4. Live source against a structured market-stream subscriber.
5. Multi threaded execution, one shard per symbol group.
