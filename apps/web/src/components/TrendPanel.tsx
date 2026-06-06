import { BarChart, LineChart } from "@mantine/charts";
import { Paper, Stack, Text } from "@mantine/core";
import type { ReplayPoint } from "../types";
import { formatNumber } from "../utils/format";

export function EquityDrawdownPanel({ data }: { data: ReplayPoint[] }) {
  return (
    <Paper className="panel" p="md" radius="sm">
      <Stack gap="xs">
        <Text fw={650}>Equity and drawdown</Text>
        <LineChart
          h={220}
          data={data}
          dataKey="label"
          withLegend
          withDots={false}
          curveType="linear"
          series={[
            { name: "equity", color: "workbenchBlue.6" },
            { name: "drawdown", color: "red.6" },
          ]}
          valueFormatter={formatNumber}
        />
      </Stack>
    </Paper>
  );
}

export function MarketEvidencePanel({ data }: { data: ReplayPoint[] }) {
  return (
    <Paper className="panel" p="md" radius="sm">
      <Stack gap="xs">
        <Text fw={650}>Gas and funding context</Text>
        <BarChart
          h={220}
          data={data}
          dataKey="label"
          withLegend
          series={[
            { name: "gas", color: "yellow.6" },
            { name: "funding", color: "violet.6" },
          ]}
          valueFormatter={formatNumber}
        />
      </Stack>
    </Paper>
  );
}
