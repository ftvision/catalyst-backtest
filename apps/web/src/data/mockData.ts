import type {
  AuditData,
  GraphSummary,
  MarketReplayData,
  ResultData,
  SetupData,
} from "../types";

const baseTime = Date.UTC(2024, 4, 11, 0, 0, 0) / 1000;
const hour = 60 * 60 * 2;

const candleSeeds = [
  2808, 2748, 2665, 2712, 2792, 2824, 2892, 2988, 2916, 2854, 2768, 2832, 2874,
  2942, 2988, 3045, 3004, 3096,
];

const marketCandles = Array.from({ length: 72 }, (_, index) => {
  const seedIndex = Math.floor((index / 71) * (candleSeeds.length - 1));
  const nextSeedIndex = Math.min(seedIndex + 1, candleSeeds.length - 1);
  const local = (index / 71) * (candleSeeds.length - 1) - seedIndex;
  const baseClose = candleSeeds[seedIndex] * (1 - local) + candleSeeds[nextSeedIndex] * local;
  const wave = Math.sin(index * 0.83) * 28 + Math.cos(index * 0.31) * 16;
  const close = Math.round(baseClose + wave);
  const open = index === 0 ? close - 18 : Math.round(baseClose + Math.sin((index - 1) * 0.83) * 24);
  const spread = 18 + Math.abs(Math.sin(index * 0.47)) * 34;
  const high = Math.round(Math.max(open, close) + spread);
  const low = Math.round(Math.min(open, close) - spread * 0.78);
  const spike = [8, 9, 26, 39, 52, 63].includes(index) ? 2.8 : 1;

  return {
    close,
    open,
    high,
    low,
    volume: Math.round((280_000 + Math.abs(close - open) * 5_400 + Math.sin(index * 0.41) * 90_000) * spike),
  };
});

const equity = marketCandles.map((_, index) =>
  Number((98 + index * 0.19 + Math.sin(index * 0.28) * 1.6 - Math.max(0, Math.sin(index * 0.11)) * 0.8).toFixed(2)),
);
const drawdown = marketCandles.map((_, index) =>
  Number((-0.35 - Math.abs(Math.sin(index * 0.22)) * 1.7 - (index > 36 && index < 51 ? 1.6 : 0)).toFixed(2)),
);
const gas = marketCandles.map((_, index) => Number((1.5 + Math.abs(Math.sin(index * 0.39)) * 1.6).toFixed(2)));
const funding = marketCandles.map((_, index) => Number((Math.sin(index * 0.27) * 0.0018).toFixed(5)));

export const graph: GraphSummary = {
  id: "g_eth_threshold_base_swap",
  hash: "9f3c7a1b",
  name: "ETH threshold -> Base swap",
  version: "1.2.0",
  updatedAt: "2026-06-06 11:24 UTC",
  status: "validated",
  nodeCount: 7,
  edgeCount: 6,
  nodes: [
    { id: "eth-below-1800", kind: "signal", label: "ETH threshold", detail: "signal crossing" },
    { id: "cooldown-15m", kind: "filter", label: "Cooldown", detail: "15m" },
    { id: "buy-eth-on-base", kind: "action", label: "Base swap", detail: "market" },
    { id: "open-eth-long-5x", kind: "action", label: "Open ETH long", detail: "Hyperliquid 5x" },
    { id: "eth-price", kind: "data", label: "ETH price", detail: "1h candles" },
    { id: "base-gas", kind: "data", label: "Base gas", detail: "historical" },
    { id: "hl-funding", kind: "data", label: "Funding", detail: "8h history" },
  ],
  edges: [
    { id: "eth-price--eth-below-1800", from: "eth-price", to: "eth-below-1800" },
    { id: "eth-below-1800--cooldown-15m", from: "eth-below-1800", to: "cooldown-15m" },
    { id: "cooldown-15m--buy-eth-on-base", from: "cooldown-15m", to: "buy-eth-on-base" },
    { id: "cooldown-15m--open-eth-long-5x", from: "cooldown-15m", to: "open-eth-long-5x" },
    { id: "base-gas--buy-eth-on-base", from: "base-gas", to: "buy-eth-on-base" },
    { id: "hl-funding--open-eth-long-5x", from: "hl-funding", to: "open-eth-long-5x" },
  ],
};

