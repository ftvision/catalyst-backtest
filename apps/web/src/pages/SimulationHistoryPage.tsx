import { Button, Group, Paper, SegmentedControl, Select, SimpleGrid, Stack, Table, Text, TextInput } from "@mantine/core";
import { useEffect, useMemo, useState } from "react";
import { FileChartColumn, RotateCcw } from "lucide-react";
import type { BacktestListItem } from "../api/client";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";

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
  onOpenResult,
  onReplayEvents,
  selectedRunId,
  onSelectRun,
}: {
  items: BacktestListItem[];
  fallbackRows: Array<Record<string, string>>;
  onOpenResult: (runId?: string) => void;
  onReplayEvents: (runId?: string) => void;
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
        subtitle="Search previous runs and jump into the selected run detail views."
        action={
          <Group gap="xs">
            <Button leftSection={<FileChartColumn size={14} />} onClick={() => onOpenResult(selectedId)}>
              Open result
            </Button>
            <Button leftSection={<RotateCcw size={14} />} variant="light" onClick={() => onReplayEvents(selectedId)}>
              Replay event
            </Button>
          </Group>
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
                <Table.Th>Actions</Table.Th>
              </Table.Tr>
            </Table.Thead>
            <Table.Tbody>
              {filtered.map((row) => (
                <Table.Tr
                  key={row.id}
                  className={selectedId === row.id ? "selected-row" : undefined}
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
                  <Table.Td>
                    <Group gap="xs" wrap="nowrap">
                      <Button
                        size="xs"
                        variant="light"
                        leftSection={<FileChartColumn size={13} />}
                        onClick={(event) => {
                          event.stopPropagation();
                          setSelectedId(row.id);
                          onOpenResult(row.id);
                        }}
                      >
                        Open result
                      </Button>
                      <Button
                        size="xs"
                        variant="subtle"
                        leftSection={<RotateCcw size={13} />}
                        onClick={(event) => {
                          event.stopPropagation();
                          setSelectedId(row.id);
                          onReplayEvents(row.id);
                        }}
                      >
                        Replay event
                      </Button>
                    </Group>
                  </Table.Td>
                </Table.Tr>
              ))}
            </Table.Tbody>
          </Table>
        </div>
      </Paper>
    </Stack>
  );
}
