import { useEffect, useMemo, useState } from "react";
import {
  ActionIcon,
  Badge,
  Button,
  Divider,
  Group,
  NavLink,
  Stack,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import { useClipboard } from "@mantine/hooks";
import {
  Activity,
  CandlestickChart,
  Clipboard,
  Download,
  FileChartColumn,
  Gauge,
  History,
  Play,
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
  type MarketDataCatalogItem,
  type MarketDataBundle,
  type StrategyListItem,
  type StrategyScenarioListItem,
} from "./api/client";
import { demoConfig, demoGraph, demoMarketData } from "./data/demoRequest";
import { audit, graph, marketReplay, result, runHistory, setup } from "./data/mockData";
import { EventLensPage } from "./pages/EventLensPage";
import { MarketReplayPage } from "./pages/MarketReplayPage";
import { ResultReviewPage } from "./pages/ResultReviewPage";
import { RunSetupPage } from "./pages/RunSetupPage";
import { SimulationHistoryPage } from "./pages/SimulationHistoryPage";
import { marketCatalogId } from "./components/MarketDataSelector";
import type { AuditData, GraphSummary, MarketReplayData, ResultData, SetupData } from "./types";

type RouteId = "setup" | "replay" | "lens" | "result" | "history";

const routes: Array<{ id: RouteId; label: string; icon: React.ReactNode }> = [
  { id: "setup", label: "Run Setup", icon: <Gauge size={14} /> },
  { id: "replay", label: "Market Replay", icon: <CandlestickChart size={14} /> },
  { id: "lens", label: "Event Lens", icon: <Activity size={14} /> },
  { id: "result", label: "Result Review", icon: <FileChartColumn size={14} /> },
  { id: "history", label: "History", icon: <History size={14} /> },
];

type ApiStatus = "checking" | "healthy" | "offline" | "running" | "failed";
type RunStatus = "idle" | "submitting" | "queued" | "running" | "succeeded" | "failed";
type DataSourceMode = "store" | "inline";

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

function sleep(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.code ? `${error.code}: ${error.message}` : error.message;
  if (error instanceof Error) return error.message;
  return "Unknown service error";
}

export function App() {
  const [activeRoute, setActiveRoute] = useState<RouteId>("replay");
  const [selectedEventId, setSelectedEventId] = useState(marketReplay.selectedEventId);
  const [apiStatus, setApiStatus] = useState<ApiStatus>("checking");
  const [apiMessage, setApiMessage] = useState(`Checking ${catalystApi.baseUrl}`);
  const [runStatus, setRunStatus] = useState<RunStatus>("idle");
  const [dataSourceMode, setDataSourceMode] = useState<DataSourceMode>("store");
  const [strategyLoading, setStrategyLoading] = useState(false);
  const [strategies, setStrategies] = useState<StrategyListItem[]>([]);
  const [scenarios, setScenarios] = useState<StrategyScenarioListItem[]>([]);
  const [marketCatalog, setMarketCatalog] = useState<MarketDataCatalogItem[]>([]);
  const [marketWarnings, setMarketWarnings] = useState<string[]>([]);
  const [selectedMarketDataId, setSelectedMarketDataId] = useState<string>();
  const [activeGraph, setActiveGraph] = useState<CatalystGraph>(demoGraph);
  const [activeConfig, setActiveConfig] = useState<BacktestConfig>(demoConfig);
  const [activeMarketData, setActiveMarketData] = useState<MarketDataBundle>(demoMarketData);
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
  const clipboard = useClipboard({ timeout: 900 });

  const selectedEvent = useMemo(
    () => workbench.marketReplay.events.find((event) => event.id === selectedEventId),
    [selectedEventId, workbench.marketReplay.events],
  );

  const dataSourceLabel =
    dataSourceMode === "store" ? "Parquet store" : "Inline fallback";

  function configFromMarketItem(base: BacktestConfig, item?: MarketDataCatalogItem): BacktestConfig {
    return {
      ...base,
      start: item?.start ?? base.start,
      end: item?.end ?? base.end,
      interval: item?.interval ?? base.interval,
    };
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
    const sourceLabel = input.sourceMode === "store" ? "Parquet store" : "Inline fallback";
    const [profiles, preview, coverage] = await Promise.all([
      input.profiles ? Promise.resolve({ items: input.profiles }) : catalystApi.listPolicyProfiles(),
      catalystApi.previewGraph(input.graph, { profile: input.policyProfile }),
      catalystApi.checkCoverage({
        graph: input.graph,
        start: input.config.start,
        end: input.config.end,
        interval: input.config.interval,
        ...(input.sourceMode === "inline" ? { market_data: input.marketData } : {}),
      }),
    ]);
    const history = await catalystApi.listBacktests(preview.graph_hash);

    setDataSourceMode(input.sourceMode);
    setActiveGraph(input.graph);
    setActiveConfig(input.config);
    setActiveMarketData(input.marketData);
    setSelectedMarketDataId(input.marketDataId);
    setActiveSelection({
      strategyId: input.strategyId,
      strategyTitle: input.strategyTitle,
      scenarioId: input.scenarioId,
      scenarioTitle: input.scenarioTitle,
    });
    setWorkbench((current) => ({
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
        coverage,
        preview,
        profiles: profiles.items,
      }),
      runHistory: history.items.length ? runHistoryFromApi(history.items) : current.runHistory,
      historyItems: history.items,
    }));
    setApiStatus("healthy");
    setApiMessage(`Connected to ${catalystApi.baseUrl} / ${sourceLabel}`);
  }

  async function hydrateWithMarketItem(input: {
    graph: CatalystGraph;
    baseConfig: BacktestConfig;
    policyProfile: string;
    strategyId: string;
    strategyTitle: string;
    marketItem?: MarketDataCatalogItem;
    profiles?: Array<{ id: string; label?: string }>;
  }) {
    const config = configFromMarketItem(input.baseConfig, input.marketItem);
    if (input.marketItem) {
      const marketData = await catalystApi.loadMarketDataWindow({
        graph: input.graph,
        start: config.start,
        end: config.end,
        interval: config.interval,
      });
      await hydrateWorkbench({
        graph: input.graph,
        config,
        marketData,
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
      const marketItem = marketCatalog.find((item) => marketCatalogId(item) === selectedMarketDataId) ?? marketCatalog.find((item) => item.kind === "candles");
      await hydrateWithMarketItem({
        graph: strategy.graph,
        baseConfig: activeConfig,
        policyProfile: workbench.setup.policy,
        strategyId: strategy.id,
        strategyTitle: strategy.title,
        marketItem,
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

  useEffect(() => {
    let cancelled = false;

    async function loadServiceWorkbench() {
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
        setMarketCatalog(catalog.items);
        setMarketWarnings(catalog.warnings ?? []);
        const marketItem = catalog.items.find((item) => item.kind === "candles");
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
      } catch (error) {
        if (cancelled) return;
        setApiStatus("offline");
        setApiMessage(`Using mock data: ${errorMessage(error)}`);
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

      const [serviceResult, events, metadata, history] = await Promise.all([
        catalystApi.getResult(created.id),
        catalystApi.getEvents(created.id, { limit: 100 }),
        catalystApi.getMetadata(created.id),
        catalystApi.listBacktests(),
      ]);
      const replay = marketReplayFromApi(serviceResult, events.items, activeMarketData);
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

      setWorkbench((current) => ({
        ...current,
        setup: {
          ...current.setup,
          ...serviceSetup,
          coverage: current.setup.coverage,
        },
        marketReplay: replay,
        result: review,
        audit: auditData,
        runHistory: history.items.length ? runHistoryFromApi(history.items) : current.runHistory,
        historyItems: history.items,
      }));
      setSelectedEventId(replay.selectedEventId);
      setRunStatus("succeeded");
      setApiStatus("healthy");
      setApiMessage(`Backtest ${created.id} completed / ${dataSourceLabel}`);
      setActiveRoute("result");
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
              <Button leftSection={<Play size={14} />} onClick={runBacktest} loading={isRunning}>
                {runLabel}
              </Button>
            </Group>
          </div>
        </header>

        <main className="workspace">
          {activeRoute === "setup" ? (
            <RunSetupPage
              graph={workbench.graph}
              setup={workbench.setup}
              runHistory={workbench.runHistory}
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
            />
          ) : null}
          {activeRoute === "replay" ? (
            <MarketReplayPage
              graph={workbench.graph}
              setup={workbench.setup}
              result={workbench.result}
              replay={workbench.marketReplay}
              selectedEventId={selectedEventId}
              onSelectEvent={setSelectedEventId}
              onInspectEvent={() => setActiveRoute("lens")}
            />
          ) : null}
          {activeRoute === "lens" ? (
            <EventLensPage
              audit={workbench.audit}
              replay={workbench.marketReplay}
              result={workbench.result}
              setup={workbench.setup}
              selectedEventId={selectedEventId}
              selectedReplayEvent={selectedEvent}
              onSelectEvent={setSelectedEventId}
            />
          ) : null}
          {activeRoute === "result" ? (
            <ResultReviewPage graph={workbench.graph} setup={workbench.setup} result={workbench.result} />
          ) : null}
          {activeRoute === "history" ? (
            <SimulationHistoryPage
              items={workbench.historyItems}
              fallbackRows={workbench.runHistory}
              setup={workbench.setup}
              result={workbench.result}
              onOpenResult={() => setActiveRoute("result")}
              onReplayEvents={() => setActiveRoute("replay")}
            />
          ) : null}
        </main>
      </div>
    </div>
  );
}