export const setup: SetupData = {
  runId: "run_2026_06_06_18_24",
  start: "2024-01-01T00:00",
  end: "2024-06-01T00:00",
  interval: "1h",
  policy: "strict_v1",
  portfolio: [
    { venue: "Base", asset: "USDC", amount: "100,000.00", percent: "50.0%" },
    { venue: "Base", asset: "ETH", amount: "50.0000", percent: "25.0%" },
    { venue: "Hyperliquid", asset: "USDC", amount: "50,000.00", percent: "25.0%" },
  ],
  coverage: [
    { kind: "Candles", source: "ETH / USDC on Base", interval: "1h", coverage: 98.3, status: "success" },
    { kind: "Funding", source: "Hyperliquid ETH", interval: "8h", coverage: 94.1, status: "success" },
    { kind: "Gas", source: "Base eth_feeHistory", interval: "1h", coverage: 63.2, status: "warning" },
    { kind: "Yield", source: "Aave LST stETH", interval: "1d", coverage: 78.6, status: "warning" },
  ],
  assumptions: [
    ["Policy profile", "Strict v1"],
    ["Fill price", "Close price"],
    ["Slippage", "fixed_bps 10"],
    ["Gas model", "historical_fee_history"],
    ["Signal trigger", "crossing"],
    ["Missing data", "missing_required fail"],
  ],
  warnings: [
    "Gas coverage on Base is incomplete for the 1h interval.",
    "Yield data for stETH has missing periods.",
    "Funding history is available, but sparse before February.",
  ],
};

export const result: ResultData = {
  status: "completed",
  createdAt: "2026-06-06 18:24 UTC",
  metrics: [
    { label: "Final value", value: "$109,371.92", detail: "Start $100,000.00" },
    { label: "Return", value: "+9.37%", detail: "+$9,371.92", tone: "positive" },
    { label: "PnL", value: "+$9,371.92", detail: "Realized +$7,812.21", tone: "positive" },
    { label: "Max DD", value: "-3.8%", detail: "-$3,842.31", tone: "negative" },
    { label: "Trades", value: "24", detail: "18 wins / 6 losses" },
    { label: "Rejected", value: "7", detail: "policy blocked" },
  ],
  equity: [98, 99, 97, 94, 92, 94, 93, 95, 96, 98, 97, 101, 103, 102, 105, 104, 107, 106, 109],
  drawdown: [0, -0.4, -0.7, -2.1, -3.5, -2.9, -3.2, -2.4, -2.1, -1.8, -2.4, -1.5, -1.1, -1.3, -0.9, -1.1, -0.7, -0.8, -0.6],
  portfolio: [
    {
      venue: "Base",
      total: "$64,212.34",
      assets: [
        { asset: "ETH", balance: "17.4521", price: "$2,908.34", value: "$50,764.91", percent: "46.4%" },
        { asset: "USDC", balance: "12,347.21", price: "$1.00", value: "$12,347.21", percent: "11.3%" },
        { asset: "cbETH", balance: "1.2387", price: "$2,512.18", value: "$3,100.22", percent: "2.8%" },
      ],
    },
    {
      venue: "Hyperliquid",
      total: "$45,159.58",
      assets: [
        { asset: "ETH-PERP", balance: "5.0000", price: "$2,908.10", value: "$14,540.50", percent: "13.3%" },
        { asset: "BTC-PERP", balance: "0.2500", price: "$66,842.00", value: "$16,710.50", percent: "15.3%" },
        { asset: "USDC", balance: "13,908.58", price: "$1.00", value: "$13,908.58", percent: "12.7%" },
      ],
    },
  ],
  timeline: [
    { time: "15:32:11", node: "eth-below-1800", signal: "signal_fired", action: "buy-eth-on-base", venue: "Base", fees: "$6.21", gas: "$1.02", pnl: "$1,234.11" },
    { time: "14:47:02", node: "take-profit-eth", signal: "signal_fired", action: "sell-eth-on-hl", venue: "Hyperliquid", fees: "$2.45", gas: "-", pnl: "$875.22" },
    { time: "13:10:44", node: "buy-eth-on-base", signal: "-", action: "rejected_action", venue: "Base", fees: "-", gas: "-", pnl: "-" },
    { time: "12:02:18", node: "rebalance", signal: "signal_fired", action: "rebalance", venue: "Base", fees: "$3.11", gas: "$0.75", pnl: "-$12.44" },
  ],
  costs: [
    { label: "Gross PnL", value: "+$1,368.03", tone: "positive", amount: 1368.03 },
    { label: "Fees", value: "-$72.41", tone: "negative", amount: -72.41 },
    { label: "Gas", value: "-$12.84", tone: "negative", amount: -12.84 },
    { label: "Slippage", value: "-$21.33", tone: "negative", amount: -21.33 },
    { label: "Funding", value: "-$5.27", tone: "negative", amount: -5.27 },
    { label: "Net PnL", value: "+$1,256.18", tone: "positive", amount: 1256.18 },
  ],
};

