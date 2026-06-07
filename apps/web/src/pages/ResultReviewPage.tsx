import { Group, Paper, SimpleGrid, Stack, Text } from "@mantine/core";
import { CostAttribution } from "../components/CostAttribution";
import { DataTable } from "../components/DataTable";
import { EquityDrawdownChart } from "../components/EquityDrawdownChart";
import { MetricStrip } from "../components/MetricStrip";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";
import type { GraphSummary, MarketReplayData, ResultData, SetupData } from "../types";
import type { UTCTimestamp } from "lightweight-charts";

function shortDate(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toISOString().slice(0, 10);
}

function numberValue(value: string) {
  const parsed = Number(value.replace(/,/g, ""));
  return Number.isFinite(parsed) ? parsed : 0;
}

function compactNumber(value: number, maximumFractionDigits = 4): string {
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: 0,
    maximumFractionDigits,
  }).format(value);
}

function money(value: number) {
  return `$${compactNumber(value, 2)}`;
}

function initialPrice(asset: string, replay: MarketReplayData) {
  if (asset.toUpperCase().includes("USD")) return 1;
  const baseSymbol = replay.symbol.split("/")[0]?.trim().toUpperCase();
  const normalizedAsset = asset.toUpperCase();
  if (
    baseSymbol &&
    (normalizedAsset === baseSymbol || normalizedAsset === `${baseSymbol}-PERP`)
  ) {
    return replay.candles[0]?.close;
  }
  return undefined;
}

function initialPortfolioByVenue(portfolio: SetupData["portfolio"], replay: MarketReplayData) {
  const rows = portfolio.map((row) => {
    const amount = numberValue(row.amount);
    const price = initialPrice(row.asset, replay);
    const value = price === undefined ? undefined : amount * price;
    return { ...row, price, value };
  });
  const totalKnownValue = rows.reduce((sum, row) => sum + (row.value ?? 0), 0);
  const grouped = new Map<string, typeof rows>();
  rows.forEach((row) => {
    grouped.set(row.venue, [...(grouped.get(row.venue) ?? []), row]);
  });
  return Array.from(grouped.entries()).map(([venue, assets]) => ({
    venue,
    total: assets.some((asset) => asset.value !== undefined)
      ? money(assets.reduce((sum, asset) => sum + (asset.value ?? 0), 0))
      : "-",
    assets: assets.map((asset) => ({
      ...asset,
      priceLabel: asset.price === undefined ? "-" : money(asset.price),
      valueLabel: asset.value === undefined ? "-" : money(asset.value),
      weightLabel:
        asset.value === undefined || totalKnownValue <= 0
          ? "-"
          : `${compactNumber((asset.value / totalKnownValue) * 100, 2)}%`,
    })),
  }));
}

function fallbackTrend(result: ResultData, setup: SetupData) {
  const startMs = Date.parse(setup.start);
  const endMs = Date.parse(setup.end);
  const hasSetupWindow = Number.isFinite(startMs) && Number.isFinite(endMs) && endMs > startMs;
  const fallbackStartMs = hasSetupWindow ? startMs : Date.UTC(2024, 0, 1, 0, 0, 0);
  const stepMs =
    hasSetupWindow && result.equity.length > 1
      ? (endMs - startMs) / (result.equity.length - 1)
      : 60 * 60 * 1000;

  return result.equity.map((value, index) => ({
    time: Math.floor((fallbackStartMs + index * stepMs) / 1000) as UTCTimestamp,
    label: `T${String(index + 1).padStart(2, "0")}`,
    equity: value,
    drawdown: result.drawdown[index] ?? 0,
  }));
}

export function ResultReviewPage({
  graph,
  setup,
  result,
  replay,
}: {
  graph: GraphSummary;
  setup: SetupData;
  result: ResultData;
  replay: MarketReplayData;
}) {
  const trend = result.trend ?? fallbackTrend(result, setup);
  const initialPortfolio = initialPortfolioByVenue(setup.portfolio, replay);

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
              <Text fw={650}>Initial portfolio</Text>
              <Text size="xs" c="dimmed">
                Run config
              </Text>
            </Group>
            {initialPortfolio.map((venue) => (
              <Stack key={venue.venue} gap="xs">
                <Group justify="space-between">
                  <Text fw={650}>{venue.venue}</Text>
                  <Text className="mono">{venue.total}</Text>
                </Group>
                <DataTable
                  columns={["Asset", "Starting balance", "Price", "Value", "Weight"]}
                  rows={venue.assets.map((asset) => [
                    asset.asset,
                    <span className="mono">{asset.amount}</span>,
                    asset.priceLabel,
                    asset.valueLabel,
                    asset.weightLabel,
                  ])}
                />
              </Stack>
            ))}
          </Stack>
        </Paper>

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
      </SimpleGrid>

      <SimpleGrid cols={{ base: 1 }} spacing="md">
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
