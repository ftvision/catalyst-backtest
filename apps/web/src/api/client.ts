const API_BASE = import.meta.env.VITE_CATALYST_API_BASE ?? "http://127.0.0.1:8080";

export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export interface CatalystGraphNode {
  id: string;
  kind: string;
  subtype?: string;
  config?: Record<string, JsonValue>;
}

export interface CatalystGraph {
  schema_version?: string;
  /** Named scalar values substitutable into node configs ("$name" / {var}). */
  variables?: Record<string, string | number | boolean>;
  settings?: Record<string, JsonValue>;
  nodes: CatalystGraphNode[];
  edges: Array<Record<string, JsonValue>>;
}

export interface BacktestConfig {
  start: string;
  end: string;
  interval: string;
  initial_portfolio: Record<string, Record<string, string>>;
}

export interface CandlePointApi {
  ts: string;
  open: string;
  high: string;
  low: string;
  close: string;
  volume?: string;
}

export interface MarketDataBundle {
  schema_version: string;
  interval: string;
  start: string;
  end: string;
  candles?: Array<{
    venue: string;
    symbol: string;
    quote: string;
    points: CandlePointApi[];
  }>;
  gas?: Array<{
    chain: string;
    points: Array<{ ts: string; gas_usd: string }>;
  }>;
  funding?: Array<{
    venue: string;
    symbol: string;
    points: Array<{ ts: string; rate: string }>;
  }>;
  yields?: Array<Record<string, JsonValue>>;
  providers?: Array<Record<string, JsonValue>>;
  warnings?: string[];
}

export interface MarketDataCatalogItem {
  kind: string;
  source: string;
  venue?: string;
  symbol?: string;
  quote?: string;
  protocol?: string;
  asset?: string;
  chain?: string;
  pool?: string;
  interval?: string;
  start?: string | null;
  end?: string | null;
  missing_date_ranges?: Array<{ start: string; end: string }>;
  files?: number;
  points?: number;
}

export interface MarketDataCatalogResponse {
  source: string;
  root?: string;
  items: MarketDataCatalogItem[];
  warnings?: string[];
}

export interface BacktestRequest {
  graph: CatalystGraph;
  config: BacktestConfig;
  policy: { profile: string };
  market_data?: MarketDataBundle;
}

export interface StrategyListItem {
  id: string;
  title: string;
  source: string;
  graph_path: string;
}

export interface StrategyDetail {
  id: string;
  title: string;
  source: string;
  graph: CatalystGraph;
}

export interface StrategyScenarioListItem {
  id: string;
  title: string;
  scenario_path: string;
}

export interface StrategyScenarioDetail {
  id: string;
  title: string;
  scenario: {
    id?: string;
    config: BacktestConfig;
    policy?: { profile?: string };
    market_data: MarketDataBundle;
  };
}

export interface BacktestStatus {
  id: string;
  status: "queued" | "running" | "succeeded" | "failed" | string;
  error?: string | null;
  created_at?: string;
  started_at?: string | null;
  finished_at?: string | null;
}

export interface BacktestListItem {
  id: string;
  graph_hash?: string;
  status: string;
  policy_profile?: string;
  start?: string;
  end?: string;
  interval?: string;
  created_at?: string;
  started_at?: string | null;
  finished_at?: string | null;
  summary?: Partial<BacktestSummary>;
  warning_count?: number;
}

export interface GraphPreview {
  graph_hash: string;
  valid: boolean;
  error?: string;
  graph_summary?: {
    node_count?: number;
    edge_count?: number;
    nodes?: string[];
    actions?: string[];
    signals?: string[];
    [key: string]: JsonValue | string[] | undefined;
  };
  data_requirements?: {
    candles?: Array<{ venue?: string; symbol?: string; interval?: string }>;
    gas?: Array<{ chain?: string; interval?: string }>;
    funding?: Array<{ venue?: string; symbol?: string }>;
    yields?: Array<Record<string, JsonValue>>;
  };
  resolved_policy?: Record<string, JsonValue>;
  resolved_variables?: Record<string, JsonValue>;
  warnings?: string[];
}

export interface CoverageResponse {
  coverage?: Array<{
    kind: string;
    source?: string;
    venue?: string;
    chain?: string;
    symbol?: string;
    interval?: string;
    points?: string | number;
    complete?: boolean;
    covered_pct?: string | number;
    coverage?: string | number;
    status?: string;
    [key: string]: JsonValue | undefined;
  }>;
  warnings?: string[];
}

export interface BacktestSummary {
  starting_value_usd: string;
  final_value_usd: string;
  pnl_usd: string;
  return_pct: string;
  max_drawdown_pct: string;
  trade_count: number;
  rejected_count: number;
}

