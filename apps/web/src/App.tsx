import { useEffect, useMemo, useState } from "react";
import {
  ActionIcon,
  Badge,
  Button,
  Group,
  Stack,
  Tabs,
  Text,
  Title,
  Tooltip,
} from "@mantine/core";
import { useClipboard } from "@mantine/hooks";
import {
  Activity,
  Braces,
  CandlestickChart,
  Clipboard,
  Download,
  FileChartColumn,
  Gauge,
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
import { ApiError, catalystApi, type BacktestStatus, type MarketDataBundle } from "./api/client";
import {
  buildDemoBacktestRequest,
  buildStoreBacktestRequest,
  demoConfig,
  demoGraph,
  demoMarketData,
} from "./data/demoRequest";
import { audit, graph, marketReplay, result, runHistory, setup } from "./data/mockData";
import { EventLensPage } from "./pages/EventLensPage";
import { MarketReplayPage } from "./pages/MarketReplayPage";
import { ResultReviewPage } from "./pages/ResultReviewPage";
import { RunSetupPage } from "./pages/RunSetupPage";
import type { AuditData, GraphSummary, MarketReplayData, ResultData, SetupData } from "./types";

type RouteId = "setup" | "replay" | "lens" | "result";

const routes: Array<{ id: RouteId; label: string; icon: React.ReactNode }> = [
  { id: "setup", label: "Run Setup", icon: <Gauge size={14} /> },
  { id: "replay", label: "Market Replay", icon: <CandlestickChart size={14} /> },
  { id: "lens", label: "Event Lens", icon: <Activity size={14} /> },
  { id: "result", label: "Result Review", icon: <FileChartColumn size={14} /> },
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
  const [activeMarketData, setActiveMarketData] = useState<MarketDataBundle>(demoMarketData);
  const [workbench, setWorkbench] = useState<WorkbenchState>({
    graph,
    setup,
    marketReplay,
    result,
    audit,
    runHistory,
  });
  const clipboard = useClipboard({ timeout: 900 });

  const selectedEvent = useMemo(
    () => workbench.marketReplay.events.find((event) => event.id === selectedEventId),
    [selectedEventId, workbench.marketReplay.events],
  );

  useEffect(() => {
    let cancelled = false;

    async function loadServiceWorkbench() {
      try {
        setApiStatus("checking");
        setApiMessage(`Checking ${catalystApi.baseUrl}`);
        await catalystApi.health();
        let sourceMode: DataSourceMode = "store";
        let sourceLabel = "Parquet store";
        let marketData: MarketDataBundle;
        try {
          marketData = await catalystApi.loadMarketDataWindow({
            graph: demoGraph,
            start: demoConfig.start,
            end: demoConfig.end,
            interval: demoConfig.interval,
          });
        } catch (error) {
          sourceMode = "inline";
          sourceLabel = "Inline demo fallback";
          marketData = await catalystApi.loadMarketDataWindow({
            graph: demoGraph,
            start: demoConfig.start,
            end: demoConfig.end,
            interval: demoConfig.interval,
            market_data: demoMarketData,
          });
          console.warn("Fell back to inline market-data demo", error);
        }
        const [profiles, preview, coverage] = await Promise.all([
          catalystApi.listPolicyProfiles(),
          catalystApi.previewGraph(demoGraph, { profile: setup.policy }),
          catalystApi.checkCoverage({
            graph: demoGraph,
            start: demoConfig.start,
            end: demoConfig.end,
            interval: demoConfig.interval,
            ...(sourceMode === "inline" ? { market_data: marketData } : {}),
          }),
        ]);
        const history = await catalystApi.listBacktests(preview.graph_hash);

        if (cancelled) return;

        setDataSourceMode(sourceMode);
        setActiveMarketData(marketData);
        setWorkbench((current) => ({
          ...current,
          graph: graphFromPreview(demoGraph, preview),
          marketReplay: marketReplayWithMarketData(current.marketReplay, marketData),
          setup: setupFromService({
            graph: demoGraph,
            config: demoConfig,
            policyProfile: setup.policy,
            dataSourceLabel: sourceLabel,
            coverage,
            preview,
            profiles: profiles.items,
          }),
          runHistory: history.items.length ? runHistoryFromApi(history.items) : current.runHistory,
        }));
        setApiStatus("healthy");
        setApiMessage(`Connected to ${catalystApi.baseUrl} / ${sourceLabel}`);
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
      const request =
        dataSourceMode === "store"
          ? buildStoreBacktestRequest(workbench.setup.policy)
          : buildDemoBacktestRequest(workbench.setup.policy);
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
        graph: demoGraph,
        config: demoConfig,
        policyProfile: workbench.setup.policy,
        dataSourceLabel: dataSourceMode === "store" ? "Parquet store" : "Inline demo fallback",
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
      }));
      setSelectedEventId(replay.selectedEventId);
      setRunStatus("succeeded");
      setApiStatus("healthy");
      setApiMessage(`Backtest ${created.id} completed / ${dataSourceMode === "store" ? "Parquet store" : "Inline demo fallback"}`);
      setActiveRoute("result");
    } catch (error) {
      setRunStatus("failed");
      setApiStatus("failed");
      setApiMessage(errorMessage(error));
    }
  }

  const runLabel =
    runStatus === "submitting"
      ? "Submitting"
      : runStatus === "queued"
        ? "Queued"
        : runStatus === "running"
          ? "Running"
          : "Run backtest";
  const isRunning = runStatus === "submitting" || runStatus === "queued" || runStatus === "running";

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="topbar-inner">
          <Group gap="sm" align="center">
            <div className="brand-mark" aria-hidden="true">
              <Braces size={18} />
            </div>
            <Stack gap={1}>
              <Group gap="xs">
                <Title order={1}>Catalyst Backtest</Title>
                <Badge
                  variant="light"
                  color={apiStatus === "healthy" ? "teal" : apiStatus === "offline" ? "gray" : apiStatus === "failed" ? "red" : "blue"}
                  radius="sm"
                >
                  API {apiStatus}
                </Badge>
              </Group>
              <Text size="sm" c="dimmed">
                {apiMessage}
              </Text>
            </Stack>
          </Group>

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

      <div className="workflow">
        <Tabs value={activeRoute} onChange={(value) => setActiveRoute(value as RouteId)}>
          <Tabs.List>
            {routes.map((route) => (
              <Tabs.Tab key={route.id} value={route.id} leftSection={route.icon}>
                {route.label}
              </Tabs.Tab>
            ))}
          </Tabs.List>
        </Tabs>
      </div>

      <main className="workspace">
        {activeRoute === "setup" ? (
          <RunSetupPage
            graph={workbench.graph}
            setup={workbench.setup}
            runHistory={workbench.runHistory}
            onRun={runBacktest}
            runLabel={runLabel}
            runDisabled={isRunning}
            dataSourceLabel={dataSourceMode === "store" ? "Parquet store" : "Inline demo fallback"}
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
      </main>
    </div>
  );
}
