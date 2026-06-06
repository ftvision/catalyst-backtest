import type { Meta, StoryObj } from "@storybook/react";
import { Paper, Stack, Text } from "@mantine/core";
import { MarketReplayChart } from "../components/MarketReplayChart";
import { marketReplay } from "../data/mockData";

const meta = {
  title: "Workbench/MarketReplayChart",
  component: MarketReplayChart,
  parameters: {
    layout: "centered",
  },
} satisfies Meta<typeof MarketReplayChart>;

export default meta;
type Story = StoryObj<typeof meta>;

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
