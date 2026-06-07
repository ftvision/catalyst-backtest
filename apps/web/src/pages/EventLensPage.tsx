import {
  Alert,
  Group,
  Paper,
  Progress,
  SimpleGrid,
  Stack,
  Table,
  Text,
  Title,
} from "@mantine/core";
import { AlertTriangle } from "lucide-react";
import { MarketReplayChart } from "../components/MarketReplayChart";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";
import type { AuditData, MarketEvent, MarketReplayData, SetupData } from "../types";

export function EventLensPage({
  audit,
  replay,
  setup,
  selectedEventId,
  selectedReplayEvent,
  onSelectEvent,
}: {
  audit: AuditData;
  replay: MarketReplayData;
  setup: SetupData;
  selectedEventId: string;
  selectedReplayEvent?: MarketEvent;
  onSelectEvent: (eventId: string) => void;
}) {
  const lensEvent = selectedReplayEvent ?? replay.events.find((event) => event.id === selectedEventId);
  const eventTitle = lensEvent?.label ?? audit.selected.kind.replaceAll("_", " ");
  const eventStatus = lensEvent?.status ?? "executed";
  const eventTime = lensEvent?.labelTime ?? audit.selected.raw.timestamp;

  return (
    <Stack gap="md">
      <SectionHeader
        title="Event Lens"
        subtitle="Explain one event with market evidence, pricing, balances, and policy reasons."
      />

      <div className="event-grid">
        <Paper className="panel" p="md" radius="sm">
          <Stack gap="sm">
            <Text fw={650}>Trace events</Text>
            {audit.events.map((event) => (
              <button
                key={event.id}
                type="button"
                className="event-row"
                aria-pressed={selectedEventId === event.id}
                onClick={() => onSelectEvent(event.id)}
              >
                <Group justify="space-between" gap="xs">
                  <Text size="sm" fw={650}>
                    {event.time}
                  </Text>
                  <StatusBadge status={event.status} />
                </Group>
                <Text size="xs" c="dimmed">
                  {event.kind} / {event.venue}
                </Text>
                <Text size="xs" className="mono">
                  {event.node}
                </Text>
              </button>
            ))}
          </Stack>
        </Paper>

        <Stack gap="md">
          <Paper className="panel" p="md" radius="sm">
            <Stack gap="sm">
              <Group justify="space-between" align="flex-start">
                <Stack gap={2}>
                  <Title order={2}>{eventTitle}</Title>
                  <Text size="sm" c="dimmed">
                    {eventTime} / {audit.selected.venue} / {audit.selected.instrument}
                  </Text>
                </Stack>
                <StatusBadge status={eventStatus} />
              </Group>
              <Text>
                {lensEvent
                  ? `${lensEvent.label} is selected from the replay timeline. The lens keeps pricing, balances, costs, and policy reasons beside the exact market window.`
                  : audit.selected.explanation}
              </Text>
              <SimpleGrid cols={{ base: 2, sm: 4 }} spacing="xs">
                {[
                  ["Replay event", lensEvent ? String(lensEvent.index) : audit.selected.kind],
                  ["Side", audit.selected.side],
                  ["Order", audit.selected.orderType],
                  ["Policy", setup.policy],
                ].map(([label, value]) => (
                  <Paper key={label} className="panel-muted" p="xs">
                    <Text size="xs" c="dimmed">
                      {label}
                    </Text>
                    <Text fw={650}>{value}</Text>
                  </Paper>
                ))}
              </SimpleGrid>
            </Stack>
          </Paper>

          <SimpleGrid cols={{ base: 1, xl: 2 }} spacing="md">
            <Paper className="panel" p="md" radius="sm">
              <Stack gap="sm">
                <Text fw={650}>Local market window</Text>
                <MarketReplayChart
                  candles={replay.candles}
                  replay={replay.replay}
                  events={replay.events}
                  selectedEventId={selectedEventId}
                  compact
                />
              </Stack>
            </Paper>

            <Paper className="panel" p="md" radius="sm">
              <Stack gap="sm">
                <Text fw={650}>Event impact</Text>
                <SimpleGrid cols={{ base: 2, sm: 3 }} spacing="xs">
                  {[
                    ["Status", eventStatus],
                    ["Observed price", lensEvent?.price ?? "-"],
                    ["Impact", lensEvent?.impact ?? audit.selected.raw.status ?? "-"],
                    ...audit.selected.pricing.slice(0, 3),
                  ].map(([label, value]) => (
                    <Paper key={label} className="panel-muted" p="xs">
                      <Text size="xs" c="dimmed">
                        {label}
                      </Text>
                      <Text size="sm" fw={650} className="mono">
                        {value}
                      </Text>
                    </Paper>
                  ))}
                </SimpleGrid>
              </Stack>
            </Paper>
          </SimpleGrid>

          <SimpleGrid cols={{ base: 1, lg: 2 }} spacing="md">
            <Paper className="panel" p="md" radius="sm">
              <Stack gap="sm">
                <Text fw={650}>Portfolio before</Text>
                {audit.selected.before.map((asset) => (
                  <Stack key={asset.asset} gap={4}>
                    <Group justify="space-between">
                      <Text size="sm">{asset.asset}</Text>
                      <Text size="sm" className="mono">
                        {asset.value}
                      </Text>
                    </Group>
                    <Progress value={asset.percent} size="sm" color="gray" />
                  </Stack>
                ))}
              </Stack>
            </Paper>

            <Paper className="panel" p="md" radius="sm">
              <Stack gap="sm">
                <Text fw={650}>Portfolio after</Text>
                {audit.selected.after.map((asset) => (
                  <Stack key={asset.asset} gap={4}>
                    <Group justify="space-between">
                      <Text size="sm">{asset.asset}</Text>
                      <Text size="sm" className="mono">
                        {asset.value}
                      </Text>
                    </Group>
                    <Progress value={asset.percent} size="sm" color="teal" />
                  </Stack>
                ))}
              </Stack>
            </Paper>
          </SimpleGrid>

          <Paper className="panel" p="md" radius="sm">
            <Stack gap="sm">
              <Text fw={650}>Pricing context</Text>
              <Table withTableBorder>
                <Table.Tbody>
                  {audit.selected.pricing.map(([label, value]) => (
                    <Table.Tr key={label}>
                      <Table.Td>{label}</Table.Td>
                      <Table.Td className="mono">{value}</Table.Td>
                    </Table.Tr>
                  ))}
                </Table.Tbody>
              </Table>
            </Stack>
          </Paper>

          <Alert color="red" icon={<AlertTriangle size={16} />} title="Rejected actions stay visible">
            {audit.rejected.map((item) => `${item.time} ${item.action}: ${item.reason}`).join(" ")}
          </Alert>
        </Stack>
      </div>
    </Stack>
  );
}
