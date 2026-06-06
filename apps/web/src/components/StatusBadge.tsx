import { Badge } from "@mantine/core";
import type { EventStatus } from "../types";

const eventColors: Record<EventStatus, string> = {
  signal: "blue",
  executed: "teal",
  rejected: "red",
  policy: "violet",
  warning: "yellow",
};

const stateColors: Record<string, string> = {
  success: "teal",
  completed: "teal",
  validated: "teal",
  warning: "yellow",
  danger: "red",
  rejected: "red",
  healthy: "teal",
  succeeded: "teal",
  queued: "blue",
  submitting: "blue",
  running: "blue",
  checking: "blue",
  offline: "gray",
  failed: "red",
};

export function StatusBadge({ status, label }: { status: string; label?: string }) {
  const color = eventColors[status as EventStatus] ?? stateColors[status] ?? "gray";

  return (
    <Badge color={color} variant="light" radius="sm">
      {label ?? status.replaceAll("_", " ")}
    </Badge>
  );
}
