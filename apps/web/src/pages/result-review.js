import {
  assumptionsList,
  button,
  contextStrip,
  dataTable,
  lineChart,
  metricStrip,
  portfolioTable,
  progressBar,
  statusPill,
  surface
} from "../design-system/components.js";

export function renderResultReview({ graph, setup, result }) {
  const chartBody = `
    <div class="surface-note">Equity curve</div>
    ${lineChart(result.equity, { kind: "equity", label: "Equity curve" })}
    <div class="surface-note">Drawdown</div>
    ${lineChart(result.drawdown, { kind: "drawdown", label: "Drawdown curve" })}
  `;

  const portfolioBody = `
    ${result.portfolio
      .map(
        (group) => `
          <div class="portfolio-group">
            <div class="group-title">
              <span>${group.venue}</span>
              <span>${group.total}</span>
            </div>
            ${portfolioTable(group.assets)}
          </div>
        `
      )
      .join("")}
  `;

  const assumptionsBody = assumptionsList([
    ["Profile", "Strict v1"],
    ["Execution", "Close price, market order"],
    ["Slippage", "0.05% per trade"],
    ["Funding", "Historical 8h"],
    ["Idle cash", "USDC 4.00% APR"],
    ["Data", "missing_required fail"]
  ]);

  const coverageBody = `
    <div class="coverage-row">
      <span class="data-chip">Base</span>
      <div>
        <strong>100%</strong>
        <div class="event-meta">2024-01-01 00:00 to 2024-06-01 00:00</div>
      </div>
      ${progressBar(100)}
    </div>
    <div class="coverage-row">
      <span class="data-chip">Hyperliquid</span>
      <div>
        <strong>98.6%</strong>
        <div class="event-meta">Funding sparse before February</div>
      </div>
      ${progressBar(98.6, "var(--warning)")}
    </div>
  `;

  const timelineBody = dataTable({
    columns: [
      { key: "time", label: "Time" },
      { key: "node", label: "Node ID" },
      { key: "signal", label: "Signal" },
      { key: "action", label: "Action" },
      { key: "venue", label: "Venue" },
      { key: "fees", label: "Fees" },
      { key: "gas", label: "Gas" },
      {
        key: "pnl",
        label: "PnL",
        render: (row) => `<span class="${row.pnl.startsWith("-") ? "negative" : row.pnl.startsWith("$") || row.pnl.startsWith("+") ? "positive" : "muted"}">${row.pnl}</span>`
      }
    ],
    rows: result.timeline,
    className: "timeline-table"
  });

  return `
    <div class="page">
      ${contextStrip({
        eyebrow: "Result Review",
        title: graph.name,
        pills: [
          { label: "Completed", tone: "success" },
          { label: "Strict v1", tone: "info" }
        ],
        meta: [`Run ${setup.runId}`, result.createdAt, `Graph hash ${graph.hash}`],
        stats: [
          { label: "Coverage", value: "98.6%", tone: "positive" },
          { label: "Warnings", value: "3", tone: "negative" }
        ]
      })}

      ${metricStrip(result.metrics)}

      <div class="results-grid">
        ${surface({ title: "Performance", note: "Outcome first, caveats visible.", body: chartBody })}
        <div class="page">
          ${surface({ title: "Final portfolio", note: "Balances by venue.", body: portfolioBody })}
          ${surface({ title: "Resolved assumptions", action: button("View policy", { variant: "ghost" }), body: assumptionsBody })}
          ${surface({ title: "Data coverage", note: "Provider coverage carried through from the run.", body: coverageBody })}
        </div>
      </div>

      ${surface({
        title: "Recent timeline",
        note: "Signals, executed actions, rejected actions, and costs.",
        action: button("Open audit", { variant: "ghost" }),
        body: timelineBody
      })}
    </div>
  `;
}