export const marketReplay: MarketReplayData = {
  symbol: "ETH / USDC",
  venue: "Base + Hyperliquid",
  period: "May 11 - May 18, 2024",
  selectedEventId: "evt-2",
  candles: marketCandles.map((candle, index) => ({
    time: (baseTime + index * hour) as MarketReplayData["candles"][number]["time"],
    open: candle.open,
    close: candle.close,
    high: candle.high,
    low: candle.low,
    volume: candle.volume,
  })),
  replay: equity.map((value, index) => ({
    label: `T${String(index + 1).padStart(2, "0")}`,
    equity: value,
    drawdown: drawdown[index],
    gas: gas[index],
    funding: funding[index] * 1000,
  })),
  events: [
    { id: "evt-1", index: 1, time: (baseTime + 5 * hour) as MarketReplayData["events"][number]["time"], labelTime: "May 11 10:10", kind: "signal_fired", label: "ETH below threshold", node: "eth-below-1800", status: "signal", price: "$1,797.65", impact: "-" },
    { id: "evt-2", index: 2, time: (baseTime + 24 * hour) as MarketReplayData["events"][number]["time"], labelTime: "May 13 14:30", kind: "action_executed", label: "Buy ETH on Base", node: "buy-eth-on-base", status: "executed", price: "$2,988.40", impact: "-10,000 USDC, +3.3442 ETH" },
    { id: "evt-3", index: 3, time: (baseTime + 37 * hour) as MarketReplayData["events"][number]["time"], labelTime: "May 14 14:35", kind: "rejected_action", label: "Close position rejected", node: "close-position", status: "rejected", price: "$2,991.20", impact: "insufficient balance" },
    { id: "evt-4", index: 4, time: (baseTime + 50 * hour) as MarketReplayData["events"][number]["time"], labelTime: "May 16 00:00", kind: "funding_accrued", label: "Funding accrued", node: "funding-eth-perp", status: "policy", price: "$2,923.10", impact: "+$12.34" },
    { id: "evt-5", index: 5, time: (baseTime + 63 * hour) as MarketReplayData["events"][number]["time"], labelTime: "May 18 11:02", kind: "gas_cost", label: "Base gas paid", node: "tx-gas", status: "warning", price: "$3,045.50", impact: "-$0.18" },
  ],
  evidence: [
    ["Candle close", "$1,797.65"],
    ["Base gas", "1.82 gwei"],
    ["Funding rate", "0.0031%"],
    ["Data coverage", "98.6%"],
    ["Available balance", "22,134.76 USDC"],
  ],
};

