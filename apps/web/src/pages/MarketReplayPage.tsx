import { Button, Group, Paper, SimpleGrid, Stack, Text } from "@mantine/core";
import { ArrowRight, ScanLine } from "lucide-react";
import { DataTable } from "../components/DataTable";
import { MarketReplayChart } from "../components/MarketReplayChart";
import { SectionHeader } from "../components/SectionHeader";
import { StatusBadge } from "../components/StatusBadge";
import { MarketEvidencePanel } from "../components/TrendPanel";
import type { GraphSummary, MarketReplayData, ResultData, SetupData } from "../types";

export function MarketReplayPage({
  graph,
  setup,
  result,
  replay,
  selectedEventId,
  onSelectEvent,
  onInspectEvent,
}: {
  graph: GraphSummary;
  setup: SetupData;
  result: ResultData;
  replay: MarketReplayData;
  selectedEventId: string;
  onSelectEvent: (eventId: string) => void;
  onInspectEvent: () => void;
}) {
  const selectedEvent = replay.events.find((event) => event.id === selectedEventId) ?? replay.events[0];

  return (
    <Stack gap="md">
      <SectionHeader
        title="Market Replay"
        subtitle="Historical candles, equity, drawdown, gas, funding, and trace events in one overview."
        action={
          <Button variant="light" leftSection={<ScanLine size={14} />} onClick={onInspectEvent}>
            Inspect in Event Lens
          </Button>
        }
      />

      <div className="section-grid">
        <Paper className="panel" p="md" radius="sm">
          <Stack gap="sm">
            <Group justify="space-between" align="flex-start">
              <Stack gap={2}>
                <Text fw={650}>{replay.symbol}</Text>
                <Text size="sm" c="dimmed">
                  {replay.venue} / {replay.period} / policy {setup.policy}
                </Text>
              </Stack>
              <Group gap="xs">
                <StatusBadge status={result.status} />
                <Text size="xs" c="dimmed" className="mono">
                  {graph.hash}
                </Text>
              </Group>
            </Group>

            <MarketReplayChart
              candles={replay.candles}
              replay={replay.replay}
              events={replay.events}
              selectedEventId={selectedEventId}
            />
          </Stack>
        </Paper>

        <Paper className="panel" p="md" radius="sm">
          <Stack gap="sm">
            <Stack gap={2}>
              <Text fw={650}>Event overview</Text>
              <Text size="xs" c="dimmed">
                Select an event to keep chart, evidence, and audit context aligned.
              </Text>
            </Stack>
            <Paper className="selected-event-panel" p="sm" radius="sm">
              <Stack gap={6}>
                <Group justify="space-between" gap="xs">
                  <Text size="sm" fw={650}>
                    {selectedEvent.index}. {selectedEvent.label}
                  </Text>
                  <StatusBadge status={selectedEvent.status} />
                </Group>
                <Text size="xs" c="dimmed">
                  {selectedEvent.labelTime} / {selectedEvent.node}
                </Text>
                <Text size="xs" className="mono">
                  {selectedEvent.price} / {selectedEvent.impact}
                </Text>
                <Button
                  variant="subtle"
                  size="xs"
                  rightSection={<ArrowRight size={14} />}
                  onClick={onInspectEvent}
                >
                  Inspect costs and policy
                </Button>
              </Stack>
            </Paper>
            <Text size="xs" c="dimmed">
              Nearby events
            </Text>
            {replay.events.filter((event) => event.id !== selectedEvent.id).map((event) => (
              <button
                key={event.id}
                className="event-row"
                type="button"
                aria-pressed={selectedEventId === event.id}
                onClick={() => onSelectEvent(event.id)}
              >
                <Group justify="space-between" gap="xs">
                  <Text size="sm" fw={650}>
                    {event.index}. {event.label}
                  </Text>
                  <StatusBadge status={event.status} />
                </Group>
                <Text size="xs" c="dimmed">
                  {event.labelTime} / {event.node}
                </Text>
                <Text size="xs" className="mono">
                  {event.price} / {event.impact}
                </Text>
              </button>
            ))}
          </Stack>
        </Paper>
      </div>

      <SimpleGrid cols={{ base: 1 }} spacing="md">
        <MarketEvidencePanel data={replay.replay} />
      </SimpleGrid>

      <Paper className="panel" p="md" radius="sm">
        <Stack gap="sm">
          <Stack gap={2}>
            <Text fw={650}>Selected market evidence</Text>
            <Text size="xs" c="dimmed">
              Evidence window for {selectedEvent.label} at {selectedEvent.labelTime}.
            </Text>
          </Stack>
          <DataTable
            columns={["Evidence", "Value"]}
            rows={replay.evidence.map(([label, value]) => [label, <span className="mono">{value}</span>])}
          />
        </Stack>
      </Paper>
    </Stack>
  );
}
