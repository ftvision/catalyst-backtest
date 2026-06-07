import { useEffect, useMemo, useRef, useState } from "react";
import {
  ActionIcon,
  Badge,
  Divider,
  Group,
  NavLink,
  SegmentedControl,
  Stack,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import { useClipboard } from "@mantine/hooks";
import {
  Clipboard,
  Database,
  Download,
  FileChartColumn,
  Gauge,
  History,
} from "lucide-react";
import {
  auditFromApi,
  graphFromPreview,
  marketReplayFromApi,
  marketReplayWithMarketData,
  resultFromApi,
  runHistoryFromApi,
  setupFromService,
} from "./api/adapters";
import {
  ApiError,
  catalystApi,
  type BacktestConfig,
  type BacktestListItem,
  type BacktestStatus,
  type CatalystGraph,
  type CoverageResponse,
  type GraphPreview,
  type MarketDataCatalogItem,
  type MarketDataBundle,
  type StrategyListItem,
  type StrategyScenarioListItem,
} from "./api/client";
import { demoConfig, demoGraph, demoMarketData } from "./data/demoRequest";
import { audit, graph, marketReplay, result, runHistory, setup } from "./data/mockData";
import { EventLensPage } from "./pages/EventLensPage";
import { MarketDataPage } from "./pages/MarketDataPage";
import { MarketReplayPage } from "./pages/MarketReplayPage";
import { ResultReviewPage } from "./pages/ResultReviewPage";
import { RunSetupPage } from "./pages/RunSetupPage";
import { SimulationHistoryPage } from "./pages/SimulationHistoryPage";
import { marketCatalogId } from "./components/MarketDataSelector";
import {
  deleteCachedRunDetail,
  getLastRunId,
  loadAllCachedRunDetails,
  loadCachedRunDetail,
  loadDeletedRunIds,
  makeCachedRunDetail,
  markRunDeleted,
  saveCachedRunDetail,
  setLastRunId,
  type CachedRunDetail,
} from "./storage/runCache";
import type { AuditData, GraphSummary, MarketReplayData, ResultData, SetupData } from "./types";

type RouteId = "setup" | "data" | "history" | "details";
type DetailTabId = "result" | "replay" | "lens";

const routes: Array<{ id: RouteId; label: string; icon: React.ReactNode }> = [
  { id: "setup", label: "Run Setup", icon: <Gauge size={14} /> },
  { id: "data", label: "Market Data", icon: <Database size={14} /> },
  { id: "history", label: "History", icon: <History size={14} /> },
  { id: "details", label: "Run Details", icon: <FileChartColumn size={14} /> },
];

const detailTabs: Array<{ value: DetailTabId; label: string }> = [
  { value: "result", label: "Result Review" },
  { value: "replay", label: "Market Replay" },
  { value: "lens", label: "Event Lens" },
];

type ApiStatus = "checking" | "healthy" | "offline" | "running" | "failed";
type RunStatus = "idle" | "submitting" | "queued" | "running" | "succeeded" | "failed";
type DataSourceMode = "store" | "inline";

const DEFAULT_BACKTEST_START = "2026-03-01T00:00:00Z";
const DEFAULT_BACKTEST_END = "2026-06-01T00:00:00Z";
const ACTIVE_ROUTE_STORAGE_KEY = "catalyst:active-route";

interface WorkbenchState {
  graph: GraphSummary;
  setup: SetupData;
  marketReplay: MarketReplayData;
  result: ResultData;
  audit: AuditData;
  runHistory: Array<Record<string, string>>;
  historyItems: BacktestListItem[];
}

interface ActiveSelection {
  strategyId: string;
  strategyTitle: string;
  scenarioId: string;
  scenarioTitle: string;
}

function initialRouteFromStorage(): RouteId {
  if (typeof localStorage === "undefined") return "setup";
  const stored = localStorage.getItem(ACTIVE_ROUTE_STORAGE_KEY);
  return routes.some((route) => route.id === stored) ? (stored as RouteId) : "setup";
}

function storeActiveRoute(route: RouteId) {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(ACTIVE_ROUTE_STORAGE_KEY, route);
}

interface PolicyProfileOption {
  id: string;
  label?: string;
}

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.code ? `${error.code}: ${error.message}` : error.message;
  if (error instanceof Error) return error.message;
  return "Unknown service error";
}

function emptyMarketData(config: BacktestConfig, warnings: string[] = []): MarketDataBundle {
  return {
    schema_version: "catalyst.market_data.bundle.v1",
    interval: config.interval,
    start: config.start,
    end: config.end,
    candles: [],
    gas: [],
    funding: [],
    warnings,
  };
}

function mergeWarnings(...groups: Array<string[] | undefined>) {
  return Array.from(new Set(groups.flatMap((group) => group ?? [])));
}

function compactPortfolioAmount(value: string) {
  const parsed = Number(value.replace(/,/g, ""));
  if (!Number.isFinite(parsed)) return value;
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: 0,
    maximumFractionDigits: 4,
  }).format(parsed);
}

function portfolioRowsFromConfig(config: BacktestConfig) {
  return Object.entries(config.initial_portfolio).flatMap(([venue, assets]) =>
    Object.entries(assets).map(([asset, amount]) => ({
      venue,
      asset,
      amount: compactPortfolioAmount(amount),
      percent: "-",
    })),
  );
}

function labelForDataSourceMode(mode: DataSourceMode) {
  return mode === "store" ? "Parquet store" : "Inline fallback";
}

function previewFromCachedRun(detail: CachedRunDetail): GraphPreview {
  return {
    graph_hash: detail.metadata.graph_hash ?? detail.graphHash ?? "cached-run",
    valid: true,
  };
}

