//! Reads the Parquet historical market-data store directly into a
//! [`catalyst_contracts::MarketDataBundle`], so the simulation can be driven by a
//! dataset *reference* + window instead of a bundle serialized over the wire.
//!
//! The store layout is the cross-language contract documented in
//! `docs/market-data-storage.md` and written by the Python ingesters
//! (`packages/market-data`, issue #30):
//!
//! ```text
//! <root>/candles/venue=<v>/symbol=<s>/interval=<i>/<YYYY-MM-DD>.parquet
//! <root>/funding/venue=<v>/symbol=<s>/<YYYY-MM-DD>.parquet
//! <root>/gas/chain=<c>/<YYYY-MM-DD>.parquet
//! <root>/yields/protocol=<p>/asset=<a>/chain=<c>/pool=<pool|_none>/<YYYY-MM-DD>.parquet
//! ```
//!
//! Value columns are decimal-strings (matching the contract), `ts` is
//! `timestamp[us, UTC]`. Reads are partition-pruned by date and window-filtered
//! by timestamp — only the needed files/rows are touched. Local filesystem only
//! for now (object_store / S3 is a later step).

use std::fmt;
use std::path::{Path, PathBuf};

use arrow::array::{Array, StringArray, TimestampMicrosecondArray};
use chrono::{DateTime, NaiveDate, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::{Deserialize, Serialize};

use catalyst_contracts::market_data::{
    Candle, CandleSeries, Coverage, FundingPoint, FundingSeries, GasPoint, GasSeries, Provider,
    YieldPoint, YieldSeries,
};
use catalyst_contracts::MarketDataBundle;

// --- Data requirements (mirror the Python compiler's output; data, not logic) ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CandleReq {
    pub venue: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FundingReq {
    pub venue: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GasReq {
    pub chain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YieldReq {
    pub protocol: String,
    pub asset: String,
    pub chain: String,
    #[serde(default)]
    pub pool: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataRequirements {
    #[serde(default)]
    pub candles: Vec<CandleReq>,
    #[serde(default)]
    pub funding: Vec<FundingReq>,
    #[serde(default)]
    pub gas: Vec<GasReq>,
    #[serde(default)]
    pub yields: Vec<YieldReq>,
}

/// A reference to a dataset in the Parquet store plus what to load from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleRef {
    pub root: String,
    pub data_requirements: DataRequirements,
}

#[derive(Debug)]
pub enum LoaderError {
    Time(String),
    Read(String),
}

impl fmt::Display for LoaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoaderError::Time(m) => write!(f, "bad timestamp: {m}"),
            LoaderError::Read(m) => write!(f, "parquet read error: {m}"),
        }
    }
}
impl std::error::Error for LoaderError {}

fn parse_micros(rfc3339: &str) -> Result<i64, LoaderError> {
    DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| dt.with_timezone(&Utc).timestamp_micros())
        .map_err(|e| LoaderError::Time(format!("{rfc3339}: {e}")))
}

fn micros_to_iso(micros: i64) -> String {
    DateTime::<Utc>::from_timestamp_micros(micros)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

/// A row: (ts micros, value columns as optional decimal-strings, in `value_cols` order).
type Row = (i64, Vec<Option<String>>);

/// Read every `*.parquet` in `dir` whose filename date falls in the window, and
/// return rows whose `ts` is within `[start_us, end_us]`, sorted by `ts`.
fn read_window(
    dir: &Path,
    value_cols: &[&str],
    start_us: i64,
    end_us: i64,
) -> Result<Vec<Row>, LoaderError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let start_date = DateTime::<Utc>::from_timestamp_micros(start_us).map(|d| d.date_naive());
    let end_date = DateTime::<Utc>::from_timestamp_micros(end_us).map(|d| d.date_naive());

    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| LoaderError::Read(e.to_string()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "parquet").unwrap_or(false))
        .collect();
    files.sort();

    let mut rows: Vec<Row> = Vec::new();
    for path in files {
        // Partition pruning by the file's date stem.
        if let (Some(stem), Some(sd), Some(ed)) = (path.file_stem(), start_date, end_date) {
            if let Ok(file_date) = stem.to_string_lossy().parse::<NaiveDate>() {
                if file_date < sd || file_date > ed {
                    continue;
                }
            }
        }
        read_file(&path, value_cols, start_us, end_us, &mut rows)?;
    }
    rows.sort_by_key(|(ts, _)| *ts);
    Ok(rows)
}

