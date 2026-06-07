//! Loader tests: write a Parquet store mirroring the #30 layout, then load it.

use std::fs::{self, File};
use std::path::Path;
use std::sync::Arc;

use arrow::array::{ArrayRef, StringArray, TimestampMicrosecondArray};
use arrow::datatypes::{Field, Schema};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;

use catalyst_market_data_loader::{load_bundle, BundleRef, CandleReq, DataRequirements, GasReq};

const H0: i64 = 1_704_067_200_000_000; // 2024-01-01T00:00:00Z in micros
const HOUR: i64 = 3_600_000_000;

fn write_parquet(path: &Path, cols: &[(&str, ArrayRef)]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let fields: Vec<Field> = cols
        .iter()
        .map(|(name, arr)| Field::new(*name, arr.data_type().clone(), true))
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let arrays: Vec<ArrayRef> = cols.iter().map(|(_, a)| a.clone()).collect();
    let batch = RecordBatch::try_new(schema.clone(), arrays).unwrap();
    let file = File::create(path).unwrap();
    let mut writer = ArrowWriter::try_new(file, schema, None).unwrap();
    writer.write(&batch).unwrap();
    writer.close().unwrap();
}

fn ts_col(micros: &[i64]) -> ArrayRef {
    Arc::new(TimestampMicrosecondArray::from(micros.to_vec()).with_timezone("UTC"))
}

fn str_col(vals: &[&str]) -> ArrayRef {
    Arc::new(StringArray::from(vals.to_vec()))
}

fn write_candles(
    root: &Path,
    venue: &str,
    symbol: &str,
    interval: &str,
    date: &str,
    micros: &[i64],
) {
    let dir = root
        .join("candles")
        .join(format!("venue={venue}"))
        .join(format!("symbol={symbol}"))
        .join(format!("interval={interval}"));
    let closes: Vec<String> = micros
        .iter()
        .enumerate()
        .map(|(i, _)| format!("{}", 2000 + i))
        .collect();
    let closes_ref: Vec<&str> = closes.iter().map(|s| s.as_str()).collect();
    write_parquet(
        &dir.join(format!("{date}.parquet")),
        &[
            ("ts", ts_col(micros)),
            ("open", str_col(&closes_ref)),
            ("high", str_col(&closes_ref)),
            ("low", str_col(&closes_ref)),
            ("close", str_col(&closes_ref)),
            ("volume", str_col(&vec!["1"; micros.len()])),
        ],
    );
}

#[test]
fn loads_candles_and_gas_within_window() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_candles(
        root,
        "base",
        "ETH",
        "1h",
        "2024-01-01",
        &[H0, H0 + HOUR, H0 + 2 * HOUR],
    );
    let gas_dir = root.join("gas").join("chain=base");
    write_parquet(
        &gas_dir.join("2024-01-01.parquet"),
        &[("ts", ts_col(&[H0])), ("gas_usd", str_col(&["0.02"]))],
    );

    let bundle_ref = BundleRef {
        root: root.to_string_lossy().to_string(),
        data_requirements: DataRequirements {
            candles: vec![CandleReq {
                venue: "base".into(),
                symbol: "ETH".into(),
            }],
            gas: vec![GasReq {
                chain: "base".into(),
            }],
            ..Default::default()
        },
    };
    let bundle = pollster::block_on(load_bundle(
        &bundle_ref,
        "2024-01-01T00:00:00Z",
        "2024-01-01T03:00:00Z",
        "1h",
    ))
    .unwrap();

    assert_eq!(bundle.candles.len(), 1);
    let series = &bundle.candles[0];
    assert_eq!(series.venue, "base");
    assert_eq!(series.points.len(), 3);
    assert_eq!(series.points[0].ts, "2024-01-01T00:00:00Z");
    assert_eq!(series.points[0].close, "2000");
    assert_eq!(series.points[0].volume.as_deref(), Some("1"));

    assert_eq!(bundle.gas.len(), 1);
    assert_eq!(bundle.gas[0].points[0].gas_usd, "0.02");

    assert!(bundle.warnings.is_empty());
    assert!(bundle
        .providers
        .iter()
        .all(|p| p.coverage.as_ref().unwrap().complete == Some(true)));
}

