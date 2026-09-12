//! OHLCV bar aggregation.
//!
//! Status: scaffolding only. The types below are the intended shape of the
//! feature; the bodies are deliberately unimplemented. Rules in v0.1 are
//! tick-driven and never see bars. See `docs/design.md` for why.
//!
//! Where this fits once implemented:
//!
//!     TickSource -> [BarBuilder] -> Engine
//!                        |
//!                        +-> closed bars -> chart export / `on_bar` rules
//!
//! The builder lives on the host side on purpose. Time bucketing is a trusted
//! decision: a guest rule must not be able to redefine what a minute is, nor
//! to hold unbounded per-bucket state.

#![allow(dead_code)]

use crate::model::Trade;

/// One OHLCV bucket for a single instrument.
///
/// Prices use the instrument's price exponent, volumes its quantity exponent,
/// exactly like `Trade`. No floats anywhere: bars must be reproducible bit for
/// bit across machines, which is the same reason rules use fixed point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bar {
    pub symbol_id: u32,
    /// Start of the bucket, aligned on `period_ns`. Derived from the trade's
    /// exchange timestamp, never from the local receive timestamp.
    pub open_ts_ns: i64,
    pub period_ns: i64,
    pub open: i64,
    pub high: i64,
    pub low: i64,
    pub close: i64,
    /// Total traded quantity in the bucket.
    pub volume: i64,
    /// Share of `volume` where the taker was the buyer (`is_buy`). Kept so a
    /// future `taker-imbalance` on bars does not need the raw trades.
    pub buy_volume: i64,
    pub trade_count: u32,
}

impl Bar {
    /// Opens a bucket from its first trade.
    pub fn open_from(_trade: &Trade, _period_ns: i64) -> Self {
        // open = high = low = close = trade.price
        // volume = trade.qty, buy_volume = if trade.is_buy { qty } else { 0 }
        // open_ts_ns = align(trade.exchange_ts_ns, period_ns)
        todo!("Bar::open_from")
    }

    /// Folds one more trade into an already open bucket.
    pub fn update(&mut self, _trade: &Trade) {
        // high = max(high, price); low = min(low, price); close = price
        // volume and buy_volume accumulate; trade_count += 1
        //
        // Overflow: volume is i64 in qty fixed point. A 15 minute bucket on a
        // liquid pair stays far from i64::MAX, but the folding path below sums
        // several buckets, so use checked_add there rather than assuming.
        todo!("Bar::update")
    }

    /// Folds a finished child bar into its parent (1m -> 5m -> 15m).
    ///
    /// This is what keeps the cost independent of how many horizons are
    /// configured: higher timeframes are never recomputed from raw trades.
    /// `child` must be contiguous and strictly later than `self`; callers own
    /// that invariant.
    pub fn merge(&mut self, _child: &Bar) {
        // open and open_ts_ns: keep self's (the parent opened first)
        // high = max, low = min, close = child.close
        // volume, buy_volume, trade_count: sum
        // period_ns stays the parent's period
        todo!("Bar::merge")
    }
}

/// Start of the bucket containing `ts_ns`, for a bucket length of `period_ns`.
///
/// `rem_euclid` rather than `%` so pre-epoch timestamps do not round the wrong
/// way. Not expected in practice, but the wrong answer would be silent.
#[inline]
fn align(ts_ns: i64, period_ns: i64) -> i64 {
    ts_ns - ts_ns.rem_euclid(period_ns)
}

/// How a bucket with no trades is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapPolicy {
    /// Emit nothing. Consumers see a jump in `open_ts_ns`.
    Skip,
    /// Emit a flat bar at the previous close, with zero volume. Needed by most
    /// charting front ends, which expect a contiguous series.
    FillFlat,
}

/// Per instrument, per level, the bucket currently being filled.
struct Level {
    period_ns: i64,
    /// How many child bars fold into one bar at this level. 1 for the base.
    fold_factor: u32,
    current: Option<Bar>,
    /// Children folded into `current` so far, to know when it is complete.
    folded: u32,
}

