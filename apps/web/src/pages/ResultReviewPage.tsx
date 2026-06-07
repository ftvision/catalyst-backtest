import { Group, Paper, SimpleGrid, Stack, Text } from "@mantine/core";
import { CostAttribution } from "../components/CostAttribution";
import { DataTable } from "../components/DataTable";
import { EquityDrawdownChart } from "../components/EquityDrawdownChart";
import { MetricStrip } from "../components/MetricStrip";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";
import type { GraphSummary, ResultData, SetupData } from "../types";
import type { UTCTimestamp } from "lightweight-charts";

function shortDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toISOString().slice(0, 10);
}

export function ResultReviewPage({
  graph,
  setup,
  result,
}: {
  graph: GraphSummary;
  setup: SetupData;
  result: ResultData;
}) {
  const trend = result.trend ?? result.equity.map((value, index) => ({
    time: (Date.UTC(2024, 0, 1, index, 0, 0) / 1000) as UTCTimestamp,
    label: `T${String(index + 1).padStart(2, "0")}`,
    equity: value,
    drawdown: result.drawdown[index],
  }));

  return (
    <Stack gap="md">
      <SectionHeader
        title="Result Review"
        subtitle="Outcome, portfolio state, trace timeline, and cost attribution for the completed run."
        action={
          <Group gap="xs">
            <StatusBadge status={result.status} />
            <Text size="xs" c="dimmed" className="mono">
              {setup.runId}
            </Text>
          </Group>
        }
      />

      <MetricStrip metrics={result.metrics} />

      <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="md">
        <Paper className="panel" p="md" radius="sm">
          <Stack gap="xs">
            <Group justify="space-between">
              <Text fw={650}>Equity and drawdown</Text>
              <Text size="xs" c="dimmed" className="mono">
                {shortDate(setup.start)} - {shortDate(setup.end)} / {setup.interval}
              </Text>
            </Group>
            <EquityDrawdownChart data={trend} />
          </Stack>
        </Paper>

        <Paper className="panel" p="md" radius="sm">
          <Stack gap="xs">
            <Text fw={650}>Gross to net PnL</Text>
            <CostAttribution costs={result.costs} />
          </Stack>
        </Paper>
      </SimpleGrid>

      <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="md">
        <Paper className="panel" p="md" radius="sm">
          <Stack gap="sm">
            <Group justify="space-between">
              <Text fw={650}>Final portfolio</Text>
              <Text size="xs" c="dimmed">
                Graph {graph.hash}
              </Text>
            </Group>
            {result.portfolio.map((venue) => (
              <Stack key={venue.venue} gap="xs">
                <Group justify="space-between">
                  <Text fw={650}>{venue.venue}</Text>
                  <Text className="mono">{venue.total}</Text>
                </Group>
                <DataTable
                  columns={["Asset", "Balance", "Price", "Value", "Weight"]}
                  rows={venue.assets.map((asset) => [
                    asset.asset,
                    <span className="mono">{asset.balance}</span>,
                    asset.price,
                    asset.value,
                    asset.percent,
                  ])}
                />
              </Stack>
            ))}
          </Stack>
        </Paper>

        <Paper className="panel" p="md" radius="sm">
          <Stack gap="sm">
            <Text fw={650}>Recent trace timeline</Text>
            <DataTable
              columns={["Time", "Node", "Signal", "Action", "Venue", "Fees", "Gas", "Notional"]}
              rows={result.timeline.map((event) => [
                event.time,
                <span className="mono">{event.node}</span>,
                event.signal,
                event.action,
                event.venue,
                event.fees,
                event.gas,
                event.pnl,
              ])}
            />
          </Stack>
        </Paper>
      </SimpleGrid>
    </Stack>
  );
}
