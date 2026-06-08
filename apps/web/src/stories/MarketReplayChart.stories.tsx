import type { Meta, StoryObj } from "@storybook/react";
import { Paper, Stack, Text } from "@mantine/core";
import { MarketReplayChart } from "../components/MarketReplayChart";
import { marketReplay } from "../data/mockData";
import type { CandlePoint, MarketEvent, ReplayPoint } from "../types";

const meta = {
  title: "Workbench/MarketReplayChart",
  component: MarketReplayChart,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof MarketReplayChart>;

export default meta;
type Story = StoryObj<typeof meta>;

const hour = 3_600;
const eventTimestamp = Date.UTC(2026, 3, 11, 18, 0, 0) / 1000;
const sameTickCandles: CandlePoint[] = [
  { time: (eventTimestamp - 4 * hour) as CandlePoint["time"], open: 2241.89, high: 2244.04, low: 2240.62, close: 2242.69, volume: 180_000 },
  { time: (eventTimestamp - 3 * hour) as CandlePoint["time"], open: 2242.84, high: 2248.96, low: 2242.82, close: 2248.96, volume: 185_000 },
  { time: (eventTimestamp - 2 * hour) as CandlePoint["time"], open: 2250.42, high: 2261.07, low: 2248.56, close: 2261.07, volume: 188_000 },
  { time: (eventTimestamp - hour) as CandlePoint["time"], open: 2260.81, high: 2261.44, low: 2255.94, close: 2261.44, volume: 190_000 },
  { time: eventTimestamp as CandlePoint["time"], open: 2260.53, high: 2301.95, low: 2259.1, close: 2301.95, volume: 320_000 },
  { time: (eventTimestamp + hour) as CandlePoint["time"], open: 2302.52, high: 2314.14, low: 2302.52, close: 2314.14, volume: 230_000 },
  { time: (eventTimestamp + 2 * hour) as CandlePoint["time"], open: 2314.56, high: 2314.56, low: 2294.42, close: 2301.16, volume: 210_000 },
  { time: (eventTimestamp + 3 * hour) as CandlePoint["time"], open: 2298.67, high: 2301.6, low: 2297.47, close: 2301.6, volume: 205_000 },
  { time: (eventTimestamp + 4 * hour) as CandlePoint["time"], open: 2300.74, high: 2305.51, low: 2292.46, close: 2293.89, volume: 195_000 },
  { time: (eventTimestamp + 5 * hour) as CandlePoint["time"], open: 2296.56, high: 2297.4, low: 2282.56, close: 2285.72, volume: 180_000 },
];
const sameTickReplay: ReplayPoint[] = sameTickCandles.map((candle, index) => ({
  time: candle.time,
  label: `T${index + 1}`,
  equity: 30_000 + index * 45,
  drawdown: -0.2,
  gas: 0,
  funding: 0,
}));
const sameTickEvents: MarketEvent[] = [
  {
    id: "signal-same-tick",
    index: 4,
    time: eventTimestamp as MarketEvent["time"],
    labelTime: "2026-04-11 18:00 UTC",
    kind: "signal_fired",
    label: "Signal Fired",
    node: "eth-above-2300",
    status: "signal",
    price: "$2,301.95",
    observedPrice: 2301.95,
    impact: "-",
  },
  {
    id: "action-same-tick",
    index: 5,
    time: eventTimestamp as MarketEvent["time"],
    labelTime: "2026-04-11 18:00 UTC",
    kind: "action_executed",
    label: "Action Executed",
    node: "sell-eth-on-base-005",
    status: "executed",
    price: "$2,300.22",
    observedPrice: 2300.22,
    impact: "$115.01",
    side: "sell",
    orderType: "swap",
    fillAmount: "0.05 ETH",
    notional: "$115.01",
  },
];

export const CandleReplay: Story = {
  args: {
    candles: marketReplay.candles,
    replay: marketReplay.replay,
    events: marketReplay.events,
    selectedEventId: marketReplay.selectedEventId,
  },
  decorators: [
    (Story) => (
      <Paper p="md" style={{ width: 980, background: "var(--cb-surface)" }}>
        <Stack gap="xs">
          <Text fw={650}>ETH / USDC with volume, equity, and drawdown panes</Text>
          <Story />
        </Stack>
      </Paper>
    ),
  ],
};

export const CompactLensWindow: Story = {
  args: {
    candles: marketReplay.candles,
    replay: marketReplay.replay,
    events: marketReplay.events,
    selectedEventId: "evt-5",
    compact: true,
  },
  decorators: [
    (Story) => (
      <Paper p="md" style={{ width: 620, background: "var(--cb-surface)" }}>
        <Story />
      </Paper>
    ),
  ],
};

export const SameTickSignalAndAction: Story = {
  args: {
    candles: sameTickCandles,
    replay: sameTickReplay,
    events: sameTickEvents,
    selectedEventId: "action-same-tick",
    compact: false,
    granularityMode: "tick",
  },
  decorators: [
    (Story) => (
      <Paper p="md" style={{ width: 760, background: "var(--cb-surface)" }}>
        <Stack gap="xs">
          <Text fw={650}>Same tick signal + action marker proof</Text>
          <Text size="sm" c="dimmed">
            Signal and action both occur at 2026-04-11 18:00 UTC; their x-position must match while their y-position uses each observed price.
          </Text>
          <Story />
        </Stack>
      </Paper>
    ),
  ],
};
