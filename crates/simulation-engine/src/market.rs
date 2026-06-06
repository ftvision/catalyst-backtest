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
                    };
                    idx.candles.entry(key.clone()).or_default().insert(ts, bar);
                    idx.by_symbol.entry(series.symbol.clone()).or_default().insert(ts, bar.close);
                    idx.all_ts.insert(ts);
                }
            }
        }
        for series in &bundle.funding {
            let key = (series.venue.clone(), series.symbol.clone());
            for p in &series.points {
                if let Some(ts) = parse_ts(&p.ts) {
                    idx.funding.entry(key.clone()).or_default().insert(ts, dec(&p.rate));
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
                }
            }
        }
        idx
    }

    /// Sorted unique candle timestamps within `[start, end]` — the tick clock.
    pub fn ticks(&self, start: i64, end: i64) -> Vec<i64> {
        self.all_ts.range(start..=end).copied().collect()
    }

    pub fn has_ticks(&self) -> bool {
        !self.all_ts.is_empty()
    }

    pub fn bar_at(&self, venue: &str, symbol: &str, ts: i64) -> Option<Bar> {
        self.candles.get(&(venue.to_string(), symbol.to_string())).and_then(|m| m.get(&ts).copied())
    }

    /// Price for a symbol on any venue at `ts` (exact, else last known <= ts).
    pub fn price_any(&self, symbol: &str, ts: i64) -> Option<Decimal> {
        let m = self.by_symbol.get(symbol)?;
        m.get(&ts).copied().or_else(|| m.range(..=ts).next_back().map(|(_, v)| *v))
    }

    pub fn funding_at(&self, venue: &str, symbol: &str, ts: i64) -> Option<Decimal> {
        self.funding.get(&(venue.to_string(), symbol.to_string())).and_then(|m| m.get(&ts).copied())
    }

    pub fn gas_at(&self, chain: &str, ts: i64) -> Option<Decimal> {
        let m = self.gas.get(chain)?;
        m.get(&ts).copied().or_else(|| m.range(..=ts).next_back().map(|(_, v)| *v))
    }

    pub fn apr_at(&self, key: &YieldKey, ts: i64) -> Option<Decimal> {
        let m = self.yields.get(key)?;
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
    fn gas_usd(&self, chain: &str) -> Option<Decimal> {
        self.index.gas_at(chain, self.ts)
    }
}
