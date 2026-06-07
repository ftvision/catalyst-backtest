import type { BacktestRequest, CandlePointApi, CatalystGraph, MarketDataBundle } from "../api/client";

const startMs = Date.UTC(2026, 2, 1, 0, 0, 0);
const hourMs = 60 * 60 * 1000;
const closes = [
  2808, 2768, 2710, 2665, 2688, 2712, 2748, 2792, 2824, 2868, 2892, 2948,
  2988, 2964, 2916, 2886, 2854, 2812, 2768, 2794, 2832, 2874, 2918, 2942,
  2988, 3014, 3045, 3004, 2976, 3022, 3068, 3096,
];

function isoAt(index: number) {
  return new Date(startMs + index * hourMs).toISOString();
}

const candlePoints: CandlePointApi[] = closes.map((close, index) => {
  const previous = index === 0 ? close - 12 : closes[index - 1];
  const open = previous + Math.round(Math.sin(index * 0.6) * 8);
  const spread = 18 + Math.round(Math.abs(Math.cos(index * 0.42)) * 22);

  return {
    ts: isoAt(index),
    open: String(open),
    high: String(Math.max(open, close) + spread),
    low: String(Math.min(open, close) - spread),
    close: String(close),
    volume: String(Math.round(180_000 + Math.abs(close - open) * 3_100 + Math.sin(index * 0.31) * 35_000)),
  };
});

export const demoGraph: CatalystGraph = {
  schema_version: "catalyst.graph.definition.v1",
  nodes: [
    {
      id: "buy",
      kind: "action",
      subtype: "swap",
      config: {
        from_asset: "USDC",
        to_asset: "ETH",
        amount: "100",
        chain: "base",
      },
    },
  ],
  edges: [],
};

export const demoConfig = {
  start: isoAt(0),
  end: isoAt(closes.length - 1),
  interval: "1h",
  initial_portfolio: {
    base: {
      USDC: "1000",
    },
  },
};

export const demoMarketData: MarketDataBundle = {
  schema_version: "catalyst.backtest.market_data_bundle.v1",
  interval: demoConfig.interval,
  start: demoConfig.start,
  end: demoConfig.end,
  candles: [
    {
      venue: "base",
      symbol: "ETH",
      quote: "USD",
      points: candlePoints,
    },
  ],
  gas: [
    {
      chain: "base",
      points: candlePoints.map((point, index) => ({
        ts: point.ts,
        gas_usd: (0.015 + Math.abs(Math.sin(index * 0.37)) * 0.035).toFixed(4),
      })),
    },
  ],
  funding: [
    {
      venue: "hyperliquid",
      symbol: "ETH",
      points: candlePoints.map((point, index) => ({
        ts: point.ts,
        rate: (Math.sin(index * 0.23) * 0.0002).toFixed(6),
      })),
    },
  ],
  providers: [
    { kind: "candles", name: "inline-demo", coverage: { start: demoConfig.start, end: demoConfig.end, complete: true } },
    { kind: "gas", name: "inline-demo", coverage: { start: demoConfig.start, end: demoConfig.end, complete: true } },
    { kind: "funding", name: "inline-demo", coverage: { start: demoConfig.start, end: demoConfig.end, complete: true } },
  ],
  warnings: [],
};

export function buildDemoBacktestRequest(policyProfile = "strict_v1"): BacktestRequest {
  return {
    graph: demoGraph,
    config: demoConfig,
    policy: { profile: policyProfile },
    market_data: demoMarketData,
  };
}

export function buildStoreBacktestRequest(policyProfile = "strict_v1"): BacktestRequest {
  return {
    graph: demoGraph,
    config: demoConfig,
    policy: { profile: policyProfile },
  };
}
