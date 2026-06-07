import type { UTCTimestamp } from "lightweight-charts";
import type {
  BacktestEvent,
  BacktestListItem,
  BacktestMetadata,
  BacktestResult,
  CatalystGraph,
  CoverageResponse,
  GraphPreview,
  MarketDataBundle,
} from "./client";
import type {
  AuditData,
  EventStatus,
  GraphSummary,
  MarketEvent,
  MarketReplayData,
  ResultData,
  SetupData,
  Tone,
} from "../types";

function stringField(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function compactDecimalField(value: unknown, maximumFractionDigits = 6): string | undefined {
  const numeric = numberValue(value, Number.NaN);
  if (Number.isFinite(numeric)) return compactNumber(numeric, maximumFractionDigits);
  return stringField(value);
}

function numberValue(value: unknown, fallback = 0): number {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string") {
    const parsed = Number(value.replace(/,/g, ""));
    if (Number.isFinite(parsed)) return parsed;
  }
  return fallback;
}

function compactNumber(value: number, maximumFractionDigits = 4): string {
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: 0,
    maximumFractionDigits,
  }).format(value);
}

function money(value: number): string {
  const sign = value < 0 ? "-" : "";
  return `${sign}$${compactNumber(Math.abs(value), 2)}`;
}

function signedMoney(value: number): string {
  return value >= 0 ? `+${money(value)}` : money(value);
}

function percent(value: number): string {
  const sign = value > 0 ? "+" : "";
  return `${sign}${compactNumber(value, 4)}%`;
}

function isoLabel(iso?: string): string {
  if (!iso) return "-";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  const hour = String(date.getUTCHours()).padStart(2, "0");
  const minute = String(date.getUTCMinutes()).padStart(2, "0");
  return `${year}-${month}-${day} ${hour}:${minute} UTC`;
}

function shortDate(iso?: string): string {
  if (!iso) return "-";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toISOString().slice(0, 10);
}

function unixTime(iso: string, fallbackIndex = 0): UTCTimestamp {
  const seconds = Math.floor(new Date(iso).getTime() / 1000);
  return (Number.isFinite(seconds) ? seconds : fallbackIndex) as UTCTimestamp;
}