export interface BacktestResult {
  summary: BacktestSummary;
  equity_curve?: Array<{ ts: string; equity_usd: string }>;
  drawdown_curve?: Array<{ ts: string; drawdown_pct: string }>;
  trades?: Array<{
    ts: string;
    node_id?: string;
    kind?: string;
    venue?: string;
    symbol?: string;
    side?: string;
    price?: string;
    amount?: string;
    value_usd?: string;
    fee_usd?: string;
    gas_usd?: string;
    status?: string;
    reason?: string;
  }>;
  final_portfolio?: {
    balances?: Record<string, Record<string, string>>;
    perp_positions?: Record<string, Record<string, JsonValue>>;
    yield_positions?: Record<string, Record<string, JsonValue>>;
  };
  costs?: {
    total_fees_usd?: string;
    total_gas_usd?: string;
    total_funding_usd?: string;
    total_yield_usd?: string;
  };
  metadata?: {
    policy?: Record<string, JsonValue>;
    interval?: string;
    start?: string;
    end?: string;
    data_coverage?: JsonValue;
    warnings?: string[];
  };
}

export interface BacktestMetadata {
  id: string;
  graph_hash?: string;
  status: string;
  created_at?: string;
  started_at?: string | null;
  finished_at?: string | null;
  config?: { start?: string; end?: string; interval?: string };
  resolved_policy?: Record<string, JsonValue>;
  data_coverage?: JsonValue;
  warnings?: string[];
  summary?: Partial<BacktestSummary>;
}

export interface BacktestEvent {
  ts: string;
  type: string;
  node_id?: string;
  reason?: string;
  detail?: Record<string, JsonValue>;
}

export class ApiError extends Error {
  status: number;
  code?: string;
  details?: unknown;

  constructor(message: string, status: number, code?: string, details?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.details = details;
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
    ...init,
  });
  const body = await response.json().catch(() => undefined);

  if (!response.ok) {
    const message = body?.error?.message ?? `Catalyst API request failed: ${response.status}`;
    throw new ApiError(message, response.status, body?.error?.code, body);
  }

  return body as T;
}

export const catalystApi = {
  baseUrl: API_BASE,
  health: () => request<{ status: string; service: string }>("/health"),
  listPolicyProfiles: () =>
    request<{ items: Array<{ id: string; label?: string; resolved_policy?: Record<string, JsonValue> }> }>(
      "/policy-profiles",
    ),
  listStrategies: () => request<{ items: StrategyListItem[] }>("/strategies"),
  getStrategy: (id: string) => request<StrategyDetail>(`/strategies/${encodeURIComponent(id)}`),
  listStrategyScenarios: () => request<{ items: StrategyScenarioListItem[] }>("/strategy-scenarios"),
  getStrategyScenario: (id: string) =>
    request<StrategyScenarioDetail>(`/strategy-scenarios/${encodeURIComponent(id)}`),
  listMarketDataCatalog: () => request<MarketDataCatalogResponse>("/market-data/catalog"),
  listBacktests: (graphHash?: string) =>
    request<{ items: BacktestListItem[] }>(
      `/backtests${graphHash ? `?graph_hash=${encodeURIComponent(graphHash)}` : ""}`,
    ),
  previewGraph: (graph: CatalystGraph, policy?: { profile: string }) =>
    request<GraphPreview>("/backtests/preview", {
      method: "POST",
      body: JSON.stringify({ graph, policy }),
    }),
  checkCoverage: (body: {
    graph: CatalystGraph;
    start: string;
    end: string;
    interval: string;
    market_data?: MarketDataBundle;
  }) =>
    request<CoverageResponse>("/market-data/coverage", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  loadMarketDataWindow: (body: {
    graph: CatalystGraph;
    start: string;
    end: string;
    interval: string;
    market_data?: MarketDataBundle;
  }) =>
    request<MarketDataBundle>("/market-data/window", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  createBacktest: (body: BacktestRequest) =>
    request<{ id: string; status: string }>("/backtests", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  getBacktest: (id: string) => request<BacktestStatus>(`/backtests/${encodeURIComponent(id)}`),
  getResult: (id: string) =>
    request<BacktestResult>(`/backtests/${encodeURIComponent(id)}/result`),
  getMetadata: (id: string) =>
    request<BacktestMetadata>(`/backtests/${encodeURIComponent(id)}/metadata`),
  getEvents: (id: string, params?: { status?: string; cursor?: number; limit?: number }) => {
    const query = new URLSearchParams();
    if (params?.status) query.set("status", params.status);
    if (params?.cursor !== undefined) query.set("cursor", String(params.cursor));
    if (params?.limit !== undefined) query.set("limit", String(params.limit));
    const suffix = query.toString() ? `?${query.toString()}` : "";
    return request<{ items: BacktestEvent[]; next_cursor?: number | null; total: number }>(
      `/backtests/${encodeURIComponent(id)}/events${suffix}`,
    );
  },
};
