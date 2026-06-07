import { Button, Divider, Group, Paper, Stack, Text } from "@mantine/core";
import { Play } from "lucide-react";
import type { SetupData } from "../types";
import { StatusBadge } from "./StatusBadge";

function readinessStatus(setup: SetupData, hasMarketData: boolean) {
  if (!hasMarketData) return "danger";
  if (setup.coverage.some((item) => item.status === "danger")) return "danger";
  if (setup.coverage.some((item) => item.status === "warning") || setup.warnings.length) return "warning";
  return "success";
}

export function RunReadinessRail({
  setup,
  graphStatus,
  hasMarketData,
  runLabel,
  disabled,
  onRun,
}: {
  setup: SetupData;
  graphStatus: string;
  hasMarketData: boolean;
  runLabel: string;
  disabled?: boolean;
  onRun: () => void;
}) {
  const status = readinessStatus(setup, hasMarketData);
  const marketDataReady = hasMarketData && !setup.coverage.some((item) => item.status === "danger");
  const marketDataDetail = !hasMarketData
    ? "No local series"
    : marketDataReady
      ? setup.assumptions.find(([label]) => label === "Data source")?.[1] ?? "-"
      : "Missing required coverage";
  const checks = [
    { label: "Graph validated", value: graphStatus === "validated", detail: graphStatus },
    { label: "Market data coverage", value: marketDataReady, detail: marketDataDetail },
    { label: "Portfolio present", value: setup.portfolio.length > 0, detail: `${setup.portfolio.length} balance rows` },
    { label: "Policy configured", value: Boolean(setup.policy), detail: setup.policy },
  ];

  return (
    <Paper className="panel readiness-rail" p="md" radius="sm">
      <Stack gap="md">
        <Group justify="space-between" align="flex-start">
          <Stack gap={2}>
            <Text fw={700}>Run readiness</Text>
            <Text size="sm" c="dimmed">
              Preflight contract for the current setup.
            </Text>
          </Stack>
          <StatusBadge status={status} />
        </Group>

        <Stack gap="xs">
          {checks.map((check) => (
            <Group key={check.label} justify="space-between" align="flex-start" gap="sm" className="readiness-row">
              <Stack gap={0}>
                <Text size="sm" fw={650}>
                  {check.label}
                </Text>
                <Text size="xs" c="dimmed" className="mono">
                  {check.detail}
                </Text>
              </Stack>
              <StatusBadge status={check.value ? "success" : "danger"} label={check.value ? "ok" : "blocker"} />
            </Group>
          ))}
        </Stack>

        <Divider />

        <Stack gap={4}>
          <Text size="xs" c="dimmed">
            Window
          </Text>
          <Text size="sm" className="mono">
            {setup.start} to {setup.end}
          </Text>
          <Text size="xs" c="dimmed">
            {setup.interval} samples
          </Text>
        </Stack>

        <Stack gap={4}>
          <Text size="xs" c="dimmed">
            Warnings
          </Text>
          <Text size="sm">{setup.warnings.join(" ")}</Text>
        </Stack>

        <Button leftSection={<Play size={14} />} onClick={onRun} disabled={disabled || status === "danger"}>
          {runLabel}
        </Button>
      </Stack>
    </Paper>
  );
}