#[test]
fn window_filters_rows_outside_range() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_candles(
        root,
        "base",
        "ETH",
        "1h",
        "2024-01-01",
        &[H0, H0 + HOUR, H0 + 2 * HOUR, H0 + 3 * HOUR],
    );

    let bundle_ref = BundleRef {
        root: root.to_string_lossy().to_string(),
        data_requirements: DataRequirements {
            candles: vec![CandleReq {
                venue: "base".into(),
                symbol: "ETH".into(),
            }],
            ..Default::default()
        },
    };
    // ask for only the first two hours
    let bundle = pollster::block_on(load_bundle(
        &bundle_ref,
        "2024-01-01T00:00:00Z",
        "2024-01-01T01:00:00Z",
        "1h",
    ))
    .unwrap();
    assert_eq!(bundle.candles[0].points.len(), 2);
}

#[test]
fn missing_series_warns_and_is_incomplete() {
    let tmp = tempfile::tempdir().unwrap();
    let bundle_ref = BundleRef {
        root: tmp.path().to_string_lossy().to_string(),
        data_requirements: DataRequirements {
            candles: vec![CandleReq {
                venue: "base".into(),
                symbol: "ETH".into(),
            }],
            ..Default::default()
        },
    };
    let bundle = pollster::block_on(load_bundle(
        &bundle_ref,
        "2024-01-01T00:00:00Z",
        "2024-01-01T03:00:00Z",
        "1h",
    ))
    .unwrap();
    assert!(bundle.candles[0].points.is_empty());
    assert!(bundle
        .warnings
        .iter()
        .any(|w| w.contains("no candles for ETH")));
    assert_eq!(
        bundle.providers[0].coverage.as_ref().unwrap().complete,
        Some(false)
    );
}

#[test]
fn round_trips_through_the_contract() {
    let tmp = tempfile::tempdir().unwrap();
    write_candles(tmp.path(), "base", "ETH", "1h", "2024-01-01", &[H0]);
    let bundle_ref = BundleRef {
        root: tmp.path().to_string_lossy().to_string(),
        data_requirements: DataRequirements {
            candles: vec![CandleReq {
                venue: "base".into(),
                symbol: "ETH".into(),
            }],
            ..Default::default()
        },
    };
    let bundle = pollster::block_on(load_bundle(
        &bundle_ref,
        "2024-01-01T00:00:00Z",
        "2024-01-01T01:00:00Z",
        "1h",
    ))
    .unwrap();
    let json = serde_json::to_string(&bundle).unwrap();
    let _back: catalyst_contracts::MarketDataBundle = serde_json::from_str(&json).unwrap();
}

#[test]
fn reads_via_explicit_file_url() {
    // Same data, but addressed through an object_store URL (file://) rather than a
    // bare path — proves the object_store URL path that s3:// / gs:// also use.
    let tmp = tempfile::tempdir().unwrap();
    write_candles(
        tmp.path(),
        "base",
        "ETH",
        "1h",
        "2024-01-01",
        &[H0, H0 + HOUR],
    );
    let url = url::Url::from_directory_path(tmp.path()).unwrap();
    let bundle_ref = BundleRef {
        root: url.to_string(),
        data_requirements: DataRequirements {
            candles: vec![CandleReq {
                venue: "base".into(),
                symbol: "ETH".into(),
            }],
            ..Default::default()
        },
    };
    let bundle = pollster::block_on(load_bundle(
        &bundle_ref,
        "2024-01-01T00:00:00Z",
        "2024-01-01T02:00:00Z",
        "1h",
    ))
    .unwrap();
    assert_eq!(bundle.candles[0].points.len(), 2);
    assert_eq!(bundle.candles[0].points[0].close, "2000");
}

// --- provider provenance (#38) ---