function listItemFromCachedRun(detail: CachedRunDetail): BacktestListItem {
  return {
    id: detail.runId,
    graph_hash: detail.metadata.graph_hash ?? detail.graphHash,
    status: detail.status.status,
    policy_profile: detail.request.policyProfile,
    start: detail.metadata.config?.start ?? detail.request.config.start,
    end: detail.metadata.config?.end ?? detail.request.config.end,
    interval: detail.metadata.config?.interval ?? detail.request.config.interval,
    created_at: detail.status.created_at ?? detail.metadata.created_at,
    started_at: detail.status.started_at ?? detail.metadata.started_at,
    finished_at: detail.status.finished_at ?? detail.metadata.finished_at,
    summary: detail.metadata.summary ?? detail.result.summary,
    warning_count: detail.metadata.warnings?.length ?? detail.result.metadata?.warnings?.length ?? 0,
  };
}

function upsertHistoryItem(items: BacktestListItem[], item: BacktestListItem) {
  const next = items.filter((existing) => existing.id !== item.id);
  return [item, ...next];
}

/**
 * Union of several history lists, deduped by run id, newest first. Earlier
 * lists win on collision — pass the freshest source (the live service list)
 * first and the locally-cached runs after, so the cache fills in runs the
 * in-memory service has forgotten without overwriting fresher status.
 */
function mergeHistoryItems(...lists: BacktestListItem[][]): BacktestListItem[] {
  const byId = new Map<string, BacktestListItem>();
  for (const list of lists) {
    for (const item of list) {
      if (!byId.has(item.id)) byId.set(item.id, item);
    }
  }
  return [...byId.values()].sort((a, b) => (b.created_at ?? "").localeCompare(a.created_at ?? ""));
}

function withoutDeletedHistoryItems(items: BacktestListItem[], deletedRunIds: string[]) {
  if (!deletedRunIds.length) return items;
  const deleted = new Set(deletedRunIds);
  return items.filter((item) => !deleted.has(item.id));
}

