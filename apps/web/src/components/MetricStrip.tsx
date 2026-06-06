import { Paper, SimpleGrid, Stack, Text } from "@mantine/core";
import type { MetricItem } from "../types";

export function MetricStrip({ metrics }: { metrics: MetricItem[] }) {
  return (
    <SimpleGrid cols={{ base: 2, sm: 3, lg: 6 }} spacing="xs">
      {metrics.map((metric) => (
        <Paper key={metric.label} className="panel-muted" p="sm" radius="sm">
          <Stack gap={2}>
            <Text size="xs" c="dimmed">
              {metric.label}
            </Text>
            <Text
              className={[
                "metric-value",
                metric.tone === "positive" ? "metric-positive" : "",
                metric.tone === "negative" ? "metric-negative" : "",
              ].join(" ")}
            >
              {metric.value}
            </Text>
            <Text size="xs" c="dimmed">
              {metric.detail}
            </Text>
          </Stack>
        </Paper>
      ))}
    </SimpleGrid>
  );
}