/// Builds a ladder of aligned OHLCV bars from a trade stream.
///
/// Construction is a ladder of multipliers rather than a list of durations, so
/// that every level divides its parent exactly: `new(60s, &[5, 3])` yields
/// 1m, 5m and 15m. A list like `[1m, 5m, 7m]` would not fold cleanly and is
/// rejected by construction.
pub struct BarBuilder {
    base_period_ns: i64,
    gap_policy: GapPolicy,
    /// Indexed by symbol id, then by level (0 = base).
    levels: Vec<Vec<Level>>,
    /// Bars closed by the last `push`, cleared at the start of each call.
    closed: Vec<Bar>,
}

impl BarBuilder {
    pub fn new(_base_period_ns: i64, _fold_factors: &[u32], _gap_policy: GapPolicy) -> Self {
        // fold_factors must all be >= 2. A factor of 1 would make a level
        // alias its parent and close twice on the same trade.
        todo!("BarBuilder::new")
    }

    /// Feeds one trade and returns the bars closed by it, base level first.
    ///
    /// Ordering note: a bar closes when the *next* bucket's first trade
    /// arrives, so bars are emitted late by one trade. On an illiquid symbol
    /// that lateness is unbounded in wall clock terms. Fixing it properly
    /// requires a periodic host tick, which is the same gap already noted for
    /// the `trade-gap` rule.
    pub fn push(&mut self, _trade: &Trade) -> &[Bar] {
        // 1. Trades with exchange_ts_ns == 0 have no event time (market-stream
        //    leaves it at 0 when Binance's `E` is missing). They must be
        //    dropped here and counted, never bucketed into 1970.
        // 2. bucket = align(ts, base_period_ns)
        // 3. If it differs from the current base bucket, close the base bar,
        //    push it to `closed`, then fold it into level 1; if level 1 is now
        //    complete, close and fold it into level 2, and so on up the ladder.
        // 4. Open or update the base bucket with this trade.
        //
        // Out of order trades: Binance's aggTrade stream is monotonic per
        // symbol, but market-stream's publisher is fed by the decoder and is
        // lossy under backpressure, so a late trade is possible. v0 policy:
        // drop anything older than the current bucket and count it. Do not
        // silently reopen a closed bar.
        todo!("BarBuilder::push")
    }

    /// Closes every open bucket at end of stream.
    ///
    /// Partial bars are emitted as is. Whether a consumer should trust them is
    /// its own decision, so mark them via `trade_count` rather than hiding
    /// them: replaying a finite `.msr.zst` would otherwise always lose the
    /// tail.
    pub fn flush(&mut self) -> &[Bar] {
        todo!("BarBuilder::flush")
    }
}

// Guest side, when bars eventually reach rules. Adding this is additive: the
// host calls `on_bar` only on modules that export it, so every existing
// tick-only rule keeps working untouched.
//
//     on_bar(open_ts_ns: i64, period_ns: i64,
//            open: i64, high: i64, low: i64, close: i64,
//            volume: i64, trade_count: i32) -> i32
//
// Nine scalar arguments is the point where the flat v0 ABI starts to hurt.
// That is the natural moment to move to the component model and pass a real
// record instead, not before.

#[cfg(test)]
mod tests {
    // Cases to cover once implemented:
    //
    // - align() on a bucket boundary is a fixed point, and one nanosecond
    //   earlier lands in the previous bucket.
    // - A single trade produces open == high == low == close.
    // - Three 1m bars folding into a 5m bar: open from the first, close from
    //   the last, high and low as extrema, volume as the sum.
    // - Folding the base level by hand must equal building the higher level
    //   directly from the trades. Good proptest target.
    // - exchange_ts_ns == 0 is dropped and counted, not bucketed.
    // - flush() emits the partial tail bar exactly once.
}
