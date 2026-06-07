import { ActionIcon, Button, Group, NumberInput, Paper, Popover, Select, SimpleGrid, Stack, Table, Text, TextInput, Title } from "@mantine/core";
import { useMemo, useState } from "react";
import { HelpCircle, Play } from "lucide-react";
import type { MarketDataCatalogItem, StrategyListItem } from "../api/client";
import { DataTable } from "../components/DataTable";
import { MarketDataSelector } from "../components/MarketDataSelector";
import { RunReadinessRail } from "../components/RunReadinessRail";
import { SectionHeader } from "../components/SectionHeader";
import { SetupModule } from "../components/SetupModule";
import { SetupStepStrip, type SetupStep } from "../components/SetupStepStrip";
import { StatusBadge } from "../components/StatusBadge";
import type { AuditData, GraphSummary, SetupData } from "../types";

export function RunSetupPage({
  graph,
  setup,
  runHistory,
  onRun,
  runLabel = "Run backtest",
  runDisabled = false,
  strategies = [],
  selectedStrategyId,
  onSelectStrategy,
  selectorDisabled = false,
  marketCatalog = [],
  selectedMarketDataId,
  onSelectMarketData,
  marketWarnings = [],
  policyMatrix = [],
}: {
  graph: GraphSummary;
  setup: SetupData;
  runHistory: Array<Record<string, string>>;
  onRun: () => void;
  runLabel?: string;
  runDisabled?: boolean;
  strategies?: StrategyListItem[];
  selectedStrategyId?: string;
  onSelectStrategy?: (id: string) => void;
  selectorDisabled?: boolean;
  marketCatalog?: MarketDataCatalogItem[];
  selectedMarketDataId?: string;
  onSelectMarketData?: (id: string) => void;
  marketWarnings?: string[];
  policyMatrix?: AuditData["policyMatrix"];
}) {
  const [selectedNodeId, setSelectedNodeId] = useState(graph.nodes[0]?.id);
  const selectedNode = graph.nodes.find((node) => node.id === selectedNodeId) ?? graph.nodes[0];
  const strategyOptions = strategies.map((strategy) => ({
    value: strategy.id,
    label: strategy.title,
  }));
  const hasMarketData = marketCatalog.some((item) => item.kind === "candles");
  const coverageStatus = setup.coverage.some((item) => item.status === "danger")
    ? "danger"
    : setup.coverage.some((item) => item.status === "warning") || marketWarnings.length
      ? "warning"
      : "success";
  const steps: SetupStep[] = useMemo(
    () => [
      { id: "graph", label: "Graph", detail: `${graph.nodeCount} nodes / ${graph.hash}`, status: graph.status === "validated" ? "success" : "warning" },
      {
        id: "market-data",
        label: "Market data",
        detail: hasMarketData ? `${setup.interval} / ${setup.start}` : "No local series",
        status: hasMarketData ? coverageStatus : "danger",
      },
      { id: "portfolio", label: "Portfolio", detail: `${setup.portfolio.length} balances`, status: setup.portfolio.length ? "success" : "danger" },
      { id: "configuration", label: "Configuration", detail: setup.policy, status: setup.policy ? "success" : "danger" },
    ],
    [coverageStatus, graph.hash, graph.nodeCount, graph.status, hasMarketData, setup.interval, setup.policy, setup.portfolio.length, setup.start],
  );

  return (
    <Stack gap="md">
      <SectionHeader
        title="Run Setup"
        subtitle="Confirm graph, local market data, portfolio, and policy before creating a run."
        action={
          <Button leftSection={<Play size={14} />} onClick={onRun} disabled={runDisabled || !hasMarketData}>
            {runLabel}
          </Button>
        }
      />

      <SetupStepStrip steps={steps} />

      <div className="setup-preflight-grid">
        <Stack gap="md">
          <SetupModule title="Graph" subtitle="Read-only strategy graph and node requirements." status={graph.status === "validated" ? "success" : "warning"}>
            <Group justify="space-between" align="flex-start">
              <Stack gap={2}>
                <Title order={2}>{graph.name}</Title>
                <Text size="sm" c="dimmed" className="mono">
                  {graph.id} / {graph.hash}
                </Text>
              </Stack>
              <StatusBadge status={graph.status} />
            </Group>
            <Select
              label="Strategy"
              value={selectedStrategyId}
              data={strategyOptions}
              onChange={(value) => value && onSelectStrategy?.(value)}
              disabled={selectorDisabled || strategyOptions.length === 0}
              searchable
            />
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
            <div className="node-inspector-grid">
              <Table withTableBorder highlightOnHover>
                <Table.Thead>
                  <Table.Tr>
                    <Table.Th>Node</Table.Th>
                    <Table.Th>Kind</Table.Th>
                    <Table.Th>Detail</Table.Th>
                  </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                  {graph.nodes.map((node) => (
                    <Table.Tr
                      key={node.id}
                      className={selectedNode?.id === node.id ? "selected-row" : undefined}
                      onClick={() => setSelectedNodeId(node.id)}
                    >
                      <Table.Td className="mono">{node.label}</Table.Td>
                      <Table.Td>{node.kind}</Table.Td>
                      <Table.Td>{node.detail}</Table.Td>
                    </Table.Tr>
                  ))}
                </Table.Tbody>
              </Table>
              <Paper className="panel-muted node-details" p="sm" radius="sm">
                <Stack gap="xs">
                  <Group justify="space-between">
                    <Text fw={700}>Node details</Text>
                    <StatusBadge status={selectedNode ? "success" : "warning"} label={selectedNode ? "selected" : "none"} />
                  </Group>
                  <Text size="xs" c="dimmed">
                    Id
                  </Text>
                  <Text className="mono" size="sm">
                    {selectedNode?.id ?? "-"}
                  </Text>
                  <Text size="xs" c="dimmed">
                    Requirement
                  </Text>
                  <Text size="sm">{selectedNode?.detail ?? "Select a node to inspect the requirement."}</Text>
                </Stack>
              </Paper>
            </div>
          </SetupModule>

          <SetupModule title="Market data" subtitle="Choose the local Parquet replay window before running." status={hasMarketData ? coverageStatus : "danger"}>
            <MarketDataSelector
              catalog={marketCatalog}
              selectedId={selectedMarketDataId}
              onSelect={onSelectMarketData}
              disabled={selectorDisabled}
              warnings={marketWarnings}
            />
          </SetupModule>

          <SetupModule title="Initial portfolio" subtitle="Starting balances passed into the simulator." status={setup.portfolio.length ? "success" : "danger"}>
            <DataTable
              columns={["Venue", "Asset", "Amount", "Weight"]}
              rows={setup.portfolio.map((item) => [
                item.venue,
                item.asset,
                <span className="mono">{item.amount}</span>,
                item.percent,
              ])}
            />
          </SetupModule>

          <SetupModule
            title="Configuration"
            subtitle="Policy profile and deterministic simulation assumptions."
            status={setup.policy ? "success" : "danger"}
            action={
              policyMatrix.length ? (
                <Popover width={680} position="bottom-end" shadow="md">
                  <Popover.Target>
                    <ActionIcon variant="light" aria-label="Compare policy profiles">
                      <HelpCircle size={16} />
                    </ActionIcon>
                  </Popover.Target>
                  <Popover.Dropdown>
                    <Stack gap="sm" className="policy-helper">
                      <Stack gap={2}>
                        <Text fw={650}>Policy profile comparison</Text>
                        <Text size="xs" c="dimmed">
                          These assumptions are resolved before the run. Pick the profile here, then inspect event-specific reasons in Event Lens.
                        </Text>
                      </Stack>
                      <DataTable
                        columns={["Rule", "Strict", "Conservative", "Research"]}
                        rows={policyMatrix}
                      />
                    </Stack>
                  </Popover.Dropdown>
                </Popover>
              ) : null
            }
          >
            <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="sm">
              <TextInput label="Start" value={setup.start} readOnly />
              <TextInput label="End" value={setup.end} readOnly />
              <Select label="Interval" value={setup.interval} data={["15m", "1h", "4h", "1d"]} readOnly />
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
              <TextInput label="Run ID" value={setup.runId} readOnly />
              <TextInput label="Timezone" value="UTC" readOnly />
            </SimpleGrid>
          </SetupModule>

          <SetupModule title="Recent runs" subtitle="Short history for this graph. Full history lives in Simulation History." status={runHistory.length ? "success" : "warning"}>
            <DataTable
              columns={["Run", "Policy", "Range", "Return"]}
              rows={runHistory.slice(0, 5).map((run) => [
                <span className="mono">{run.id}</span>,
                run.policy,
                run.range,
                run.returnUsd,
              ])}
            />
          </SetupModule>
        </Stack>

        <RunReadinessRail
          setup={setup}
          graphStatus={graph.status}
          hasMarketData={hasMarketData}
          runLabel={runLabel}
          disabled={runDisabled}
          onRun={onRun}
        />
      </div>
    </Stack>
  );
}
