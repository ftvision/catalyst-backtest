import { Button, Group, Paper, SegmentedControl, Select, SimpleGrid, Stack, Table, Text, TextInput } from "@mantine/core";
import { useMemo, useState } from "react";
import { CopyPlus, FileChartColumn, RotateCcw } from "lucide-react";
import type { BacktestListItem } from "../api/client";
import { CostAttribution } from "../components/CostAttribution";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";
import type { ResultData, SetupData } from "../types";

function shortDate(value?: string | null) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toISOString().replace("T", " ").slice(0, 16) + " UTC";
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

export function SimulationHistoryPage({
  items,
  fallbackRows,
  setup,
  result,
  onOpenResult,
  onReplayEvents,
}: {
  items: BacktestListItem[];
  fallbackRows: Array<Record<string, string>>;
  setup: SetupData;
  result: ResultData;
  onOpenResult: () => void;
  onReplayEvents: () => void;
}) {
  const rows = items.length ? items : fallbackRows.map(rowFromFallback);
  const [status, setStatus] = useState("all");
  const [policy, setPolicy] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState(rows[0]?.id);
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

      <div className="history-grid">
        <Paper className="panel" p="md" radius="sm">
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
                  onClick={() => setSelectedId(row.id)}
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
        </Paper>

        <Paper className="panel" p="md" radius="sm">
          <Stack gap="md">
            <Group justify="space-between" align="flex-start">
              <Stack gap={2}>
                <Text fw={700}>Selected run</Text>
                <Text size="sm" c="dimmed" className="mono">{selected?.id ?? "-"}</Text>
              </Stack>
              <StatusBadge status={selected?.status ?? "warning"} />
            </Group>
            <SimpleGrid cols={2} spacing="xs">
              <Paper className="panel-muted" p="xs">
                <Text size="xs" c="dimmed">Market window</Text>
                <Text size="xs" className="mono">{shortDate(selected?.start)}</Text>
              </Paper>
              <Paper className="panel-muted" p="xs">
                <Text size="xs" c="dimmed">Interval</Text>
                <Text size="sm">{selected?.interval ?? setup.interval}</Text>
              </Paper>
              <Paper className="panel-muted" p="xs">
                <Text size="xs" c="dimmed">Policy</Text>
                <Text size="sm">{selected?.policy_profile ?? setup.policy}</Text>
              </Paper>
              <Paper className="panel-muted" p="xs">
                <Text size="xs" c="dimmed">Max drawdown</Text>
                <Text size="sm">{selected?.summary?.max_drawdown_pct ?? result.metrics.find((metric) => metric.label === "Max DD")?.value ?? "-"}</Text>
              </Paper>
            </SimpleGrid>
            <Stack gap="xs">
              <Text fw={650}>Cost snapshot</Text>
              <CostAttribution costs={result.costs} compact />
            </Stack>
            <Group gap="xs">
              <Button leftSection={<FileChartColumn size={14} />} onClick={onOpenResult}>Open result</Button>
              <Button leftSection={<RotateCcw size={14} />} variant="light" onClick={onReplayEvents}>Replay events</Button>
              <Button leftSection={<CopyPlus size={14} />} variant="subtle">Duplicate setup</Button>
            </Group>
          </Stack>
        </Paper>
      </div>
    </Stack>
  );
}