function stringConfig(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function objectConfig(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

type MarketRequirement =
  | { kind: "candles"; venue: string; symbol: string }
  | { kind: "funding"; venue: string; symbol: string }
  | { kind: "gas"; chain: string }
  | { kind: "yields"; protocol: string; asset: string; chain: string; pool?: string };

function requirementKey(req: MarketRequirement) {
  if (req.kind === "candles" || req.kind === "funding") return `${req.kind}:${req.venue}:${req.symbol}`;
  if (req.kind === "gas") return `${req.kind}:${req.chain}`;
  return `${req.kind}:${req.protocol}:${req.asset}:${req.chain}:${req.pool ?? ""}`;
}

function requiredMarketData(graph: CatalystGraph): MarketRequirement[] {
  const requirements = new Map<string, MarketRequirement>();
  const stableAssets = new Set(["USD", "USDC", "USDT", "DAI"]);

  const add = (req: MarketRequirement) => {
    requirements.set(requirementKey(req), req);
  };
  const addCandle = (venue: string, symbol: string) => {
    const req: MarketRequirement = { kind: "candles", venue, symbol };
    add(req);
  };
  const addGas = (chain: string) => {
    if (chain !== "hyperliquid") add({ kind: "gas", chain });
  };
  const addYield = (config: Record<string, unknown>) => {
    const protocol = stringConfig(config.protocol);
    const asset = stringConfig(config.asset);
    const chain = stringConfig(config.chain);
    if (!protocol || !asset || !chain) return;
    add({ kind: "yields", protocol, asset, chain, pool: stringConfig(config.pool) });
    addGas(chain);
  };
  const addThresholdSource = (source: unknown) => {
    const config = objectConfig(source);
    if (config?.kind === "yield") addYield(config);
    if (config?.kind === "funding") {
      const venue = stringConfig(config.venue);
      const symbol = stringConfig(config.symbol);
      if (venue && symbol) add({ kind: "funding", venue, symbol });
    }
    if (config?.kind === "price") {
      const venue = stringConfig(config.venue);
      const symbol = stringConfig(config.symbol);
      if (venue && symbol) addCandle(venue, symbol);
    }
  };

  graph.nodes.forEach((node) => {
    const config = node.config ?? {};
    if (node.subtype === "perp_order") {
      const venue = stringConfig(config.chain);
      const symbol = stringConfig(config.symbol);
      if (venue && symbol) {
        addCandle(venue, symbol);
        add({ kind: "funding", venue, symbol });
      }
    }
    if (node.subtype === "swap") {
      const venue = stringConfig(config.chain);
      [stringConfig(config.from_asset), stringConfig(config.to_asset)].forEach((asset) => {
        if (venue && asset && !stableAssets.has(asset)) addCandle(venue, asset);
      });
      if (venue) addGas(venue);
    }
    if (node.subtype === "yield_deposit" || node.subtype === "yield_withdraw") addYield(config);
    if (node.subtype === "threshold") {
      addThresholdSource(config.source);
      const reference = objectConfig(config.reference);
      if (reference?.source) addThresholdSource(reference.source);
    }
  });

  return Array.from(requirements.values());
}

function compatibleMarketItem(graph: CatalystGraph, catalog: MarketDataCatalogItem[]) {
  const required = requiredMarketData(graph);
  if (required.length === 0) return catalog.find((item) => item.kind === "candles") ?? catalog[0];
  return catalog.find((item) =>
    required.some((req) => {
      if (req.kind !== item.kind) return false;
      if (req.kind === "candles" || req.kind === "funding") {
        return item.venue === req.venue && item.symbol === req.symbol;
      }
      if (req.kind === "gas") return item.chain === req.chain;
      return (
        item.protocol === req.protocol &&
        item.asset === req.asset &&
        item.chain === req.chain &&
        (item.pool ?? undefined) === req.pool
      );
    }),
  );
}

export function App() {
  const [activeRoute, setActiveRoute] = useState<RouteId>(() => initialRouteFromStorage());
  const [activeDetailTab, setActiveDetailTab] = useState<DetailTabId>("result");
  const [visitedDetailTabs, setVisitedDetailTabs] = useState<Record<DetailTabId, boolean>>({
    result: true,
    replay: false,
    lens: false,
  });
  const [selectedEventId, setSelectedEventId] = useState(marketReplay.selectedEventId);
  const [apiStatus, setApiStatus] = useState<ApiStatus>("checking");
  const [apiMessage, setApiMessage] = useState(`Checking ${catalystApi.baseUrl}`);
  const [runStatus, setRunStatus] = useState<RunStatus>("idle");
  const [dataSourceMode, setDataSourceMode] = useState<DataSourceMode>("store");
  const [strategyLoading, setStrategyLoading] = useState(false);
  const [strategies, setStrategies] = useState<StrategyListItem[]>([]);
  const [scenarios, setScenarios] = useState<StrategyScenarioListItem[]>([]);
  const [policyProfiles, setPolicyProfiles] = useState<PolicyProfileOption[]>([]);
  const [marketCatalog, setMarketCatalog] = useState<MarketDataCatalogItem[]>([]);
  const [marketWarnings, setMarketWarnings] = useState<string[]>([]);
  const [deletedRunIds, setDeletedRunIds] = useState<string[]>(() => loadDeletedRunIds());
  const [selectedMarketDataId, setSelectedMarketDataId] = useState<string>();
  const [activeGraph, setActiveGraph] = useState<CatalystGraph>(demoGraph);
  const [activeConfig, setActiveConfig] = useState<BacktestConfig>(demoConfig);
  const [activeMarketData, setActiveMarketData] = useState<MarketDataBundle>(demoMarketData);
  const [resolvedVariables, setResolvedVariables] = useState<Record<string, unknown>>({});
  const [activeSelection, setActiveSelection] = useState<ActiveSelection>({
    strategyId: "g_inline_service_demo",
    strategyTitle: "ETH service backtest",
    scenarioId: "inline_demo",
    scenarioTitle: "Inline demo fallback",
  });
  const [workbench, setWorkbench] = useState<WorkbenchState>({
    graph,
    setup,
    marketReplay,
    result,
    audit,
    runHistory,
    historyItems: [],
  });

  useEffect(() => {
    if (activeRoute !== "details") return;
    setVisitedDetailTabs((current) =>
      current[activeDetailTab] ? current : { ...current, [activeDetailTab]: true },
    );
  }, [activeDetailTab, activeRoute]);
  const clipboard = useClipboard({ timeout: 900 });
  const hydrationSeq = useRef(0);

  const selectedEvent = useMemo(
    () => workbench.marketReplay.events.find((event) => event.id === selectedEventId),
    [selectedEventId, workbench.marketReplay.events],
  );

  const dataSourceLabel =
    dataSourceMode === "store" ? "Parquet store" : "Inline fallback";

  useEffect(() => {
    storeActiveRoute(activeRoute);
  }, [activeRoute]);

  function configFromMarketItem(base: BacktestConfig, item?: MarketDataCatalogItem): BacktestConfig {
    return {
      ...base,
      start: DEFAULT_BACKTEST_START,
      end: DEFAULT_BACKTEST_END,
      interval: item?.interval ?? base.interval,
    };
  }

  function updateRunConfig(patch: Partial<Pick<BacktestConfig, "start" | "end" | "interval">>) {
    setActiveConfig((current) => {
      const next = { ...current, ...patch };
      setWorkbench((workbenchState) => ({
        ...workbenchState,
        setup: {
          ...workbenchState.setup,
          start: next.start,
          end: next.end,
          interval: next.interval,
        },
      }));
      return next;
    });
    setApiMessage(`Configuration updated / ${dataSourceLabel}`);
  }

  function updatePortfolioConfig(initialPortfolio: BacktestConfig["initial_portfolio"]) {
    setActiveConfig((current) => {
      const next = { ...current, initial_portfolio: initialPortfolio };
      setWorkbench((workbenchState) => ({
        ...workbenchState,
        setup: {
          ...workbenchState.setup,
          portfolio: portfolioRowsFromConfig(next),
        },
      }));
      return next;
    });
    setApiMessage(`Initial balances updated / ${dataSourceLabel}`);
  }

  function applyRunDetail(detail: CachedRunDetail) {
    const replayMarketData = detail.replayMarketData ?? emptyMarketData(detail.request.config);
    const replay = marketReplayFromApi(detail.result, detail.events, replayMarketData);
    const review = resultFromApi(detail.result, detail.status.status);
    const auditData = auditFromApi(detail.events, detail.result, replay);
    const setupData = setupFromService({
      runId: detail.runId,
      graph: detail.request.graph,
      config: detail.request.config,
      policyProfile: detail.request.policyProfile,
      dataSourceLabel: labelForDataSourceMode(detail.request.dataSourceMode),
      metadata: detail.metadata,
    });
    const graphData = graphFromPreview(detail.request.graph, previewFromCachedRun(detail), {
      id: detail.strategyId,
      name: detail.strategyTitle,
      version: detail.scenarioId,
    });
    const historyItem = listItemFromCachedRun(detail);

    setDataSourceMode(detail.request.dataSourceMode);
    setActiveGraph(detail.request.graph);
    setActiveConfig(detail.request.config);
    setActiveMarketData(replayMarketData);
    setSelectedMarketDataId(detail.request.marketDataId);
    setActiveSelection((current) => ({
      strategyId: detail.strategyId ?? current.strategyId,
      strategyTitle: detail.strategyTitle ?? current.strategyTitle,
      scenarioId: detail.scenarioId ?? current.scenarioId,
      scenarioTitle: detail.scenarioTitle ?? current.scenarioTitle,
    }));
    setWorkbench((current) => {
      const historyItems = upsertHistoryItem(current.historyItems, historyItem);
      return {
        ...current,
        graph: graphData,
        setup: {
          ...setupData,
          coverage: current.setup.coverage,
        },
        marketReplay: replay,
        result: review,
        audit: auditData,
        historyItems,
        runHistory: runHistoryFromApi(historyItems),
      };
    });
    setSelectedEventId(replay.selectedEventId);
    setRunStatus(detail.status.status === "failed" ? "failed" : "succeeded");
    setLastRunId(detail.runId);
  }

  async function refreshRunDetailFromServer(cached: CachedRunDetail): Promise<CachedRunDetail> {
    const marketDataWindowRequest = {
      graph: cached.request.graph,
      start: cached.request.config.start,
      end: cached.request.config.end,
      interval: cached.request.config.interval,
      ...(cached.request.dataSourceMode === "inline" && cached.replayMarketData
        ? { market_data: cached.replayMarketData }
        : {}),
    };
    const replayMarketDataPromise = catalystApi
      .loadMarketDataWindow(marketDataWindowRequest)
      .catch(() => cached.replayMarketData);
    const [status, result, metadata, events, replayMarketData] = await Promise.all([
      catalystApi.getBacktest(cached.runId),
      catalystApi.getResult(cached.runId),
      catalystApi.getMetadata(cached.runId),
      catalystApi.getEvents(cached.runId, { limit: 100 }),
      replayMarketDataPromise,
    ]);

    return makeCachedRunDetail({
      runId: cached.runId,
      graphHash: metadata.graph_hash ?? cached.graphHash,
      strategyId: cached.strategyId,
      strategyTitle: cached.strategyTitle,
      scenarioId: cached.scenarioId,
      scenarioTitle: cached.scenarioTitle,
      request: cached.request,
      status,
      result,
      metadata,
      events: events.items,
      replayMarketData: replayMarketData ?? cached.replayMarketData,
    });
  }

  async function loadUncachedRunDetailFromServer(runId: string): Promise<CachedRunDetail> {
    const status = await catalystApi.getBacktest(runId);
    const [result, metadata, events] = await Promise.all([
      catalystApi.getResult(runId),
      catalystApi.getMetadata(runId),
      catalystApi.getEvents(runId, { limit: 100 }),
    ]);
    const config: BacktestConfig = {
      ...activeConfig,
      start: metadata.config?.start ?? activeConfig.start,
      end: metadata.config?.end ?? activeConfig.end,
      interval: metadata.config?.interval ?? activeConfig.interval,
    };
    const marketDataWindowRequest = {
      graph: activeGraph,
      start: config.start,
      end: config.end,
      interval: config.interval,
      ...(dataSourceMode === "inline" ? { market_data: activeMarketData } : {}),
    };
    const replayMarketData = await catalystApi
      .loadMarketDataWindow(marketDataWindowRequest)
      .catch(() => activeMarketData);

    return makeCachedRunDetail({
      runId,
      graphHash: metadata.graph_hash,
      strategyId: activeSelection.strategyId,
      strategyTitle: activeSelection.strategyTitle,
      scenarioId: activeSelection.scenarioId,
      scenarioTitle: activeSelection.scenarioTitle,
      request: {
        graph: activeGraph,
        config,
        policyProfile: workbench.setup.policy,
        dataSourceMode,
        marketDataId: selectedMarketDataId,
      },
      status,
      result,
      metadata,
      events: events.items,
      replayMarketData,
    });
  }

  async function loadRunDetail(runId: string, options: { route?: RouteId; quiet?: boolean } = {}) {
    try {
      if (!options.quiet) {
        setApiStatus("checking");
        setApiMessage(`Loading run ${runId}`);
      }
      const cached = await loadCachedRunDetail(runId);
      let detail: CachedRunDetail | undefined;
      let source = "service";
      if (cached) {
        detail = cached;
        source = "browser cache";
        try {
          detail = await refreshRunDetailFromServer(cached);
          await saveCachedRunDetail(detail);
          source = "service";
        } catch {
          source = "browser cache";
        }
      } else {
        try {
          detail = await loadUncachedRunDetailFromServer(runId);
          await saveCachedRunDetail(detail);
        } catch (error) {
          if (options.quiet) return;
          throw error;
        }
      }

      applyRunDetail(detail);
      setApiStatus("healthy");
      setApiMessage(`Loaded run ${runId} from ${source}`);
      if (options.route) setActiveRoute(options.route);
    } catch (error) {
      setApiStatus("failed");
      setApiMessage(errorMessage(error));
    }
  }

  async function hydrateWorkbench(input: {
    graph: CatalystGraph;
    config: BacktestConfig;
    marketData: MarketDataBundle;
    policyProfile: string;
    strategyId: string;
    strategyTitle: string;
    scenarioId: string;
    scenarioTitle: string;
    sourceMode: DataSourceMode;
    profiles?: Array<{ id: string; label?: string }>;
    marketDataId?: string;
  }) {
    const sequence = hydrationSeq.current + 1;
    hydrationSeq.current = sequence;
    const sourceLabel = input.sourceMode === "store" ? "Parquet store" : "Inline fallback";
    const [profiles, preview] = await Promise.all([
      input.profiles ? Promise.resolve({ items: input.profiles }) : catalystApi.listPolicyProfiles(),
      catalystApi.previewGraph(input.graph, { profile: input.policyProfile }),
    ]);
    const history = await catalystApi.listBacktests(preview.graph_hash);
    if (sequence !== hydrationSeq.current) return;

    setDataSourceMode(input.sourceMode);
    setActiveGraph(input.graph);
    setActiveConfig(input.config);
    setActiveMarketData(input.marketData);
    setResolvedVariables(preview.resolved_variables ?? {});
    setSelectedMarketDataId(input.marketDataId);
    setActiveSelection({
      strategyId: input.strategyId,
      strategyTitle: input.strategyTitle,
      scenarioId: input.scenarioId,
      scenarioTitle: input.scenarioTitle,
    });
    setWorkbench((current) => {
      const historyItems = mergeHistoryItems(history.items, current.historyItems);
      return {
        ...current,
        graph: graphFromPreview(input.graph, preview, {
          id: input.strategyId,
          name: input.strategyTitle,
          version: input.scenarioId,
        }),
        marketReplay: marketReplayWithMarketData(current.marketReplay, input.marketData),
        setup: setupFromService({
          graph: input.graph,
          config: input.config,
          policyProfile: input.policyProfile,
          dataSourceLabel: sourceLabel,
          preview,
          profiles: profiles.items,
        }),
        runHistory: historyItems.length ? runHistoryFromApi(historyItems) : current.runHistory,
        historyItems,
      };
    });
    setApiStatus("healthy");
    setApiMessage(`Connected to ${catalystApi.baseUrl} / ${sourceLabel}; checking coverage`);

    const coverageRequest = {
      graph: input.graph,
      start: input.config.start,
      end: input.config.end,
      interval: input.config.interval,
      ...(input.sourceMode === "inline" ? { market_data: input.marketData } : {}),
    };
    void catalystApi
      .checkCoverage(coverageRequest)
      .then((coverage) => {
        if (sequence !== hydrationSeq.current) return;
        const coverageWithMarketWarnings: CoverageResponse = {
          ...coverage,
          warnings: mergeWarnings(coverage.warnings, input.marketData.warnings),
        };
        setWorkbench((current) => ({
          ...current,
          setup: setupFromService({
            graph: input.graph,
            config: input.config,
            policyProfile: input.policyProfile,
            dataSourceLabel: sourceLabel,
            coverage: coverageWithMarketWarnings,
            preview,
            profiles: profiles.items,
          }),
        }));
        setApiMessage(`Connected to ${catalystApi.baseUrl} / ${sourceLabel}`);
      })
      .catch((error) => {
        if (sequence !== hydrationSeq.current) return;
        setApiMessage(`Coverage check delayed: ${errorMessage(error)}`);
      });
  }

  async function hydrateWithMarketItem(input: {
    graph: CatalystGraph;
    baseConfig: BacktestConfig;
    policyProfile: string;
    strategyId: string;
    strategyTitle: string;
    marketItem?: MarketDataCatalogItem;
    sourceMode?: DataSourceMode;
    profiles?: Array<{ id: string; label?: string }>;
  }) {
    const config = configFromMarketItem(input.baseConfig, input.marketItem);
    if (input.marketItem || input.sourceMode === "store") {
      await hydrateWorkbench({
        graph: input.graph,
        config,
        marketData: emptyMarketData(config),
        policyProfile: input.policyProfile,
        strategyId: input.strategyId,
        strategyTitle: input.strategyTitle,
        scenarioId: "local-market-data",
        scenarioTitle: "Local market data",
        sourceMode: "store",
        profiles: input.profiles,
        marketDataId: input.marketItem ? marketCatalogId(input.marketItem) : undefined,
      });
      return;
    }
    await hydrateWorkbench({
      graph: input.graph,
      config: input.baseConfig,
      marketData: demoMarketData,
      policyProfile: input.policyProfile,
      strategyId: input.strategyId,
      strategyTitle: input.strategyTitle,
      scenarioId: "inline-fallback",
      scenarioTitle: "Inline fallback",
      sourceMode: "inline",
      profiles: input.profiles,
    });
  }

  async function loadStrategySelection(strategyId: string) {
    try {
      setStrategyLoading(true);
      setApiStatus("checking");
      setApiMessage(`Loading ${strategyId}`);
      const strategy = await catalystApi.getStrategy(strategyId);
      const selectedMarketItem = marketCatalog.find((item) => marketCatalogId(item) === selectedMarketDataId);
      const compatibleSelected =
        selectedMarketItem &&
        compatibleMarketItem(strategy.graph, [selectedMarketItem])
          ? selectedMarketItem
          : undefined;
      const marketItem = compatibleSelected ?? compatibleMarketItem(strategy.graph, marketCatalog);
      await hydrateWithMarketItem({
        graph: strategy.graph,
        baseConfig: activeConfig,
        policyProfile: workbench.setup.policy,
        strategyId: strategy.id,
        strategyTitle: strategy.title,
        marketItem,
        sourceMode: marketCatalog.length ? "store" : undefined,
      });
      setSelectedEventId((current) => {
        const stillExists = workbench.marketReplay.events.some((event) => event.id === current);
        return stillExists ? current : marketReplay.selectedEventId;
      });
    } catch (error) {
      setApiStatus("failed");
      setApiMessage(errorMessage(error));
    } finally {
      setStrategyLoading(false);
    }
  }

  async function applyVariables(next: Record<string, string>) {
    try {
      setStrategyLoading(true);
      setApiStatus("checking");
      setApiMessage("Applying parameters");
      await hydrateWorkbench({
        graph: { ...activeGraph, variables: next },
        config: activeConfig,
        marketData: activeMarketData,
        policyProfile: workbench.setup.policy,
        strategyId: activeSelection.strategyId,
        strategyTitle: activeSelection.strategyTitle,
        scenarioId: activeSelection.scenarioId,
        scenarioTitle: activeSelection.scenarioTitle,
        sourceMode: dataSourceMode,
        marketDataId: selectedMarketDataId,
      });
    } catch (error) {
      setApiStatus("failed");
      setApiMessage(errorMessage(error));
    } finally {
      setStrategyLoading(false);
    }
  }

  async function loadMarketSelection(id: string) {
    const marketItem = marketCatalog.find((item) => marketCatalogId(item) === id);
    if (!marketItem) return;
    try {
      setStrategyLoading(true);
      setApiStatus("checking");
      setApiMessage(`Loading ${marketItem.venue ?? marketItem.chain ?? "market"} ${marketItem.symbol ?? ""}`);
      await hydrateWithMarketItem({
        graph: activeGraph,
        baseConfig: activeConfig,
        policyProfile: workbench.setup.policy,
        strategyId: activeSelection.strategyId,
        strategyTitle: activeSelection.strategyTitle,
        marketItem,
      });
    } catch (error) {
      setApiStatus("failed");
      setApiMessage(errorMessage(error));
    } finally {
      setStrategyLoading(false);
    }
  }

  async function loadPolicySelection(profile: string) {
    try {
      setStrategyLoading(true);
      setApiStatus("checking");
      setApiMessage(`Loading policy ${profile}`);
      await hydrateWorkbench({
        graph: activeGraph,
        config: activeConfig,
        marketData: activeMarketData,
        policyProfile: profile,
        strategyId: activeSelection.strategyId,
        strategyTitle: activeSelection.strategyTitle,
        scenarioId: activeSelection.scenarioId,
        scenarioTitle: activeSelection.scenarioTitle,
        sourceMode: dataSourceMode,
        profiles: policyProfiles,
        marketDataId: selectedMarketDataId,
      });
    } catch (error) {
      setApiStatus("failed");
      setApiMessage(errorMessage(error));
    } finally {
      setStrategyLoading(false);
    }
  }

  useEffect(() => {
    let cancelled = false;

    async function loadServiceWorkbench() {
      // Merge every locally-cached run into the history list so the user's full
      // backtest history survives a refresh — the in-memory service only knows
      // about runs from the current process, but IndexedDB keeps them all.
      async function restoreCachedHistory() {
        const cached = await loadAllCachedRunDetails();
        if (cancelled || !cached.length) return;
        const cachedItems = cached.map(listItemFromCachedRun);
        setWorkbench((current) => {
          const historyItems = mergeHistoryItems(current.historyItems, cachedItems);
          return {
            ...current,
            runHistory: historyItems.length ? runHistoryFromApi(historyItems) : current.runHistory,
            historyItems,
          };
        });
      }

      async function restoreLastRun() {
        await restoreCachedHistory();
        const lastRunId = getLastRunId();
        if (!lastRunId || cancelled) return;
        await loadRunDetail(lastRunId, { quiet: true });
      }

      try {
        setApiStatus("checking");
        setApiMessage(`Checking ${catalystApi.baseUrl}`);
        await catalystApi.health();
        const [profiles, strategyList, scenarioList, catalog] = await Promise.all([
          catalystApi.listPolicyProfiles(),
          catalystApi.listStrategies(),
          catalystApi.listStrategyScenarios(),
          catalystApi.listMarketDataCatalog().catch(() => ({ source: "parquet-store", items: [], warnings: ["Market data catalog unavailable."] })),
        ]);
        const selectedStrategy = strategyList.items[0];
        const selectedScenario = scenarioList.items[0];
        if (!selectedStrategy || !selectedScenario) throw new Error("Strategy catalog is empty");
        const [strategy, scenario] = await Promise.all([
          catalystApi.getStrategy(selectedStrategy.id),
          catalystApi.getStrategyScenario(selectedScenario.id),
        ]);

        if (cancelled) return;

        setStrategies(strategyList.items);
        setScenarios(scenarioList.items);
        setPolicyProfiles(profiles.items);
        setMarketCatalog(catalog.items);
        setMarketWarnings(catalog.warnings ?? []);
        const marketItem = compatibleMarketItem(strategy.graph, catalog.items);
        if (marketItem) {
          await hydrateWithMarketItem({
            graph: strategy.graph,
            baseConfig: scenario.scenario.config,
            policyProfile: scenario.scenario.policy?.profile ?? setup.policy,
            strategyId: strategy.id,
            strategyTitle: strategy.title,
            marketItem,
            profiles: profiles.items,
          });
          await restoreLastRun();
          return;
        }
        await hydrateWorkbench({
          graph: strategy.graph,
          config: scenario.scenario.config,
          marketData: scenario.scenario.market_data,
          policyProfile: scenario.scenario.policy?.profile ?? setup.policy,
          strategyId: strategy.id,
          strategyTitle: strategy.title,
          scenarioId: scenario.id,
          scenarioTitle: scenario.title,
          sourceMode: "inline",
          profiles: profiles.items,
        });
        await restoreLastRun();
      } catch (error) {
        if (cancelled) return;
        setApiStatus("offline");
        setApiMessage(`Using mock data: ${errorMessage(error)}`);
        await restoreLastRun();
      }
    }

    void loadServiceWorkbench();

    return () => {
      cancelled = true;
    };
  }, []);

  function downloadPayload() {
    const payload = JSON.stringify({ ...workbench, api: { status: apiStatus, message: apiMessage } }, null, 2);
    const blob = new Blob([payload], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "catalyst-backtest-workbench.json";
    link.click();
    URL.revokeObjectURL(url);
  }

  async function waitForRun(id: string): Promise<BacktestStatus> {
    for (let attempt = 0; attempt < 120; attempt += 1) {
      const status = await catalystApi.getBacktest(id);
      if (status.status === "succeeded" || status.status === "failed") return status;
      setRunStatus(status.status === "running" ? "running" : "queued");
      setApiStatus("running");
      setApiMessage(`Backtest ${id} is ${status.status}`);
      await sleep(500);
    }
    throw new Error(`Backtest ${id} did not finish within 60 seconds`);
  }

  async function runBacktest() {
    try {
      setRunStatus("submitting");
      setApiStatus("running");
      setApiMessage("Submitting run to Rust service");
      const request = {
        graph: activeGraph,
        config: activeConfig,
        policy: { profile: workbench.setup.policy },
        ...(dataSourceMode === "inline" ? { market_data: activeMarketData } : {}),
      };
      const created = await catalystApi.createBacktest(request);
      setRunStatus("queued");
      setWorkbench((current) => ({
        ...current,
        setup: { ...current.setup, runId: created.id },
      }));

      const status = await waitForRun(created.id);
      if (status.status === "failed") {
        throw new Error(status.error ?? `Backtest ${created.id} failed`);
      }

      const marketDataWindowRequest = {
        graph: activeGraph,
        start: activeConfig.start,
        end: activeConfig.end,
        interval: activeConfig.interval,
        ...(dataSourceMode === "inline" ? { market_data: activeMarketData } : {}),
      };
      const replayMarketDataPromise = catalystApi
        .loadMarketDataWindow(marketDataWindowRequest)
        .catch(() => activeMarketData);
      const [serviceResult, events, metadata, history, replayMarketData] = await Promise.all([
        catalystApi.getResult(created.id),
        catalystApi.getEvents(created.id, { limit: 100 }),
        catalystApi.getMetadata(created.id),
        catalystApi.listBacktests(),
        replayMarketDataPromise,
      ]);
      const replay = marketReplayFromApi(serviceResult, events.items, replayMarketData);
      const review = resultFromApi(serviceResult, status.status);
      const auditData = auditFromApi(events.items, serviceResult, replay);
      const serviceSetup = setupFromService({
        runId: created.id,
        graph: activeGraph,
        config: activeConfig,
        policyProfile: workbench.setup.policy,
        dataSourceLabel,
        metadata,
      });
      const cachedDetail = makeCachedRunDetail({
        runId: created.id,
        graphHash: metadata.graph_hash,
        strategyId: activeSelection.strategyId,
        strategyTitle: activeSelection.strategyTitle,
        scenarioId: activeSelection.scenarioId,
        scenarioTitle: activeSelection.scenarioTitle,
        request: {
          graph: activeGraph,
          config: activeConfig,
          policyProfile: workbench.setup.policy,
          dataSourceMode,
          marketDataId: selectedMarketDataId,
        },
        status,
        result: serviceResult,
        metadata,
        events: events.items,
        replayMarketData,
      });

      setWorkbench((current) => {
        const historyItems = mergeHistoryItems(history.items, [listItemFromCachedRun(cachedDetail)], current.historyItems);
        return {
          ...current,
          setup: {
            ...current.setup,
            ...serviceSetup,
            coverage: current.setup.coverage,
          },
          marketReplay: replay,
          result: review,
          audit: auditData,
          runHistory: historyItems.length ? runHistoryFromApi(historyItems) : current.runHistory,
          historyItems,
        };
      });
      setActiveMarketData(replayMarketData);
      setSelectedEventId(replay.selectedEventId);
      setRunStatus("succeeded");
      setApiStatus("healthy");
      setApiMessage(`Backtest ${created.id} completed / ${dataSourceLabel}`);
      setLastRunId(created.id);
      void saveCachedRunDetail(cachedDetail).catch(() => undefined);
      openRunDetails("result");
    } catch (error) {
      setRunStatus("failed");
      setApiStatus("failed");
      setApiMessage(errorMessage(error));
    }
  }

  const runLabel =
    strategyLoading
      ? "Loading strategy"
      : runStatus === "submitting"
      ? "Submitting"
      : runStatus === "queued"
        ? "Queued"
        : runStatus === "running"
          ? "Running"
          : "Run backtest";
  const isRunning = strategyLoading || runStatus === "submitting" || runStatus === "queued" || runStatus === "running";

  function openRunDetails(tab: DetailTabId = "result") {
    setActiveDetailTab(tab);
    setActiveRoute("details");
  }

  async function openHistoryRunDetail(runId: string | undefined, tab: DetailTabId) {
    if (runId && runId !== workbench.setup.runId) {
      await loadRunDetail(runId);
    }
    openRunDetails(tab);
  }

  async function deleteHistoryRun(runId: string) {
    markRunDeleted(runId);
    setDeletedRunIds(loadDeletedRunIds());
    await deleteCachedRunDetail(runId).catch(() => undefined);
    setWorkbench((current) => {
      const historyItems = current.historyItems.filter((item) => item.id !== runId);
      return {
        ...current,
        historyItems,
        runHistory: historyItems.length ? runHistoryFromApi(historyItems) : current.runHistory.filter((row) => row.id !== runId),
      };
    });
  }

  const renderedDetailTabs = {
    result: visitedDetailTabs.result || activeDetailTab === "result",
    replay: visitedDetailTabs.replay || activeDetailTab === "replay",
    lens: visitedDetailTabs.lens || activeDetailTab === "lens",
  };
  const isMockRun =
    workbench.setup.runId === setup.runId ||
    (workbench.setup.runId === "service_demo" &&
      workbench.result.status === result.status &&
      workbench.result.createdAt === result.createdAt);
  const visibleHistoryItems = useMemo(
    () => withoutDeletedHistoryItems(workbench.historyItems, deletedRunIds),
    [deletedRunIds, workbench.historyItems],
  );
  const visibleRunHistory = useMemo(
    () =>
      visibleHistoryItems.length
        ? runHistoryFromApi(visibleHistoryItems)
        : workbench.runHistory.filter((row) => !deletedRunIds.includes(row.id)),
    [deletedRunIds, visibleHistoryItems, workbench.runHistory],
  );

  return (
    <div className="app-shell">
      <aside className="app-sidebar" aria-label="Workbench navigation">
        <Group className="brand-lockup" gap="sm" align="center">
          <div className="brand-mark" aria-hidden="true">
            <span />
            <span />
            <span />
            <span />
            <span />
          </div>
          <Title order={1}>Catalyst Backtest</Title>
        </Group>

        <Divider />

        <nav className="workflow-nav">
          {routes.map((route) => (
            <NavLink
              key={route.id}
              active={activeRoute === route.id}
              className="workflow-link"
              component="button"
              label={route.label}
              leftSection={route.icon}
              onClick={() => setActiveRoute(route.id)}
              type="button"
              aria-current={activeRoute === route.id ? "page" : undefined}
            />
          ))}
        </nav>

        <Stack className="sidebar-status" gap={4}>
          <Text size="xs" c="dimmed">
            Service
          </Text>
          <Badge
            variant="light"
            color={apiStatus === "healthy" ? "teal" : apiStatus === "offline" ? "gray" : apiStatus === "failed" ? "red" : "blue"}
            radius="sm"
          >
            API {apiStatus}
          </Badge>
          <Text size="xs" c="dimmed" lineClamp={3}>
            {apiMessage}
          </Text>
        </Stack>
      </aside>

      <div className="app-main">
        <header className="topbar">
          <div className="topbar-inner">
            <Stack gap={1} className="workspace-context">
              <Text size="xs" c="dimmed">
                Workspace
              </Text>
              <Text fw={650} size="sm" lineClamp={1}>
                {activeSelection.strategyTitle}
              </Text>
            </Stack>

            <Group className="topbar-actions" gap="xs" justify="flex-end">
              <Stack className="topbar-run-context" gap={0} align="flex-end">
                <Text size="xs" c="dimmed">
                  Selected event
                </Text>
                <Text size="xs" c="dimmed" className="mono">
                  {selectedEvent ? `${selectedEvent.index} ${selectedEvent.label}` : workbench.setup.runId}
                </Text>
              </Stack>
              <Tooltip label={clipboard.copied ? "Copied" : "Copy run ID"}>
                <ActionIcon aria-label="Copy run ID" onClick={() => clipboard.copy(workbench.setup.runId)}>
                  <Clipboard size={16} />
                </ActionIcon>
              </Tooltip>
              <Tooltip label="Download JSON">
                <ActionIcon aria-label="Download JSON" onClick={downloadPayload}>
                  <Download size={16} />
                </ActionIcon>
              </Tooltip>
            </Group>
          </div>
        </header>

        <main className="workspace">
          {activeRoute === "setup" ? (
            <RunSetupPage
              graph={workbench.graph}
              setup={workbench.setup}
              runHistory={visibleRunHistory}
              onRun={runBacktest}
              runLabel={runLabel}
              runDisabled={isRunning}
              strategies={strategies}
              selectedStrategyId={activeSelection.strategyId}
              onSelectStrategy={(id) => void loadStrategySelection(id)}
              selectorDisabled={isRunning}
              marketCatalog={marketCatalog}
              selectedMarketDataId={selectedMarketDataId}
              onSelectMarketData={(id) => void loadMarketSelection(id)}
              marketWarnings={marketWarnings}
              policyMatrix={workbench.audit.policyMatrix}
              policyProfiles={policyProfiles}
              onConfigChange={updateRunConfig}
              onPolicyChange={(profile) => void loadPolicySelection(profile)}
              initialPortfolio={activeConfig.initial_portfolio}
              onPortfolioChange={updatePortfolioConfig}
              variables={activeGraph.variables ?? {}}
              resolvedVariables={resolvedVariables}
              onVariablesChange={(vars) => void applyVariables(vars)}
              variablesBusy={strategyLoading}
            />
          ) : null}
          {activeRoute === "data" ? (
            <MarketDataPage
              catalog={marketCatalog}
              warnings={marketWarnings}
              setup={workbench.setup}
              graph={workbench.graph}
            />
          ) : null}
          {activeRoute === "history" ? (
            <SimulationHistoryPage
              items={visibleHistoryItems}
              fallbackRows={visibleRunHistory}
              selectedRunId={workbench.setup.runId}
              onSelectRun={(id) => void loadRunDetail(id)}
              onOpenResult={(id) => void openHistoryRunDetail(id, "result")}
              onReplayEvents={(id) => void openHistoryRunDetail(id, "replay")}
              onDeleteRun={(id) => void deleteHistoryRun(id)}
            />
          ) : null}
          {activeRoute === "details" ? (
            <Stack gap="md" className="run-details-shell">
              <Group className="run-details-header" justify="space-between" align="flex-start" gap="md">
                <Stack gap={2}>
                  <Text size="xs" c="dimmed">
                    Run details
                  </Text>
                  <Group gap="xs">
                    <Text fw={750} className="mono">
                      {workbench.setup.runId}
                    </Text>
                    {isMockRun ? (
                      <Badge variant="light" color="red" radius="sm">
                        MOCK run
                      </Badge>
                    ) : null}
                    <Badge variant="light" color={workbench.result.status === "succeeded" ? "teal" : "gray"} radius="sm">
                      {workbench.result.status}
                    </Badge>
                  </Group>
                </Stack>
                <SegmentedControl
                  value={activeDetailTab}
                  onChange={(value) => setActiveDetailTab(value as DetailTabId)}
                  data={detailTabs}
                  aria-label="Run detail view"
                />
              </Group>

              {renderedDetailTabs.result ? (
                <div hidden={activeDetailTab !== "result"}>
                  <ResultReviewPage
                    graph={workbench.graph}
                    setup={workbench.setup}
                    result={workbench.result}
                    replay={workbench.marketReplay}
                  />
                </div>
              ) : null}
              {renderedDetailTabs.replay ? (
                <div hidden={activeDetailTab !== "replay"}>
                  <MarketReplayPage
                    graph={workbench.graph}
                    setup={workbench.setup}
                    result={workbench.result}
                    replay={workbench.marketReplay}
                    selectedEventId={selectedEventId}
                    onSelectEvent={setSelectedEventId}
                    onInspectEvent={() => openRunDetails("lens")}
                  />
                </div>
              ) : null}
              {renderedDetailTabs.lens ? (
                <div hidden={activeDetailTab !== "lens"}>
                  <EventLensPage
                    audit={workbench.audit}
                    replay={workbench.marketReplay}
                    setup={workbench.setup}
                    selectedEventId={selectedEventId}
                    selectedReplayEvent={selectedEvent}
                    onSelectEvent={setSelectedEventId}
                  />
                </div>
              ) : null}
            </Stack>
          ) : null}
        </main>
      </div>
    </div>
  );
}
