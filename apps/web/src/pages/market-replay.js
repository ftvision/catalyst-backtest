import {
  button,
  contextStrip,
  dataTable,
  escapeHtml,
  marketReplayChart,
  statusPill,
  surface
} from "../design-system/components.js";

function eventOverview(events, selectedEventId) {
  return `
    <div class="event-overview-list">
      ${events
        .map(
          (event) => `
            <button class="event-overview-item ${event.id === selectedEventId ? "selected" : ""}" data-event-id="${event.id}">
              <span class="event-overview-title">
                <span>${String(event.index).padStart(2, "0")} ${escapeHtml(event.kind)}</span>
                ${statusPill(event.status, event.status === "executed" ? "success" : event.status === "rejected" ? "danger" : event.status === "warning" ? "warning" : "info")}
              </span>
              <span class="event-meta">${escapeHtml(event.time)} | ${escapeHtml(event.node)}</span>
              <span>${escapeHtml(event.label)}</span>
            </button>
          `
        )
        .join("")}
    </div>
  `;
}

export function renderMarketReplay({ graph, setup, result, marketReplay }) {
  const selected = marketReplay.events.find((event) => event.id === marketReplay.selectedEventId) || marketReplay.events[0];

  const chartBody = `
    <div class="meta-row" style="margin-bottom: var(--space-4);">
      <span>${escapeHtml(marketReplay.symbol)}</span>
      <span>${escapeHtml(marketReplay.venue)}</span>
      <span>${escapeHtml(marketReplay.period)}</span>
      <span>Events overlay on</span>
    </div>
    ${marketReplayChart({
      candles: marketReplay.candles,
      equity: marketReplay.equity,
      drawdown: marketReplay.drawdown,
      events: marketReplay.events,
      selectedEventId: marketReplay.selectedEventId
    })}
  `;

  const selectedBody = `
    <div class="event-detail">
      <div>
        ${statusPill(selected.kind, selected.status === "rejected" ? "danger" : selected.status === "warning" ? "warning" : "success")}
        <h2 class="surface-title" style="margin-top: var(--space-3);">${escapeHtml(selected.label)}</h2>
        <p class="surface-note">${escapeHtml(selected.time)} | ${escapeHtml(selected.node)}</p>
      </div>
      <div class="assumption-grid">
        <div class="assumption-row"><span class="assumption-label">Market price</span><strong>${escapeHtml(selected.price)}</strong></div>
        <div class="assumption-row"><span class="assumption-label">Impact</span><strong>${escapeHtml(selected.impact)}</strong></div>
        <div class="assumption-row"><span class="assumption-label">Policy</span><strong>Strict v1</strong></div>
        <div class="assumption-row"><span class="assumption-label">Coverage</span><strong>98.6%</strong></div>
      </div>
      <button class="button primary" data-route="lens">Open in Event Lens</button>
    </div>
  `;

  const eventTable = dataTable({
    columns: [
      { key: "time", label: "Time" },
      { key: "kind", label: "Type" },
      { key: "node", label: "Node" },
      { key: "label", label: "Event" },
      { key: "price", label: "Price" },
      { key: "impact", label: "Impact" }
    ],
    rows: marketReplay.events,
    className: "timeline-table"
  });

  return `
    <div class="page">
      ${contextStrip({
        eyebrow: "Market Replay",
        title: graph.name,
        pills: [
          { label: "Historical", tone: "info" },
          { label: "Events overlay", tone: "success" }
        ],
        meta: [`Run ${setup.runId}`, marketReplay.period, "5 event classes"],
        stats: [
          { label: "Final equity", value: "$112,842", tone: "positive" },
          { label: "Max DD", value: "-6.21%", tone: "negative" },
          { label: "Coverage", value: "98.6%", tone: "positive" }
        ]
      })}

      <div class="replay-overview-grid">
        ${surface({ title: "Market data, equity, drawdown", note: "Overview lane for historical data and simulated actions.", body: chartBody })}
        <div class="page">
          ${surface({ title: "Selected event", note: "Overview detail for the active marker.", body: selectedBody })}
          ${surface({ title: "Events overview", note: "Signals, executions, rejections, funding, and costs.", body: eventOverview(marketReplay.events, marketReplay.selectedEventId) })}
        </div>
      </div>

      ${surface({
        title: "Chronological events",
        note: "This is the overview list. Deep analysis belongs in Event Lens.",
        action: button("Filter events", { variant: "ghost" }),
        body: eventTable
      })}
    </div>
  `;
}
