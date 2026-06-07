import { BarChart } from "@mantine/charts";
import { ActionIcon, Button, Group, Paper, SegmentedControl, Select, SimpleGrid, Stack, Table, Text, TextInput, Tooltip } from "@mantine/core";
import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, CheckCircle2, Copy, CopyPlus, ExternalLink, FileChartColumn, MoreVertical, RotateCcw } from "lucide-react";
import type { BacktestListItem } from "../api/client";
import { CostAttribution } from "../components/CostAttribution";
import { DataTable } from "../components/DataTable";
import { EquityDrawdownChart, type EquityDrawdownPoint } from "../components/EquityDrawdownChart";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";
import type { GraphSummary, ReplayPoint, ResultData, SetupData } from "../types";
import type { UTCTimestamp } from "lightweight-charts";

function shortDate(value?: string | null) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toISOString().replace("T", " ").slice(0, 16) + " UTC";
}

function compactDate(value?: string | null) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toISOString().slice(0, 10);
}

function relativeAge(value?: string | null) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const diffMs = Math.max(Date.now() - date.getTime(), 0);
  const minutes = Math.floor(diffMs / 60_000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

function rowFromFallback(row: Record<string, string>): BacktestListItem {
  return {
    id: row.id,
    status: row.status === "success" ? "succeeded" : row.status === "danger" ? "failed" : row.status,
    policy_profile: row.policy,
    start: row.range?.split(" - ")[0],
    end: row.range?.split(" - ")[1],
    interval: row.interval,
    summary: { return_pct: row.returnUsd },
  };
}

function chartDataFromResult(result: ResultData): EquityDrawdownPoint[] {
  if (result.trend?.length) return result.trend;
  return result.equity.map((value, index) => ({
    time: (Date.UTC(2024, 0, 1, index, 0, 0) / 1000) as UTCTimestamp,
    label: `T${String(index + 1).padStart(2, "0")}`,
    equity: value,
    drawdown: result.drawdown[index] ?? 0,
  }));
}

function coverageTone(status: SetupData["coverage"][number]["status"]) {
  if (status === "success") return "available";
  if (status === "warning") return "partial";
  return "missing";
}

function readinessIcon(ok: boolean) {
  return ok ? <CheckCircle2 size={14} /> : <AlertTriangle size={14} />;
}

export function SimulationHistoryPage({
  items,
  fallbackRows,
  graph,
  setup,
  result,
  replay,
  onOpenResult,
  onReplayEvents,
  selectedRunId,
  onSelectRun,
}: {
  items: BacktestListItem[];
  fallbackRows: Array<Record<string, string>>;
  graph: GraphSummary;
  setup: SetupData;
  result: ResultData;
  replay: ReplayPoint[];
  onOpenResult: () => void;
  onReplayEvents: () => void;
  selectedRunId?: string;
  onSelectRun?: (id: string) => void;
}) {
  const rows = items.length ? items : fallbackRows.map(rowFromFallback);
  const [status, setStatus] = useState("all");
  const [policy, setPolicy] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState(selectedRunId ?? rows[0]?.id);

  useEffect(() => {
    if (selectedRunId) setSelectedId(selectedRunId);
  }, [selectedRunId]);
  const policyOptions = Array.from(new Set(rows.map((row) => row.policy_profile).filter(Boolean))).map((value) => ({
    value: value as string,
    label: value as string,
  }));
  const filtered = rows.filter((row) => {
    const statusOk = status === "all" || row.status === status;
    const policyOk = !policy || row.policy_profile === policy;
    const searchOk = !search || row.id.includes(search) || row.graph_hash?.includes(search);
    return statusOk && policyOk && searchOk;
  });
  const selected = filtered.find((row) => row.id === selectedId) ?? filtered[0] ?? rows[0];
  const summary = useMemo(
    () => ({
      total: rows.length,
      succeeded: rows.filter((row) => row.status === "succeeded").length,
      warning: rows.filter((row) => (row.warning_count ?? 0) > 0).length,
      failed: rows.filter((row) => row.status === "failed").length,
    }),
    [rows],
  );
  const equityDrawdown = useMemo(() => chartDataFromResult(result), [result]);
  const requiredCoverageKinds = ["candles", "gas", "funding"];
  const coverageRows: SetupData["coverage"] = [
    ...setup.coverage,
    ...requiredCoverageKinds
      .filter((kind) => !setup.coverage.some((item) => item.kind.toLowerCase() === kind))
      .map((kind) => ({
        kind,
        source: "missing",
        interval: setup.interval,
        coverage: kind === "funding" ? 50 : 0,
        status: "warning" as const,
      })),
  ];
  const coverageWarning = coverageRows.some((item) => item.status !== "success");
  const readiness = [
    { label: "Graph requirements", detail: "All matched", ok: graph.status !== "danger" && graph.status !== "failed" },
    { label: "Market data coverage", detail: coverageWarning ? "Funding partially missing" : "Complete", ok: !coverageWarning },
    { label: "Initial balances", detail: `${setup.portfolio.length} balance rows`, ok: setup.portfolio.length > 0 },
    { label: "Configuration", detail: "All parameters set", ok: Boolean(setup.policy && setup.interval) },
  ];
  const createdText = selected?.created_at ?? result.createdAt;
  const createdAge = relativeAge(createdText);

  return (
    <Stack gap="md">
      <SectionHeader
        title="Simulation History"
        subtitle="Search previous runs, reopen result review, replay events, or duplicate the setup."
        action={
          <Button leftSection={<RotateCcw size={14} />} variant="light">
            New run
          </Button>
        }
      />

      <Paper className="panel" p="md" radius="sm">
        <SimpleGrid cols={{ base: 1, md: 4 }} spacing="sm">
          <TextInput label="Run or graph hash" value={search} onChange={(event) => setSearch(event.currentTarget.value)} placeholder="af8ceb3f" />
          <Select label="Policy" value={policy} data={policyOptions} onChange={setPolicy} clearable />
          <Select label="Market source" value="parquet-store" data={["parquet-store", "inline"]} readOnly />
          <SegmentedControl
            value={status}
            onChange={setStatus}
            data={[
              { label: "All", value: "all" },
              { label: "Succeeded", value: "succeeded" },
              { label: "Failed", value: "failed" },
            ]}
          />
        </SimpleGrid>
      </Paper>

      <SimpleGrid cols={{ base: 2, md: 4 }} spacing="sm">
        <Paper className="panel-muted metric-cell" p="sm">
          <Text size="xs" c="dimmed">Total runs</Text>
          <Text fw={750}>{summary.total}</Text>
        </Paper>
        <Paper className="panel-muted metric-cell" p="sm">
          <Text size="xs" c="dimmed">Succeeded</Text>
          <Text fw={750}>{summary.succeeded}</Text>
        </Paper>
        <Paper className="panel-muted metric-cell" p="sm">
          <Text size="xs" c="dimmed">Warnings</Text>
          <Text fw={750}>{summary.warning}</Text>
        </Paper>
        <Paper className="panel-muted metric-cell" p="sm">
          <Text size="xs" c="dimmed">Failed</Text>
          <Text fw={750}>{summary.failed}</Text>
        </Paper>
      </SimpleGrid>

      <Paper className="panel" p="md" radius="sm">
        <div className="table-scroll">
          <Table striped highlightOnHover withTableBorder>
            <Table.Thead>
              <Table.Tr>
                <Table.Th>Run</Table.Th>
                <Table.Th>Status</Table.Th>
                <Table.Th>Created</Table.Th>
                <Table.Th>Window</Table.Th>
                <Table.Th>Policy</Table.Th>
                <Table.Th>Return</Table.Th>
                <Table.Th>Warnings</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {filtered.map((row) => (
                <Table.Tr
                  key={row.id}
                  className={selected?.id === row.id ? "selected-row" : undefined}
                  onClick={() => {
                    setSelectedId(row.id);
                    onSelectRun?.(row.id);
                  }}
                >
                  <Table.Td className="mono">{row.id}</Table.Td>
                  <Table.Td><StatusBadge status={row.status} /></Table.Td>
                  <Table.Td>{shortDate(row.created_at)}</Table.Td>
                  <Table.Td>{shortDate(row.start)} to {shortDate(row.end)}</Table.Td>
                  <Table.Td>{row.policy_profile ?? "-"}</Table.Td>
                  <Table.Td>{row.summary?.return_pct ?? "-"}</Table.Td>
                  <Table.Td>{row.warning_count ?? 0}</Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </div>
      </Paper>

      <Paper className="panel history-detail" p="md" radius="sm">
        <Stack gap="md">
          <Group justify="space-between" align="flex-start">
            <Group gap="sm" align="center">
              <Text fw={750}>Run {selected?.id ?? setup.runId}</Text>
              <StatusBadge status={selected?.status ?? result.status} />
              <Text size="xs" c="dimmed">
                Created {shortDate(createdText)}{createdAge ? ` (${createdAge})` : ""}
              </Text>
            </Group>
            <Group gap="xs">
              <Button leftSection={<FileChartColumn size={14} />} onClick={onOpenResult}>Open result</Button>
              <Button leftSection={<RotateCcw size={14} />} variant="light" onClick={onReplayEvents}>Replay events</Button>
              <Button leftSection={<CopyPlus size={14} />} variant="subtle">Duplicate setup</Button>
              <Tooltip label="More actions">
                <ActionIcon aria-label="More actions">
                  <MoreVertical size={16} />
                </ActionIcon>
              </Tooltip>
            </Group>
          </Group>

          <div className="history-detail-grid">
            <Stack className="history-detail-side" gap="md">
              <Paper className="panel-muted history-detail-panel" p="sm">
                <Stack gap="xs">
                  <Text fw={650} size="sm">Run readiness snapshot</Text>
                  {readiness.map((item) => (
                    <Group key={item.label} className={item.ok ? "history-readiness ok" : "history-readiness warn"} justify="space-between" gap="xs">
                      <Group gap="xs">
                        {readinessIcon(item.ok)}
                        <Text size="xs">{item.label}</Text>
                      </Group>
                      <Text size="xs" fw={650}>{item.detail}</Text>
                    </Group>
                  ))}
                </Stack>
              </Paper>

              <Paper className="panel-muted history-detail-panel" p="sm">
                <Stack gap={8}>
                  <Group justify="space-between">
                    <Text size="xs" c="dimmed">Graph</Text>
                    <Group gap={4}>
                      <Text size="xs" className="mono">{graph.hash} v{graph.version}</Text>
                      <ExternalLink size={12} />
                    </Group>
                  </Group>
                  <Group justify="space-between">
                    <Text size="xs" c="dimmed">Policy profile</Text>
                    <Text size="xs" className="mono">{selected?.policy_profile ?? setup.policy}</Text>
                  </Group>
                  <Group justify="space-between">
                    <Text size="xs" c="dimmed">Source</Text>
                    <Text size="xs" className="mono">parquet-store</Text>
                  </Group>
                  <Group justify="space-between">
                    <Text size="xs" c="dimmed">Deterministic seed</Text>
                    <Text size="xs" className="mono">42</Text>
                  </Group>
                  <Group justify="space-between">
                    <Text size="xs" c="dimmed">Events processed</Text>
                    <Text size="xs" className="mono">{result.timeline.length.toLocaleString()}</Text>
                  </Group>
                  <Group justify="space-between">
                    <Text size="xs" c="dimmed">Backtest engine</Text>
                    <Text size="xs" className="mono">v1.12.0</Text>
                  </Group>
                  <Group justify="space-between">
                    <Text size="xs" c="dimmed">Run id</Text>
                    <Group gap={4}>
                      <Text size="xs" className="mono">{selected?.id ?? setup.runId}</Text>
                      <Copy size={12} />
                    </Group>
                  </Group>
                </Stack>
              </Paper>
            </Stack>

            <Stack gap="md">
              <Paper className="panel-muted history-detail-panel" p="sm">
                <Stack gap="xs">
                  <Text fw={650} size="sm">Market data coverage</Text>
                  <Group className="history-coverage-axis" justify="space-between">
                    <Text size="xs" c="dimmed" className="mono">{shortDate(setup.start)}</Text>
                    <Text size="xs" c="dimmed" className="mono">12:00</Text>
                    <Text size="xs" c="dimmed" className="mono">{shortDate(setup.end)}</Text>
                  </Group>
                  <Stack gap={7}>
                    {coverageRows.map((item) => {
                      const tone = coverageTone(item.status);
                      const width = Math.max(5, Math.min(100, item.coverage));
                      return (
                        <div key={`${item.kind}-${item.source}`} className="history-coverage-row">
                          <Text size="xs">{item.kind} ({item.interval})</Text>
                          <div className="history-coverage-track">
                            <span className={`history-coverage-fill ${tone}`} style={{ width: `${width}%` }} />
                          </div>
                        </div>
                      );
                    })}
                  </Stack>
                  <Group gap="md" className="history-coverage-legend">
                    <Group gap={6}><span className="legend-swatch available" /><Text size="xs" c="dimmed">Available</Text></Group>
                    <Group gap={6}><span className="legend-swatch partial" /><Text size="xs" c="dimmed">Partial or missing</Text></Group>
                    <Group gap={6}><span className="legend-swatch missing" /><Text size="xs" c="dimmed">No data</Text></Group>
                  </Group>
                </Stack>
              </Paper>

              <SimpleGrid cols={1} spacing="md">
                <Paper className="panel-muted history-detail-panel" p="sm">
                  <Stack gap="xs">
                    <Text fw={650} size="sm">Initial balances</Text>
                    <DataTable
                      columns={["Asset", "Venue", "Amount", "Weight"]}
                      rows={setup.portfolio.map((item) => [
                        item.asset,
                        item.venue,
                        <span className="mono">{item.amount}</span>,
                        item.percent,
                      ])}
                    />
                  </Stack>
                </Paper>

                <Paper className="panel-muted history-detail-panel" p="sm">
                  <Stack gap="xs">
                    <Text fw={650} size="sm">Cost breakdown (USD)</Text>
                    <CostAttribution costs={result.costs} compact />
                  </Stack>
                </Paper>
              </SimpleGrid>
            </Stack>

            <Stack gap="md">
              <Paper className="panel-muted history-detail-panel history-chart-panel" p="sm">
                <Stack gap="xs">
                  <Text fw={650} size="sm">Equity and drawdown</Text>
                  <EquityDrawdownChart data={equityDrawdown} />
                </Stack>
              </Paper>

              <Paper className="panel-muted history-detail-panel history-chart-panel" p="sm">
                <Stack gap="xs">
                  <Text fw={650} size="sm">Gas and funding context ({setup.interval})</Text>
                  <BarChart
                    h={210}
                    data={replay}
                    dataKey="label"
                    withLegend
                    series={[
                      { name: "gas", color: "yellow.6" },
                      { name: "funding", color: "violet.6" },
                    ]}
                  />
                </Stack>
              </Paper>
            </Stack>
          </div>
        </Stack>
      </Paper>
    </Stack>
  );
}
