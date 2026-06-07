import { ActionIcon, Group, NumberInput, Paper, Popover, Select, SimpleGrid, Stack, Text, TextInput, Title } from "@mantine/core";
import { DateTimePicker } from "@mantine/dates";
import dayjs from "dayjs";
import utc from "dayjs/plugin/utc";
import { useEffect, useMemo, useState } from "react";
import { HelpCircle } from "lucide-react";
import type { BacktestConfig, MarketDataCatalogItem, StrategyListItem } from "../api/client";
import { DataTable } from "../components/DataTable";
import { GraphTopologyPreview } from "../components/GraphTopologyPreview";
import { MarketDataSelector } from "../components/MarketDataSelector";
import { ParametersPanel } from "../components/ParametersPanel";
import { RunReadinessRail } from "../components/RunReadinessRail";
import { SectionHeader } from "../components/SectionHeader";
import { SetupModule } from "../components/SetupModule";
import { SetupStepStrip, type SetupStep } from "../components/SetupStepStrip";
import { StatusBadge } from "../components/StatusBadge";
import type { AuditData, GraphSummary, SetupData } from "../types";

dayjs.extend(utc);

function isoToPickerValue(value: string) {
  const parsed = dayjs.utc(value);
  return parsed.isValid() ? parsed.format("YYYY-MM-DD HH:mm:ss") : null;
}

