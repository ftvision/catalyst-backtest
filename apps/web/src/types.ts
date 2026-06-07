import type { UTCTimestamp } from "lightweight-charts";

export type Tone = "positive" | "negative" | "neutral";
export type EventStatus = "signal" | "executed" | "rejected" | "policy" | "warning";

export interface GraphNode {
  id: string;
  kind: string;
  label: string;
  detail: string;
}

export interface GraphEdge {
  id: string;
  from: string;
  to: string;
}

export interface GraphSummary {
  id: string;
  hash: string;
  name: string;
  version: string;
  updatedAt: string;
  status: string;
  nodeCount: number;
  edgeCount: number;
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface SetupData {
  runId: string;
  start: string;
  end: string;
  interval: string;
  policy: string;
  portfolio: Array<{ venue: string; asset: string; amount: string; percent: string }>;
  coverage: Array<{
    kind: string;
    source: string;
    interval: string;
    coverage: number;
    status: "success" | "warning" | "danger";
  }>;
  assumptions: Array<[string, string]>;
  warnings: string[];
}

export interface MetricItem {
  label: string;
  value: string;
  detail: string;
  tone?: Tone;
}

export interface ResultData {
  status: string;
  createdAt: string;
  metrics: MetricItem[];
  equity: number[];
  drawdown: number[];
  trend?: Array<{
    time: UTCTimestamp;
    label: string;
    equity: number;
    drawdown: number;
  }>;
  portfolio: Array<{
    venue: string;
    total: string;
    assets: Array<{
      asset: string;
      balance: string;
      price: string;
      value: string;
      percent: string;
    }>;
  }>;
  timeline: Array<{
    time: string;
    node: string;
    signal: string;
    action: string;
    venue: string;
    fees: string;
    gas: string;
    pnl: string;
  }>;
  costs: Array<{ label: string; value: string; tone: Tone; amount: number }>;
}

export interface MarketEvent {
  id: string;
  index: number;
  time: UTCTimestamp;
  labelTime: string;
  kind: string;
  label: string;
  node: string;
  status: EventStatus;
  price: string;
  impact: string;
}

export interface CandlePoint {
  time: UTCTimestamp;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

export interface ReplayPoint {
  label: string;
  equity: number;
  drawdown: number;
  gas: number;
  funding: number;
}

export interface MarketReplayData {
  symbol: string;
  venue: string;
  period: string;
  selectedEventId: string;
  candles: CandlePoint[];
  replay: ReplayPoint[];
  events: MarketEvent[];
  evidence: Array<[string, string]>;
}

export interface AuditData {
  selectedEventId: string;
  events: Array<{
    id: string;
    time: string;
    kind: string;
    node: string;
    venue: string;
    status: EventStatus;
  }>;
  selected: {
    kind: string;
    node: string;
    venue: string;
    explanation: string;
    instrument: string;
    side: string;
    leverage: string;
    orderType: string;
    before: Array<{ asset: string; amount: string; value: string; percent: number }>;
    after: Array<{ asset: string; amount: string; value: string; percent: number }>;
    pricing: Array<[string, string]>;
    raw: Record<string, string>;
  };
  policyMatrix: Array<[string, string, string, string]>;
  rejected: Array<{ time: string; node: string; action: string; reason: string }>;
}
