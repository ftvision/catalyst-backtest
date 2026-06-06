import { useMemo, useState } from "react";
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
import { audit, graph, marketReplay, result, runHistory, setup } from "./data/mockData";
import { EventLensPage } from "./pages/EventLensPage";
import { MarketReplayPage } from "./pages/MarketReplayPage";
import { ResultReviewPage } from "./pages/ResultReviewPage";
import { RunSetupPage } from "./pages/RunSetupPage";

type RouteId = "setup" | "replay" | "lens" | "result";

const routes: Array<{ id: RouteId; label: string; icon: React.ReactNode }> = [
  { id: "setup", label: "Run Setup", icon: <Gauge size={14} /> },
  { id: "replay", label: "Market Replay", icon: <CandlestickChart size={14} /> },
  { id: "lens", label: "Event Lens", icon: <Activity size={14} /> },
  { id: "result", label: "Result Review", icon: <FileChartColumn size={14} /> },
];

export function App() {
  const [activeRoute, setActiveRoute] = useState<RouteId>("replay");
  const [selectedEventId, setSelectedEventId] = useState(marketReplay.selectedEventId);
  const clipboard = useClipboard({ timeout: 900 });

  const selectedEvent = useMemo(
    () => marketReplay.events.find((event) => event.id === selectedEventId),
    [selectedEventId],
  );

  function downloadPayload() {
    const payload = JSON.stringify({ graph, setup, marketReplay, result, audit }, null, 2);
    const blob = new Blob([payload], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = "catalyst-backtest-workbench.json";
    link.click();
    URL.revokeObjectURL(url);
  }

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
                <Badge variant="light" color="teal" radius="sm">
                  API healthy
                </Badge>
              </Group>
              <Text size="sm" c="dimmed">
                Graph validation, historical replay, and trace audit.
              </Text>
            </Stack>
          </Group>

          <Group className="topbar-actions" gap="xs" justify="flex-end">
            <Stack className="topbar-run-context" gap={0} align="flex-end">
              <Text size="xs" c="dimmed">
                Selected event
              </Text>
              <Text size="xs" c="dimmed" className="mono">
                {selectedEvent ? `${selectedEvent.index} ${selectedEvent.label}` : setup.runId}
              </Text>
            </Stack>
            <Tooltip label={clipboard.copied ? "Copied" : "Copy run ID"}>
              <ActionIcon aria-label="Copy run ID" onClick={() => clipboard.copy(setup.runId)}>
                <Clipboard size={16} />
              </ActionIcon>
            </Tooltip>
            <Tooltip label="Download JSON">
              <ActionIcon aria-label="Download JSON" onClick={downloadPayload}>
                <Download size={16} />
              </ActionIcon>
            </Tooltip>
            <Button leftSection={<Play size={14} />} onClick={() => setActiveRoute("result")}>
              Run backtest
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
            graph={graph}
            setup={setup}
            runHistory={runHistory}
            onRun={() => setActiveRoute("result")}
          />
        ) : null}
        {activeRoute === "replay" ? (
          <MarketReplayPage
            graph={graph}
            setup={setup}
            result={result}
            replay={marketReplay}
            selectedEventId={selectedEventId}
            onSelectEvent={setSelectedEventId}
            onInspectEvent={() => setActiveRoute("lens")}
          />
        ) : null}
        {activeRoute === "lens" ? (
          <EventLensPage
            audit={audit}
            replay={marketReplay}
            result={result}
            setup={setup}
            selectedEventId={selectedEventId}
            selectedReplayEvent={selectedEvent}
            onSelectEvent={setSelectedEventId}
          />
        ) : null}
        {activeRoute === "result" ? (
          <ResultReviewPage graph={graph} setup={setup} result={result} />
        ) : null}
      </main>
    </div>
  );
}
