import {
  Alert,
  Button,
  Group,
  NumberInput,
  Paper,
  Progress,
  Select,
  SimpleGrid,
  Stack,
  Table,
  Text,
  TextInput,
  Textarea,
  Title,
} from "@mantine/core";
import { Play, ShieldAlert } from "lucide-react";
import { DataTable } from "../components/DataTable";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";
import type { GraphSummary, SetupData } from "../types";

export function RunSetupPage({
  graph,
  setup,
  runHistory,
  onRun,
  runLabel = "Run backtest",
  runDisabled = false,
  graphPayload = '{"graph_id":"g_eth_threshold_base_swap","nodes":7,"edges":6}',
}: {
  graph: GraphSummary;
  setup: SetupData;
  runHistory: Array<Record<string, string>>;
  onRun: () => void;
  runLabel?: string;
  runDisabled?: boolean;
  graphPayload?: string;
}) {
  return (
    <Stack gap="md">
      <SectionHeader
        title="Run Setup"
        subtitle="Resolve graph requirements, portfolio, data coverage, and policy before creating a run."
        action={
          <Button leftSection={<Play size={14} />} onClick={onRun} disabled={runDisabled}>
            {runLabel}
          </Button>
        }
      />

      <div className="setup-grid">
        <Paper className="panel" p="md" radius="sm">
          <Stack gap="md">
            <Group justify="space-between" align="flex-start">
              <Stack gap={2}>
                <Text size="xs" c="dimmed">
                  Graph
                </Text>
                <Title order={2}>{graph.name}</Title>
                <Text size="sm" c="dimmed">
                  {graph.id} / {graph.hash}
                </Text>
              </Stack>
              <StatusBadge status={graph.status} />
            </Group>

            <SimpleGrid cols={3} spacing="xs">
              <Paper className="panel-muted" p="xs">
                <Text size="xs" c="dimmed">
                  Version
                </Text>
                <Text fw={650}>{graph.version}</Text>
              </Paper>
              <Paper className="panel-muted" p="xs">
                <Text size="xs" c="dimmed">
                  Nodes
                </Text>
                <Text fw={650}>{graph.nodeCount}</Text>
              </Paper>
              <Paper className="panel-muted" p="xs">
                <Text size="xs" c="dimmed">
                  Edges
                </Text>
                <Text fw={650}>{graph.edgeCount}</Text>
              </Paper>
            </SimpleGrid>

            <DataTable
              columns={["Node", "Kind", "Detail"]}
              rows={graph.nodes.map((node) => [
                <span className="mono">{node.label}</span>,
                node.kind,
                node.detail,
              ])}
            />
          </Stack>
        </Paper>

        <Stack gap="md">
          <Paper className="panel" p="md" radius="sm">
            <Stack gap="sm">
              <Text fw={650}>Run configuration</Text>
              <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
                <TextInput label="Start" value={setup.start} readOnly />
                <TextInput label="End" value={setup.end} readOnly />
                <Select
                  label="Interval"
                  value={setup.interval}
                  data={["15m", "1h", "4h", "1d"]}
                  readOnly
                />
                <Select
                  label="Policy profile"
                  value={setup.policy}
                  data={[
                    { value: "strict_v1", label: "Strict v1" },
                    { value: "conservative_v1", label: "Conservative v1" },
                    { value: "research_v1", label: "Research v1" },
                  ]}
                  readOnly
                />
                <NumberInput label="Slippage bps" value={10} readOnly />
                <NumberInput label="Max missing candles" value={0} readOnly />
              </SimpleGrid>
              <Textarea
                label="Graph payload"
                minRows={4}
                autosize
                value={graphPayload}
                readOnly
              />
            </Stack>
          </Paper>

          <Paper className="panel" p="md" radius="sm">
            <Stack gap="xs">
              <Text fw={650}>Initial portfolio</Text>
              <DataTable
                columns={["Venue", "Asset", "Amount", "Weight"]}
                rows={setup.portfolio.map((item) => [
                  item.venue,
                  item.asset,
                  <span className="mono">{item.amount}</span>,
                  item.percent,
                ])}
              />
            </Stack>
          </Paper>
        </Stack>
      </div>

      <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="md">
        <Paper className="panel" p="md" radius="sm">
          <Stack gap="sm">
            <Text fw={650}>Data coverage</Text>
            {setup.coverage.map((item) => (
              <Stack key={item.kind} gap={4}>
                <Group justify="space-between">
                  <Text size="sm">
                    {item.kind} <Text span c="dimmed">/ {item.source}</Text>
                  </Text>
                  <Group gap="xs">
                    <Text size="sm" fw={650}>
                      {item.coverage.toFixed(1)}%
                    </Text>
                    <StatusBadge status={item.status} />
                  </Group>
                </Group>
                <Progress
                  value={item.coverage}
                  color={item.status === "warning" ? "yellow" : "teal"}
                  size="sm"
                />
              </Stack>
            ))}
          </Stack>
        </Paper>

        <Paper className="panel" p="md" radius="sm">
          <Stack gap="sm">
            <Text fw={650}>Assumptions and recent runs</Text>
            <Table withTableBorder>
              <Table.Tbody>
                {setup.assumptions.map(([label, value]) => (
                  <Table.Tr key={label}>
                    <Table.Td>{label}</Table.Td>
                    <Table.Td className="mono">{value}</Table.Td>
                  </Table.Tr>
                ))}
              </Table.Tbody>
            </Table>
            <Alert color="yellow" icon={<ShieldAlert size={16} />} title="Coverage warnings">
              {setup.warnings.join(" ")}
            </Alert>
            <DataTable
              columns={["Run", "Policy", "Range", "Return"]}
              rows={runHistory.map((run) => [
                <span className="mono">{run.id}</span>,
                run.policy,
                run.range,
                run.returnUsd,
              ])}
            />
          </Stack>
        </Paper>
      </SimpleGrid>
    </Stack>
  );
}
