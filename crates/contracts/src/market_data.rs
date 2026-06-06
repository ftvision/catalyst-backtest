//! Normalized market data bundle contract (market-data-bundle.schema.json).

use serde::{Deserialize, Serialize};

use crate::Decimal;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candle {
    pub ts: String,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<Decimal>,
}

fn default_quote() -> String {
    "USD".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandleSeries {
    pub venue: String,
    pub symbol: String,
    #[serde(default = "default_quote")]
    pub quote: String,
    #[serde(default)]
    pub points: Vec<Candle>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FundingPoint {
    pub ts: String,
    pub rate: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FundingSeries {
    pub venue: String,
    pub symbol: String,
    #[serde(default)]
    pub points: Vec<FundingPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GasPoint {
    pub ts: String,
    pub gas_usd: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GasSeries {
    pub chain: String,
    #[serde(default)]
    pub points: Vec<GasPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YieldPoint {
    pub ts: String,
    pub apr: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YieldSeries {
    pub protocol: String,
    pub asset: String,
    pub chain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
    #[serde(default)]
    pub points: Vec<YieldPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complete: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<Coverage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketDataBundle {
    #[serde(default = "default_bundle_schema_version")]
    pub schema_version: String,
    pub interval: String,
    pub start: String,
    pub end: String,
    #[serde(default)]
    pub candles: Vec<CandleSeries>,
    #[serde(default)]
    pub funding: Vec<FundingSeries>,
    #[serde(default)]
    pub gas: Vec<GasSeries>,
    #[serde(default)]
    pub yields: Vec<YieldSeries>,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

fn default_bundle_schema_version() -> String {
    "catalyst.backtest.market_data_bundle.v1".to_string()
}