function pickerValueToIso(value: string | null) {
  if (!value) return undefined;
  const parsed = dayjs.utc(value);
  return parsed.isValid() ? parsed.toISOString() : undefined;
}

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
  policyProfiles = [],
  onConfigChange,
  onPolicyChange,
  variables = {},
  resolvedVariables,
  onVariablesChange,
  variablesBusy = false,
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
  policyProfiles?: Array<{ id: string; label?: string }>;
  onConfigChange?: (patch: Partial<Pick<BacktestConfig, "start" | "end" | "interval">>) => void;
  onPolicyChange?: (profile: string) => void;
  variables?: Record<string, string | number | boolean>;
  resolvedVariables?: Record<string, unknown>;
  onVariablesChange?: (vars: Record<string, string>) => void;
  variablesBusy?: boolean;
}) {
  const [selectedNodeId, setSelectedNodeId] = useState(graph.nodes[0]?.id);

  useEffect(() => {
    if (!graph.nodes.some((node) => node.id === selectedNodeId)) {
      setSelectedNodeId(graph.nodes[0]?.id);
    }
  }, [graph.nodes, selectedNodeId]);

  const strategyOptions = strategies.map((strategy) => ({
    value: strategy.id,
    label: strategy.title,
  }));
  const intervalOptions = Array.from(
    new Set(marketCatalog.map((item) => item.interval).filter((interval): interval is string => Boolean(interval))),
  ).map((interval) => ({ value: interval, label: interval }));
  const resolvedIntervalOptions = intervalOptions.length
    ? intervalOptions
    : [{ value: setup.interval, label: setup.interval }];
  const policyOptions = policyProfiles.length
    ? policyProfiles.map((profile) => ({ value: profile.id, label: profile.label ?? profile.id }))
    : [
        { value: "strict_v1", label: "Strict v1" },
        { value: "conservative_v1", label: "Conservative v1" },
        { value: "research_v1", label: "Research v1" },
      ];
  const hasMarketData = marketCatalog.length > 0;
  const activeMarketWarnings = [
    ...marketWarnings,
    ...setup.warnings.filter((warning) => warning !== "No service warnings for this run."),
  ];
  const coverageStatus = setup.coverage.some((item) => item.status === "danger")
    ? "danger"
    : setup.coverage.some((item) => item.status === "warning") || activeMarketWarnings.length
      ? "warning"
      : "success";
  const graphStatus = graph.status === "validated" ? "success" : "danger";
  const steps: SetupStep[] = useMemo(
    () => [
      { id: "graph", label: "Graph", detail: `${graph.nodeCount} nodes / ${graph.hash}`, status: graphStatus },
      {
        id: "market-data",
        label: "Market data",
        detail: hasMarketData ? `${setup.interval} / ${setup.start}` : "No local series",
        status: hasMarketData ? coverageStatus : "danger",
      },
      { id: "portfolio", label: "Portfolio", detail: `${setup.portfolio.length} balances`, status: setup.portfolio.length ? "success" : "danger" },
      { id: "configuration", label: "Configuration", detail: setup.policy, status: setup.policy ? "success" : "danger" },
    ],
    [coverageStatus, graph.hash, graph.nodeCount, graphStatus, hasMarketData, setup.interval, setup.policy, setup.portfolio.length, setup.start],
  );
  const startValue = isoToPickerValue(setup.start);
  const endValue = isoToPickerValue(setup.end);

  return (
    <Stack gap="md">
      <SectionHeader
        title="Run Setup"
        subtitle="Confirm graph, local market data, portfolio, and policy before creating a run."
      />

      <SetupStepStrip steps={steps} />

      <div className="setup-preflight-grid">
        <Stack gap="md">
          <SetupModule title="Graph" subtitle="Read-only strategy topology." status={graphStatus}>
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
            <GraphTopologyPreview
              nodes={graph.nodes}
              edges={graph.edges}
              selectedNodeId={selectedNodeId}
              onSelectNode={setSelectedNodeId}
            />
          </SetupModule>

          {onVariablesChange && Object.keys(variables).length > 0 ? (
            <SetupModule
              title="Parameters"
              subtitle="Tune this strategy's variables before running."
              status={graphStatus}
            >
              <ParametersPanel
                variables={variables}
                resolved={resolvedVariables}
                onApply={onVariablesChange}
                busy={variablesBusy}
              />
            </SetupModule>
          ) : null}

          <SetupModule title="Market data" subtitle="Choose the local Parquet replay window before running." status={hasMarketData ? coverageStatus : "danger"}>
            <MarketDataSelector
              catalog={marketCatalog}
              selectedId={selectedMarketDataId}
              onSelect={onSelectMarketData}
              disabled={selectorDisabled}
              warnings={activeMarketWarnings}
              requiredKinds={setup.coverage.map((item) => item.kind)}
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
              <DateTimePicker
                label="Start"
                value={startValue}
                valueFormat="YYYY-MM-DD HH:mm"
                dropdownType="popover"
                description="Stored as UTC"
                onChange={(value) => {
                  const next = pickerValueToIso(value);
                  if (next) onConfigChange?.({ start: next });
                }}
              />
              <DateTimePicker
                label="End"
                value={endValue}
                valueFormat="YYYY-MM-DD HH:mm"
                dropdownType="popover"
                description="Stored as UTC"
                onChange={(value) => {
                  const next = pickerValueToIso(value);
                  if (next) onConfigChange?.({ end: next });
                }}
              />
              <Select
                label="Interval"
                value={setup.interval}
                data={resolvedIntervalOptions}
                onChange={(value) => value && onConfigChange?.({ interval: value })}
                disabled={selectorDisabled || resolvedIntervalOptions.length <= 1}
              />
              <Select
                label="Policy profile"
                value={setup.policy}
                data={policyOptions}
                onChange={(value) => value && onPolicyChange?.(value)}
                disabled={selectorDisabled || policyOptions.length === 0}
              />
              <NumberInput label="Slippage bps" value={10} readOnly />
              <NumberInput label="Max missing candles" value={0} readOnly />
              <TextInput label="Run ID" value={setup.runId} readOnly />
              <TextInput label="Timezone" value="UTC" readOnly />
            </SimpleGrid>
          </SetupModule>

          <SetupModule title="Recent runs" subtitle="Short history for this graph. Full history lives in Simulation History.">
            {runHistory.length ? (
              <DataTable
                columns={["Run", "Policy", "Range", "Return"]}
                rows={runHistory.slice(0, 5).map((run) => [
                  <span className="mono">{run.id}</span>,
                  run.policy,
                  run.range,
                  run.returnUsd,
                ])}
              />
            ) : (
              <Text size="sm" c="dimmed">
                No recent runs yet.
              </Text>
            )}
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
