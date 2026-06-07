import { Group, Stack, Text, Title } from "@mantine/core";
import type { ReactNode } from "react";

export function SectionHeader({
  title,
  subtitle,
  action,
}: {
  title: string;
  subtitle?: string;
  action?: ReactNode;
}) {
  return (
    <Group className="section-header" justify="space-between" align="flex-start" gap="md">
      <Stack gap={2}>
        <Title order={2}>{title}</Title>
        {subtitle ? (
          <Text size="sm" c="dimmed">
            {subtitle}
          </Text>
        ) : null}
      </Stack>
      {action}
    </Group>
  );
}