fn read_file(
    path: &Path,
    value_cols: &[&str],
    start_us: i64,
    end_us: i64,
    out: &mut Vec<Row>,
) -> Result<(), LoaderError> {
    let file = std::fs::File::open(path).map_err(|e| LoaderError::Read(e.to_string()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| LoaderError::Read(e.to_string()))?
        .build()
        .map_err(|e| LoaderError::Read(e.to_string()))?;

    for batch in reader {
        let batch = batch.map_err(|e| LoaderError::Read(e.to_string()))?;
        let ts = batch
            .column_by_name("ts")
            .and_then(|c| c.as_any().downcast_ref::<TimestampMicrosecondArray>())
            .ok_or_else(|| LoaderError::Read("missing/!timestamp ts column".into()))?;
        let cols: Vec<&StringArray> = value_cols
            .iter()
            .map(|name| {
                batch
                    .column_by_name(name)
                    .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                    .ok_or_else(|| LoaderError::Read(format!("missing/!string column {name}")))
            })
            .collect::<Result<_, _>>()?;

        for i in 0..batch.num_rows() {
            let t = ts.value(i);
            if t < start_us || t > end_us {
                continue;
            }
            let values = cols
                .iter()
                .map(|c| if c.is_null(i) { None } else { Some(c.value(i).to_string()) })
                .collect();
            out.push((t, values));
        }
    }
    Ok(())
}

fn req(s: &str) -> String {
    s.to_string()
}

/// Load a [`MarketDataBundle`] from the store for the given requirements + window.
pub fn load_bundle(
    bundle_ref: &BundleRef,
    start: &str,
    end: &str,
    interval: &str,
) -> Result<MarketDataBundle, LoaderError> {
    let root = Path::new(&bundle_ref.root);
    let start_us = parse_micros(start)?;
    let end_us = parse_micros(end)?;
    let reqs = &bundle_ref.data_requirements;

    let mut warnings: Vec<String> = Vec::new();
    let mut providers: Vec<Provider> = Vec::new();
    let coverage = |complete: bool| Coverage {
        start: Some(start.to_string()),
        end: Some(end.to_string()),
        complete: Some(complete),
    };

    // candles
    let mut candles = Vec::new();
    for r in &reqs.candles {
        let dir = root
            .join("candles")
            .join(format!("venue={}", r.venue))
            .join(format!("symbol={}", r.symbol))
            .join(format!("interval={interval}"));
        let rows = read_window(&dir, &["open", "high", "low", "close", "volume"], start_us, end_us)?;
        if rows.is_empty() {
            warnings.push(format!("no candles for {} on {} from 'parquet-store'", r.symbol, r.venue));
        }
        candles.push(CandleSeries {
            venue: req(&r.venue),
            symbol: req(&r.symbol),
            quote: "USD".to_string(),
            points: rows
                .into_iter()
                .map(|(t, v)| Candle {
                    ts: micros_to_iso(t),
                    open: v[0].clone().unwrap_or_default(),
                    high: v[1].clone().unwrap_or_default(),
                    low: v[2].clone().unwrap_or_default(),
                    close: v[3].clone().unwrap_or_default(),
                    volume: v[4].clone(),
                })
                .collect(),
        });
    }
    if !reqs.candles.is_empty() {
        providers.push(provider("candles", &candles.iter().map(|s| !s.points.is_empty()).collect::<Vec<_>>(), &coverage));
    }

    // funding
    let mut funding = Vec::new();
    for r in &reqs.funding {
        let dir = root.join("funding").join(format!("venue={}", r.venue)).join(format!("symbol={}", r.symbol));
        let rows = read_window(&dir, &["rate"], start_us, end_us)?;
        if rows.is_empty() {
            warnings.push(format!("no funding for {} on {} from 'parquet-store'", r.symbol, r.venue));
        }
        funding.push(FundingSeries {
            venue: req(&r.venue),
            symbol: req(&r.symbol),
            points: rows
                .into_iter()
                .map(|(t, v)| FundingPoint { ts: micros_to_iso(t), rate: v[0].clone().unwrap_or_default() })
                .collect(),
        });
    }
    if !reqs.funding.is_empty() {
        providers.push(provider("funding", &funding.iter().map(|s| !s.points.is_empty()).collect::<Vec<_>>(), &coverage));
    }

    // gas
    let mut gas = Vec::new();
    for r in &reqs.gas {
        let dir = root.join("gas").join(format!("chain={}", r.chain));
        let rows = read_window(&dir, &["gas_usd"], start_us, end_us)?;
        if rows.is_empty() {
            warnings.push(format!("no gas for {} from 'parquet-store'", r.chain));
        }
        gas.push(GasSeries {
            chain: req(&r.chain),
            points: rows
                .into_iter()
                .map(|(t, v)| GasPoint { ts: micros_to_iso(t), gas_usd: v[0].clone().unwrap_or_default() })
                .collect(),
        });
    }
    if !reqs.gas.is_empty() {
        providers.push(provider("gas", &gas.iter().map(|s| !s.points.is_empty()).collect::<Vec<_>>(), &coverage));
    }

    // yields
    let mut yields = Vec::new();
    for r in &reqs.yields {
        let pool = r.pool.clone().unwrap_or_else(|| "_none".to_string());
        let dir = root
            .join("yields")
            .join(format!("protocol={}", r.protocol))
            .join(format!("asset={}", r.asset))
            .join(format!("chain={}", r.chain))
            .join(format!("pool={pool}"));
        let rows = read_window(&dir, &["apr"], start_us, end_us)?;
        if rows.is_empty() {
            warnings.push(format!("no yields for {}/{} on {} from 'parquet-store'", r.protocol, r.asset, r.chain));
        }
        yields.push(YieldSeries {
            protocol: req(&r.protocol),
            asset: req(&r.asset),
            chain: req(&r.chain),
            pool: r.pool.clone(),
            points: rows
                .into_iter()
                .map(|(t, v)| YieldPoint { ts: micros_to_iso(t), apr: v[0].clone().unwrap_or_default() })
                .collect(),
        });
    }
    if !reqs.yields.is_empty() {
        providers.push(provider("yields", &yields.iter().map(|s| !s.points.is_empty()).collect::<Vec<_>>(), &coverage));
    }

    Ok(MarketDataBundle {
        schema_version: "catalyst.backtest.market_data_bundle.v1".to_string(),
        interval: interval.to_string(),
        start: start.to_string(),
        end: end.to_string(),
        candles,
        funding,
        gas,
        yields,
        providers,
        warnings,
    })
}

fn provider(kind: &str, non_empty: &[bool], coverage: &dyn Fn(bool) -> Coverage) -> Provider {
    let complete = !non_empty.is_empty() && non_empty.iter().all(|&b| b);
    Provider { name: "parquet-store".to_string(), kind: kind.to_string(), coverage: Some(coverage(complete)) }
}
