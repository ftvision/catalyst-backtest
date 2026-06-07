import type { Meta, StoryObj } from "@storybook/react";
import { useState } from "react";
import { Paper } from "@mantine/core";
import { App } from "../App";
import { audit, graph, marketReplay, result, runHistory, setup } from "../data/mockData";
import { EventLensPage } from "../pages/EventLensPage";
import { MarketReplayPage } from "../pages/MarketReplayPage";
import { ResultReviewPage } from "../pages/ResultReviewPage";
import { RunSetupPage } from "../pages/RunSetupPage";

const meta = {
  title: "Workbench/Pages",
  parameters: {
    viewport: {
      defaultViewport: "desktop",
    },
  },
} satisfies Meta;

export default meta;
type Story = StoryObj;

function StoryCanvas({ children }: { children: React.ReactNode }) {
  return (
    <Paper p="lg" radius={0} style={{ minHeight: "100vh", background: "var(--cb-surface)" }}>
      {children}
    </Paper>
  );
}

export const FullWorkbench: Story = {
  render: () => <App />,
};

export const RunSetup: Story = {
  render: () => (
    <StoryCanvas>
      <RunSetupPage graph={graph} setup={setup} runHistory={runHistory} onRun={() => undefined} />
    </StoryCanvas>
  ),
};

export const MarketReplayOverview: Story = {
  render: () => {
    const [selectedEventId, setSelectedEventId] = useState(marketReplay.selectedEventId);

    return (
      <StoryCanvas>
        <MarketReplayPage
          graph={graph}
          setup={setup}
          result={result}
          replay={marketReplay}
          selectedEventId={selectedEventId}
          onSelectEvent={setSelectedEventId}
          onInspectEvent={() => undefined}
        />
      </StoryCanvas>
    );
  },
};

export const EventLensDetail: Story = {
  render: () => {
    const [selectedEventId, setSelectedEventId] = useState(audit.selectedEventId);

    return (
      <StoryCanvas>
        <EventLensPage
          audit={audit}
          replay={marketReplay}
          setup={setup}
          selectedEventId={selectedEventId}
          onSelectEvent={setSelectedEventId}
        />
      </StoryCanvas>
    );
  },
};

export const ResultReview: Story = {
  render: () => (
    <StoryCanvas>
      <ResultReviewPage graph={graph} setup={setup} result={result} />
    </StoryCanvas>
  ),
};
