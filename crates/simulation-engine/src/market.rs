//! Indexes a normalized market data bundle for fast per-tick lookups, and
//! provides the [`MarketContext`] the execution models read.
//!
//! The engine never fetches raw data; it only reads the bundle handed to it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::DateTime;
use rust_decimal::Decimal;

use catalyst_contracts::MarketDataBundle;
use catalyst_execution_models::{Bar, MarketContext};

/// Parse an RFC3339 timestamp to epoch seconds.
pub fn parse_ts(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.timestamp())
}

/// Format epoch seconds back to a UTC RFC3339 string (`...Z`).
pub fn format_ts(epoch: i64) -> String {
    DateTime::from_timestamp(epoch, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

fn dec(s: &str) -> Decimal {
    s.parse().unwrap_or(Decimal::ZERO)
}

type YieldKey = (String, String, String, Option<String>);

#[derive(Debug, Default)]
pub struct BundleIndex {
    candles: HashMap<(String, String), BTreeMap<i64, Bar>>,
    by_symbol: HashMap<String, BTreeMap<i64, Decimal>>,
    funding: HashMap<(String, String), BTreeMap<i64, Decimal>>,
    gas: HashMap<String, BTreeMap<i64, Decimal>>,
    yields: HashMap<YieldKey, BTreeMap<i64, Decimal>>,
    liquidity: HashMap<(String, String), BTreeMap<i64, (Decimal, Decimal)>>,
    candle_ts: BTreeSet<i64>,
    all_ts: BTreeSet<i64>,
}

impl BundleIndex {
    pub fn build(bundle: &MarketDataBundle) -> Self {
        let mut idx = BundleIndex::default();
        for series in &bundle.candles {
            let key = (series.venue.clone(), series.symbol.clone());
            for p in &series.points {
                if let Some(ts) = parse_ts(&p.ts) {
                    let bar = Bar {
                        open: dec(&p.open),
                        high: dec(&p.high),
                        low: dec(&p.low),
                        close: dec(&p.close),
                        volume: p.volume.as_deref().map(dec),
                    };
                    idx.candles.entry(key.clone()).or_default().insert(ts, bar);
                    idx.by_symbol.entry(series.symbol.clone()).or_default().insert(ts, bar.close);
                    idx.candle_ts.insert(ts);
                    idx.all_ts.insert(ts);
                }
            }
        }
        for series in &bundle.funding {
            let key = (series.venue.clone(), series.symbol.clone());
            for p in &series.points {
                if let Some(ts) = parse_ts(&p.ts) {
                    idx.funding.entry(key.clone()).or_default().insert(ts, dec(&p.rate));
                    idx.all_ts.insert(ts);
                }
            }
        }
        for series in &bundle.gas {
            for p in &series.points {
                if let Some(ts) = parse_ts(&p.ts) {
                    idx.gas.entry(series.chain.clone()).or_default().insert(ts, dec(&p.gas_usd));
                }
            }
        }
        for series in &bundle.yields {
            let key = (
                series.protocol.clone(),
                series.asset.clone(),
                series.chain.clone(),
                series.pool.clone(),
            );
            for p in &series.points {
                if let Some(ts) = parse_ts(&p.ts) {
                    idx.yields.entry(key.clone()).or_default().insert(ts, dec(&p.apr));
                    idx.all_ts.insert(ts);
                }
            }
        }
        for series in &bundle.liquidity {
            let key = (series.venue.clone(), series.symbol.clone());
            for p in &series.points {
                if let Some(ts) = parse_ts(&p.ts) {
                    idx.liquidity
                        .entry(key.clone())
                        .or_default()
                        .insert(ts, (dec(&p.reserve_base), dec(&p.reserve_quote)));
                    idx.all_ts.insert(ts);
                }
            }
        }
        idx
    }

    /// Sorted unique strategy-driving timestamps within `[start, end]`.
    ///
    /// Candles remain the preferred simulation clock when present. That keeps
    /// coarser candle runs, such as 4h candles with hourly funding, on their bar
    /// grid while accrual functions aggregate finer data inside each bar. For
    /// non-candle strategies, funding, yields, and liquidity can drive strategy
    /// state. Gas is intentionally excluded because it is an execution cost input;
    /// using gas as a clock would make yield/funding-only signals warn on
    /// unrelated gas timestamps.
    pub fn ticks(&self, start: i64, end: i64) -> Vec<i64> {
        let candle_ticks: Vec<i64> = self.candle_ts.range(start..=end).copied().collect();
        if candle_ticks.is_empty() {
            self.all_ts.range(start..=end).copied().collect()
        } else {
            candle_ticks
        }
    }

    pub fn has_ticks(&self) -> bool {
        !self.all_ts.is_empty()
    }

    pub fn bar_at(&self, venue: &str, symbol: &str, ts: i64) -> Option<Bar> {
        self.candles.get(&(venue.to_string(), symbol.to_string())).and_then(|m| m.get(&ts).copied())
    }

    /// The first bar strictly after `ts` for a (venue, symbol), if any.
    pub fn bar_after(&self, venue: &str, symbol: &str, ts: i64) -> Option<Bar> {
        self.candles
            .get(&(venue.to_string(), symbol.to_string()))?
            .range((ts + 1)..)
            .next()
            .map(|(_, b)| *b)
    }

    /// Sorted candle timestamps (epoch secs) for a (venue, symbol) within `[start, end]`.
    pub fn candle_ts_in(&self, venue: &str, symbol: &str, start: i64, end: i64) -> Vec<i64> {
        self.candles
            .get(&(venue.to_string(), symbol.to_string()))
            .map(|m| m.range(start..=end).map(|(t, _)| *t).collect())
            .unwrap_or_default()
    }

    /// Price for a symbol on any venue at `ts` (exact, else last known <= ts).
    pub fn price_any(&self, symbol: &str, ts: i64) -> Option<Decimal> {
        let m = self.by_symbol.get(symbol)?;
        m.get(&ts).copied().or_else(|| m.range(..=ts).next_back().map(|(_, v)| *v))
    }

    pub fn funding_at(&self, venue: &str, symbol: &str, ts: i64) -> Option<Decimal> {
        self.funding.get(&(venue.to_string(), symbol.to_string())).and_then(|m| m.get(&ts).copied())
    }

    /// Sum of funding rates in the bar `(lo_excl, hi_incl]` for a (venue, symbol).
    /// Captures every funding interval within a tick, so funding is correct even
    /// when the tick interval is coarser than the funding interval (e.g. 4h ticks
    /// over hourly funding). Zero if the series is absent or has no points there.
    pub fn funding_sum(&self, venue: &str, symbol: &str, lo_excl: i64, hi_incl: i64) -> Decimal {
        use std::ops::Bound::{Excluded, Included};
        self.funding
            .get(&(venue.to_string(), symbol.to_string()))
            .map(|m| m.range((Excluded(lo_excl), Included(hi_incl))).map(|(_, r)| *r).sum())
            .unwrap_or(Decimal::ZERO)
    }

    pub fn gas_at(&self, chain: &str, ts: i64) -> Option<Decimal> {
        let m = self.gas.get(chain)?;
        m.get(&ts).copied().or_else(|| m.range(..=ts).next_back().map(|(_, v)| *v))
    }

    pub fn apr_at(&self, key: &YieldKey, ts: i64) -> Option<Decimal> {
        let m = self.yields.get(key)?;
        m.get(&ts).copied().or_else(|| m.range(..=ts).next_back().map(|(_, v)| *v))
    }

    /// Pool reserves (base, quote) for a (venue, symbol) at `ts` (exact, else last
    /// known <= ts).
    pub fn reserves_at(&self, venue: &str, symbol: &str, ts: i64) -> Option<(Decimal, Decimal)> {
        let m = self.liquidity.get(&(venue.to_string(), symbol.to_string()))?;
        m.get(&ts).copied().or_else(|| m.range(..=ts).next_back().map(|(_, v)| *v))
    }
}

/// A read-only market view bound to a single tick.
pub struct TickContext<'a> {
    pub index: &'a BundleIndex,
    pub ts: i64,
}

impl MarketContext for TickContext<'_> {
    fn bar(&self, venue: &str, symbol: &str) -> Option<Bar> {
        self.index.bar_at(venue, symbol, self.ts)
    }
    fn next_bar(&self, venue: &str, symbol: &str) -> Option<Bar> {
        self.index.bar_after(venue, symbol, self.ts)
    }
    fn gas_usd(&self, chain: &str) -> Option<Decimal> {
        self.index.gas_at(chain, self.ts)
    }
    fn pool_reserves(&self, venue: &str, symbol: &str) -> Option<(Decimal, Decimal)> {
        self.index.reserves_at(venue, symbol, self.ts)
    }
}
