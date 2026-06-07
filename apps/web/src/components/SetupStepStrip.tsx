import { Group, Paper, Stack, Text } from "@mantine/core";
import { CheckCircle2, CircleAlert, CircleX } from "lucide-react";
import { StatusBadge } from "./StatusBadge";

export interface SetupStep {
  id: string;
  label: string;
  detail: string;
  status: "success" | "warning" | "danger";
}

const iconByStatus = {
  success: CheckCircle2,
  warning: CircleAlert,
  danger: CircleX,
};

export function SetupStepStrip({ steps }: { steps: SetupStep[] }) {
  return (
    <div className="setup-step-strip">
      {steps.map((step) => {
        const Icon = iconByStatus[step.status];
        return (
          <Paper key={step.id} className="setup-step" p="sm" radius="sm">
            <Group justify="space-between" align="flex-start" gap="sm">
              <Stack gap={2}>
                <Group gap="xs">
                  <Icon size={15} className={`setup-step-icon ${step.status}`} />
                  <Text size="sm" fw={700}>
                    {step.label}
                  </Text>
                </Group>
                <Text size="xs" c="dimmed">
                  {step.detail}
                </Text>
              </Stack>
              <StatusBadge status={step.status} />
            </Group>
          </Paper>
        );
      })}
    </div>
  );
}