#[test]
fn candle_providers_carry_provenance_from_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_candles(root, "hyperliquid", "ETH", "1h", "2024-01-01", &[H0, H0 + HOUR]);
    write_candles(root, "base", "ETH", "1h", "2024-01-01", &[H0, H0 + HOUR]);
    // manifest marks the HL series native; base has no entry -> defaults reference
    fs::write(
        root.join("_provenance.json"),
        r#"{"candles/hyperliquid/ETH": "native"}"#,
    )
    .unwrap();

    let bundle_ref = BundleRef {
        root: root.to_string_lossy().to_string(),
        data_requirements: DataRequirements {
            candles: vec![
                CandleReq { venue: "hyperliquid".into(), symbol: "ETH".into() },
                CandleReq { venue: "base".into(), symbol: "ETH".into() },
            ],
            ..Default::default()
        },
    };
    let bundle = pollster::block_on(load_bundle(
        &bundle_ref,
        "2024-01-01T00:00:00Z",
        "2024-01-01T03:00:00Z",
        "1h",
    ))
    .unwrap();

    let prov = |venue: &str| {
        bundle
            .providers
            .iter()
            .find(|p| p.kind == "candles" && p.venue.as_deref() == Some(venue))
            .and_then(|p| p.provenance.clone())
            .unwrap()
    };
    assert_eq!(prov("hyperliquid"), "native");
    assert_eq!(prov("base"), "reference"); // default when not in the manifest
}

// --- liquidity / pool reserves (#40) ---

#[test]
fn loads_liquidity_series_for_candle_venue() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_candles(root, "base", "ETH", "1h", "2024-01-01", &[H0, H0 + HOUR]);
    let liq_dir = root.join("liquidity").join("venue=base").join("symbol=ETH");
    write_parquet(
        &liq_dir.join("2024-01-01.parquet"),
        &[
            ("ts", ts_col(&[H0, H0 + HOUR])),
            ("reserve_base", str_col(&["100", "100"])),
            ("reserve_quote", str_col(&["200000", "201000"])),
        ],
    );

    let bundle_ref = BundleRef {
        root: root.to_string_lossy().to_string(),
        data_requirements: DataRequirements {
            candles: vec![CandleReq { venue: "base".into(), symbol: "ETH".into() }],
            ..Default::default()
        },
    };
    let bundle = pollster::block_on(load_bundle(
        &bundle_ref,
        "2024-01-01T00:00:00Z",
        "2024-01-01T03:00:00Z",
        "1h",
    ))
    .unwrap();

    assert_eq!(bundle.liquidity.len(), 1);
    let s = &bundle.liquidity[0];
    assert_eq!((s.venue.as_str(), s.symbol.as_str()), ("base", "ETH"));
    assert_eq!(s.points.len(), 2);
    assert_eq!(s.points[0].reserve_base, "100");
    assert_eq!(s.points[1].reserve_quote, "201000");
}

// --- intra-series coverage (#42) ---

#[test]
fn series_coverage_contiguous_is_complete() {
    use catalyst_market_data_loader::series_coverage;
    let ts: Vec<i64> = (0..5).map(|i| H0 + i * HOUR).collect();
    let cov = series_coverage(&ts, 3600);
    assert_eq!(cov.present, 5);
    assert_eq!(cov.missing, 0);
    assert_eq!(cov.completeness_pct, 100.0);
    assert!(cov.missing_ranges.is_empty());
}

#[test]
fn series_coverage_detects_interior_hole() {
    use catalyst_market_data_loader::series_coverage;
    // present at hours 0,1,3,4 -> hour 2 is an interior hole
    let ts = vec![H0, H0 + HOUR, H0 + 3 * HOUR, H0 + 4 * HOUR];
    let cov = series_coverage(&ts, 3600);
    assert_eq!(cov.present, 4);
    assert_eq!(cov.expected, 5); // 0..=4
    assert_eq!(cov.missing, 1);
    assert_eq!(cov.completeness_pct, 80.0);
    assert_eq!(cov.missing_ranges.len(), 1);
    // the single missing bucket is hour 2
    assert_eq!(cov.missing_ranges[0].0, "2024-01-01T02:00:00Z");
    assert_eq!(cov.missing_ranges[0].1, "2024-01-01T02:00:00Z");
}

#[test]
fn series_coverage_multi_bucket_gap() {
    use catalyst_market_data_loader::series_coverage;
    // hours 0 then 4 -> hours 1,2,3 missing (3 buckets)
    let ts = vec![H0, H0 + 4 * HOUR];
    let cov = series_coverage(&ts, 3600);
    assert_eq!(cov.missing, 3);
    assert_eq!(cov.missing_ranges[0].0, "2024-01-01T01:00:00Z");
    assert_eq!(cov.missing_ranges[0].1, "2024-01-01T03:00:00Z");
}