export const audit: AuditData = {
  selectedEventId: "evt-2",
  events: [
    { id: "evt-1", time: "12:01:15", kind: "signal_fired", node: "entry_signal", venue: "Base", status: "signal" },
    { id: "evt-2", time: "12:01:16", kind: "action_executed", node: "buy_eth", venue: "Base", status: "executed" },
    { id: "evt-3", time: "12:01:17", kind: "action_executed", node: "transfer_to_hyperliquid", venue: "Base", status: "executed" },
    { id: "evt-4", time: "12:01:21", kind: "action_executed", node: "open_eth_long_5x", venue: "Hyperliquid", status: "executed" },
    { id: "evt-5", time: "12:15:02", kind: "rejected_action", node: "close_position", venue: "Hyperliquid", status: "rejected" },
    { id: "evt-6", time: "13:05:44", kind: "rejected_action", node: "increase_position", venue: "Hyperliquid", status: "rejected" },
    { id: "evt-7", time: "13:45:22", kind: "action_executed", node: "close_position", venue: "Hyperliquid", status: "executed" },
  ],
  selected: {
    kind: "action_executed",
    node: "buy-eth-on-base",
    venue: "Base",
    explanation: "Bought ETH on Base after the threshold signal fired in the historical price window.",
    instrument: "ETH / USDC",
    side: "Buy",
    leverage: "None",
    orderType: "Market",
    before: [
      { asset: "USDC", amount: "32,134.76", value: "$32,134.76", percent: 82 },
      { asset: "ETH", amount: "0.0000", value: "$0.00", percent: 1 },
    ],
    after: [
      { asset: "USDC", amount: "22,134.76", value: "$22,134.76", percent: 56 },
      { asset: "ETH", amount: "3.3442", value: "$10,000.00", percent: 32 },
    ],
    pricing: [
      ["Mark price", "$2,985.42"],
      ["Assumed price", "$2,985.42"],
      ["Slippage", "fixed_bps 10"],
      ["Fill price", "$2,988.40"],
      ["Total cost", "$6.82"],
    ],
    raw: {
      event_type: "action_executed",
      node_id: "buy-eth-on-base",
      venue: "base",
      timestamp: "2024-05-14T14:30:00Z",
      status: "executed",
    },
  },
  policyMatrix: [
    ["Fills", "slippage fixed_bps 10", "fixed_bps 25", "fixed_bps 5"],
    ["Partial fills", "Disallow", "Allow", "Allow"],
    ["Gas", "historical_fee_history", "p95_fee_history", "median_fee_history"],
    ["Signals", "crossing", "crossing_with_cooldown", "level"],
    ["Data", "missing_required fail", "missing_optional warn", "allow small gaps"],
    ["Perps", "funding historical", "funding historical", "funding mark_est"],
    ["Yield", "simple_apr", "simple_apr", "compound_apy"],
  ],
  rejected: [
    { time: "12:15:02", node: "n23", action: "close_position", reason: "policy reason: insufficient balance" },
    { time: "13:05:44", node: "n25", action: "increase_position", reason: "policy reason: missing_required fail" },
  ],
};

export const runHistory = [
  { id: "b2d8a7f1", status: "success", policy: "Strict v1", range: "2024-01-01 - 2024-06-01", interval: "1h", duration: "18m 42s", returnUsd: "+$9,371.92" },
  { id: "8c1e9d44", status: "warning", policy: "Conservative v1", range: "2024-01-01 - 2024-05-10", interval: "1h", duration: "16m 11s", returnUsd: "+$6,124.11" },
  { id: "3a7f0b22", status: "danger", policy: "Strict v1", range: "2024-01-01 - 2024-05-08", interval: "1h", duration: "3m 07s", returnUsd: "-" },
];
