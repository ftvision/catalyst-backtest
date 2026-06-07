import { Group, Paper, Stack, Text } from "@mantine/core";
import type { ReactNode } from "react";
import { StatusBadge } from "./StatusBadge";

export function SetupModule({
  title,
  subtitle,
  status = "success",
  children,
  action,
}: {
  title: string;
  subtitle?: string;
  status?: "success" | "warning" | "danger";
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <Paper className="panel setup-module" p="md" radius="sm">
      <Stack gap="md">
        <Group justify="space-between" align="flex-start" gap="md">
          <Stack gap={2}>
            <Group gap="xs">
              <Text fw={700}>{title}</Text>
              <StatusBadge status={status} />
            </Group>
            {subtitle ? (
              <Text size="sm" c="dimmed">
                {subtitle}
              </Text>
            ) : null}
          </Stack>
          {action}
        </Group>
        {children}
      </Stack>
    </Paper>
  );
}