function titleCase(value: string): string {
  return value
    .replaceAll("_", " ")
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

function eventStatus(type: string): EventStatus {
  if (type === "action_executed") return "executed";
  if (type === "action_rejected" || type === "rejected_action") return "rejected";
  if (type.includes("funding") || type.includes("yield") || type.includes("policy")) return "policy";
  if (type.includes("gas") || type.includes("warning")) return "warning";
  return "signal";
}

function toneFor(value: number): Tone {
  if (value > 0) return "positive";
  if (value < 0) return "negative";
  return "neutral";
}

function firstCandleSeries(marketData: MarketDataBundle) {
  return marketData.candles?.[0];
}

function priceAt(marketData: MarketDataBundle, ts: string): number {
  const points = firstCandleSeries(marketData)?.points ?? [];
  if (points.length === 0) return 0;
  const exact = points.find((point) => point.ts === ts);
  return numberValue((exact ?? points[points.length - 1]).close);
}

export function graphFromPreview(
  graph: CatalystGraph,
  preview?: GraphPreview,
  meta: { id?: string; name?: string; version?: string } = {},
): GraphSummary {
  const summary = preview?.graph_summary;
  const actionIds = summary?.actions ?? graph.nodes.filter((node) => node.kind === "action").map((node) => node.id);
  const signalIds = summary?.signals ?? graph.nodes.filter((node) => node.kind === "signal").map((node) => node.id);

  return {
    id: meta.id ?? "g_inline_service_demo",
    hash: preview?.graph_hash?.slice(0, 8) ?? "service",
    name: meta.name ?? "ETH service backtest",
    version: meta.version ?? "service",
    updatedAt: new Date().toISOString().replace("T", " ").slice(0, 16),
    status: preview?.valid === false ? "danger" : "validated",
    nodeCount: numberValue(summary?.node_count, graph.nodes.length),
    edgeCount: numberValue(summary?.edge_count, graph.edges.length),
    nodes: graph.nodes.map((node) => ({
      id: node.id,
      kind: node.kind,
      label: node.id,
      detail: node.subtype ?? (actionIds.includes(node.id) ? "action" : signalIds.includes(node.id) ? "signal" : "node"),
    })),
    edges: graph.edges
      .map((edge, index) => {
        const from = stringField(edge.from);
        const to = stringField(edge.to);
        if (!from || !to) return undefined;
        return { id: `${from}--${to}-${index}`, from, to };
      })
      .filter((edge): edge is { id: string; from: string; to: string } => Boolean(edge)),
  };
}

export function setupFromService(input: {
  runId?: string;
  graph: CatalystGraph;
  config: { start: string; end: string; interval: string; initial_portfolio: Record<string, Record<string, string>> };
  policyProfile: string;
  dataSourceLabel?: string;
  coverage?: CoverageResponse;
  preview?: GraphPreview;
  metadata?: BacktestMetadata;
  profiles?: Array<{ id: string; label?: string }>;
}): SetupData {
  const coverageRows = input.coverage?.coverage ?? [];
  const warnings = [
    ...(input.preview?.valid === false && input.preview.error ? [input.preview.error] : []),
    ...(input.preview?.warnings ?? []),
    ...(input.coverage?.warnings ?? []),
    ...(input.metadata?.warnings ?? []),
  ];

  return {
    runId: input.runId ?? input.metadata?.id ?? "service_demo",
    start: input.config.start,
    end: input.config.end,
    interval: input.config.interval,
    policy: input.policyProfile,
    portfolio: Object.entries(input.config.initial_portfolio).flatMap(([venue, assets]) =>
      Object.entries(assets).map(([asset, amount]) => ({
        venue,
        asset,
        amount: compactNumber(numberValue(amount), 4),
        percent: "-",
      })),
    ),
    coverage: coverageRows.map((row) => {
      const points = numberValue(row.points);
      const coverage = numberValue(row.completeness_pct ?? row.covered_pct ?? row.coverage, points > 0 ? 100 : 0);
      const complete = row.complete !== false && points > 0;
      const explicitStatus = typeof row.status === "string" ? row.status.toLowerCase() : "";
      const missingSeries = points <= 0 || explicitStatus === "danger" || explicitStatus === "missing";
      return {
        kind: titleCase(row.kind),
        source: String(row.source ?? row.venue ?? row.chain ?? row.symbol ?? "inline data"),
        interval: String(row.interval ?? input.config.interval),
        coverage,
        status: missingSeries ? "danger" : !complete || coverage < 70 ? "warning" : "success",
      };
    }),
    assumptions: [
      ["API", "Rust simulation service"],
      ["Policy profile", input.profiles?.find((profile) => profile.id === input.policyProfile)?.label ?? input.policyProfile],
      ["Fill price", String(input.preview?.resolved_policy?.price_selection ?? "service policy")],
      ["Data source", input.dataSourceLabel ?? "Parquet store"],
      ["Queue mode", "POST /backtests + status polling"],
      ["Graph hash", input.preview?.graph_hash?.slice(0, 12) ?? "-"],
    ],
    warnings: warnings.length ? warnings : ["No service warnings for this run."],
  };
}

export function marketReplayWithMarketData(
  current: MarketReplayData,
  marketData: MarketDataBundle,
): MarketReplayData {
  const candleSeries = firstCandleSeries(marketData);
  if (!candleSeries?.points.length) return current;
  const candles = candleSeries.points.map((point, index) => ({
    time: unixTime(point.ts, index),
    open: numberValue(point.open),
    high: numberValue(point.high),
    low: numberValue(point.low),
    close: numberValue(point.close),
    volume: numberValue(point.volume),
  }));
  const replay = candles.map((_, index) => {
    const existing = current.replay[index] ?? current.replay.at(-1) ?? {
      equity: 0,
      drawdown: 0,
      gas: 0,
      funding: 0,
    };
    return {
      ...existing,
      label: `T${String(index + 1).padStart(2, "0")}`,
    };
  });

  return {
    ...current,
    symbol: `${candleSeries.symbol} / ${candleSeries.quote}`,
    venue: candleSeries.venue,
    period: `${shortDate(marketData.start)} - ${shortDate(marketData.end)}`,
    candles,
    replay,
    evidence: [
      ["Data source", marketData.providers?.[0]?.name ? String(marketData.providers[0].name) : "market-data window"],
      ["Candle series", `${candleSeries.venue} ${candleSeries.symbol}`],
      ["Interval", marketData.interval],
      ["Start", marketData.start],
      ["End", marketData.end],
    ],
  };
}

export function runHistoryFromApi(items: BacktestListItem[]): Array<Record<string, string>> {
  return items.map((item) => ({
    id: item.id,
    status: item.status === "succeeded" ? "success" : item.status === "failed" ? "danger" : item.status,
    policy: item.policy_profile ?? "-",
    range: `${shortDate(item.start)} - ${shortDate(item.end)}`,
    interval: item.interval ?? "-",
    duration: item.finished_at ?? item.created_at ?? "-",
    returnUsd: item.summary?.return_pct !== undefined ? percent(numberValue(item.summary.return_pct)) : "-",
  }));
}

export function resultFromApi(result: BacktestResult, status?: string): ResultData {
  const summary = result.summary;
  const equityCurve = result.equity_curve ?? [];
  const drawdownCurve = result.drawdown_curve ?? [];
  const equity = equityCurve.map((point) => numberValue(point.equity_usd));
  const drawdown = drawdownCurve.map((point) => numberValue(point.drawdown_pct));
  const trend = equityCurve.map((point, index) => ({
    time: unixTime(point.ts, index),
    label: isoLabel(point.ts),
    equity: numberValue(point.equity_usd),
    drawdown: numberValue(drawdownCurve[index]?.drawdown_pct, numberValue(drawdownCurve.at(-1)?.drawdown_pct)),
  }));
  const finalValue = numberValue(summary.final_value_usd);
  const startValue = numberValue(summary.starting_value_usd);
  const pnl = numberValue(summary.pnl_usd);
  const fees = Math.abs(numberValue(result.costs?.total_fees_usd));
  const gas = Math.abs(numberValue(result.costs?.total_gas_usd));
  const fundingRaw = numberValue(result.costs?.total_funding_usd);
  const yieldRaw = numberValue(result.costs?.total_yield_usd);
  const fundingCost = Math.max(fundingRaw, 0);
  const gross = pnl + fees + gas + fundingCost - Math.max(yieldRaw, 0);

  const costs: ResultData["costs"] = [
    { label: "Gross PnL", value: signedMoney(gross), tone: toneFor(gross), amount: gross },
    { label: "Fees", value: signedMoney(-fees), tone: "negative", amount: -fees },
    { label: "Gas", value: signedMoney(-gas), tone: "negative", amount: -gas },
    { label: "Funding", value: signedMoney(-fundingCost), tone: fundingCost > 0 ? "negative" : "neutral", amount: -fundingCost },
  ];
  if (yieldRaw) costs.push({ label: "Yield", value: signedMoney(yieldRaw), tone: toneFor(yieldRaw), amount: yieldRaw });
  costs.push({ label: "Net PnL", value: signedMoney(pnl), tone: toneFor(pnl), amount: pnl });

  return {
    status: status ?? "succeeded",
    createdAt: result.metadata?.end ?? new Date().toISOString(),
    metrics: [
      { label: "Final value", value: money(finalValue), detail: `Start ${money(startValue)}` },
      { label: "Return", value: percent(numberValue(summary.return_pct)), detail: signedMoney(pnl), tone: toneFor(pnl) },
      { label: "Portfolio PnL", value: signedMoney(pnl), detail: "Includes starting positions", tone: toneFor(pnl) },
      { label: "Max DD", value: percent(numberValue(summary.max_drawdown_pct)), detail: "Largest drawdown", tone: "negative" },
      { label: "Trades", value: String(summary.trade_count ?? result.trades?.length ?? 0), detail: "service result" },
      { label: "Rejected", value: String(summary.rejected_count ?? 0), detail: "policy blocked" },
    ],
    equity,
    drawdown,
    trend,
    portfolio: portfolioFromResult(result, finalValue),
    timeline: (result.trades ?? []).slice(-8).reverse().map((trade) => ({
      time: isoLabel(trade.ts),
      node: trade.node_id ?? "-",
      signal: trade.kind ?? "-",
      action: trade.status ?? "-",
      venue: trade.venue ?? "-",
      fees: trade.fee_usd ? money(numberValue(trade.fee_usd)) : "-",
      gas: trade.gas_usd ? money(numberValue(trade.gas_usd)) : "-",
      pnl: trade.value_usd ? money(numberValue(trade.value_usd)) : "-",
    })),
    costs,
  };
}

function portfolioFromResult(result: BacktestResult, finalValue: number): ResultData["portfolio"] {
  const balances = result.final_portfolio?.balances ?? {};
  const rows = Object.entries(balances).map(([venue, assets]) => {
    const entries = Object.entries(assets);
    const stableValue = entries
      .filter(([asset]) => asset.toUpperCase().includes("USD"))
      .reduce((sum, [, balance]) => sum + numberValue(balance), 0);
    const nonStableEntries = entries.filter(([asset]) => !asset.toUpperCase().includes("USD"));
    const nonStableBalanceTotal = nonStableEntries.reduce((sum, [, balance]) => sum + numberValue(balance), 0);
    const remainingValue = Math.max(finalValue - stableValue, 0);
    const assetRows = entries.map(([asset, balance]) => {
      const amount = numberValue(balance);
      const isStable = asset.toUpperCase().includes("USD");
      const value = isStable
        ? amount
        : nonStableBalanceTotal > 0
          ? remainingValue * (amount / nonStableBalanceTotal)
          : 0;
      const price = isStable ? 1 : amount > 0 ? value / amount : 0;
      return {
        asset,
        balance: compactNumber(amount, 4),
        price: money(price),
        value: money(value),
        percent: finalValue > 0 ? `${compactNumber((value / finalValue) * 100, 2)}%` : "-",
      };
    });
    const total = assetRows.reduce((sum, row) => sum + numberValue(row.value.replace("$", "")), 0);
    return { venue, total: money(total), assets: assetRows };
  });

  return rows.length ? rows : [{ venue: "service", total: money(finalValue), assets: [] }];
}

export function marketReplayFromApi(
  result: BacktestResult,
  events: BacktestEvent[],
  marketData: MarketDataBundle,
): MarketReplayData {
  const candleSeries = firstCandleSeries(marketData);
  const candles = (candleSeries?.points ?? []).map((point, index) => ({
    time: unixTime(point.ts, index),
    open: numberValue(point.open),
    high: numberValue(point.high),
    low: numberValue(point.low),
    close: numberValue(point.close),
    volume: numberValue(point.volume),
  }));
  const gasPoints = marketData.gas?.[0]?.points ?? [];
  const fundingPoints = marketData.funding?.[0]?.points ?? [];
  const equity = result.equity_curve ?? [];
  const drawdown = result.drawdown_curve ?? [];
  const sampleCount = Math.max(candles.length, equity.length, drawdown.length);
  const replay = Array.from({ length: sampleCount }, (_, index) => ({
    label: `T${String(index + 1).padStart(2, "0")}`,
    time: candles[index]?.time ?? (equity[index]?.ts ? unixTime(equity[index].ts, index) : undefined),
    equity: numberValue(equity[index]?.equity_usd, numberValue(equity.at(-1)?.equity_usd)),
    drawdown: numberValue(drawdown[index]?.drawdown_pct, numberValue(drawdown.at(-1)?.drawdown_pct)),
    gas: numberValue(gasPoints[index]?.gas_usd),
    funding: numberValue(fundingPoints[index]?.rate) * 1000,
  }));
  const mappedEvents = eventsFromApi(events, marketData);

  return {
    symbol: `${candleSeries?.symbol ?? "ETH"} / ${candleSeries?.quote ?? "USD"}`,
    venue: candleSeries?.venue ?? "service",
    period: `${shortDate(marketData.start)} - ${shortDate(marketData.end)}`,
    selectedEventId: mappedEvents[0]?.id ?? "event-0",
    candles,
    replay,
    events: mappedEvents,
    evidence: [
      ["Candle source", candleSeries ? `${candleSeries.venue} ${candleSeries.symbol}` : "-"],
      ["Gas points", String(gasPoints.length)],
      ["Funding points", String(fundingPoints.length)],
      ["Equity samples", String(equity.length)],
      ["Events", String(events.length)],
    ],
  };
}

function eventsFromApi(events: BacktestEvent[], marketData: MarketDataBundle): MarketEvent[] {
  return events.map((event, index) => {
    const detail = event.detail ?? {};
    const type = event.type;
    const status = eventStatus(type);
    const price = numberValue(detail.price, priceAt(marketData, event.ts));
    const valueUsd = numberValue(detail.value_usd ?? detail.fee_usd ?? detail.gas_usd);
    const amount = compactDecimalField(detail.amount);
    const symbol = stringField(detail.symbol) ?? firstCandleSeries(marketData)?.symbol;
    const fee = numberValue(detail.fee_usd, Number.NaN);
    const gas = numberValue(detail.gas_usd, Number.NaN);

    return {
      id: `event-${index + 1}`,
      index: index + 1,
      time: unixTime(event.ts, index),
      labelTime: isoLabel(event.ts),
      kind: type,
      label: titleCase(type),
      node: event.node_id ?? "-",
      status,
      price: price ? money(price) : "-",
      impact: event.reason ?? (valueUsd ? money(valueUsd) : "-"),
      side: stringField(detail.side),
      orderType: stringField(detail.kind),
      fillAmount: amount ? `${amount}${symbol ? ` ${symbol}` : ""}` : undefined,
      notional: Number.isFinite(valueUsd) && valueUsd !== 0 ? money(valueUsd) : undefined,
      fee: Number.isFinite(fee) ? money(fee) : undefined,
      gas: Number.isFinite(gas) ? money(gas) : undefined,
    };
  });
}

export function auditFromApi(
  events: BacktestEvent[],
  result: BacktestResult,
  replay: MarketReplayData,
): AuditData {
  const selected = replay.events[0];
  const selectedApi = events[0];
  const rejected = events.filter((event) => event.type === "action_rejected" || event.reason);

  return {
    selectedEventId: selected?.id ?? replay.selectedEventId,
    events: replay.events.map((event) => ({
      id: event.id,
      time: event.labelTime,
      kind: event.kind,
      node: event.node,
      venue: replay.venue,
      status: event.status,
    })),
    selected: {
      kind: selected?.kind ?? "-",
      node: selected?.node ?? "-",
      venue: firstCandleSeriesFromReplay(replay),
      explanation: selectedApi?.reason ?? "Service trace event from the completed Rust backtest run.",
      instrument: replay.symbol,
      side: String(selectedApi?.detail?.side ?? "-"),
      leverage: "-",
      orderType: "Service model",
      before: [],
      after: [],
      pricing: [
        ["Event price", selected?.price ?? "-"],
        ["Impact", selected?.impact ?? "-"],
        ["Net PnL", signedMoney(numberValue(result.summary.pnl_usd))],
        ["Fees", money(numberValue(result.costs?.total_fees_usd))],
      ],
      raw: Object.fromEntries(
        Object.entries({
          event_type: selectedApi?.type,
          node_id: selectedApi?.node_id,
          timestamp: selectedApi?.ts,
          reason: selectedApi?.reason,
        }).filter(([, value]) => value !== undefined),
      ) as Record<string, string>,
    },
    policyMatrix: [
      ["Fills", String(result.metadata?.policy?.price_selection ?? "service"), "conservative", "research"],
      ["Gas", "historical", "fallback", "fixed"],
      ["Funding", "historical", "historical", "none"],
      ["Data", "required", "fallback", "forward fill"],
    ],
    rejected: rejected.map((event) => ({
      time: isoLabel(event.ts),
      node: event.node_id ?? "-",
      action: event.type,
      reason: event.reason ?? "rejected by policy",
    })),
  };
}

function firstCandleSeriesFromReplay(replay: MarketReplayData): string {
  return replay.venue || "-";
}
