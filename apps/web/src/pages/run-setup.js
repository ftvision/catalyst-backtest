import {
  assumptionsList,
  button,
  contextStrip,
  coverageRows,
  dataTable,
  emptyState,
  statusPill,
  surface
} from "../design-system/components.js";

export function renderRunSetup({ graph, setup, runHistory }) {
  const graphBody = `
    <div class="graph-flow">
      ${graph.nodes
        .slice(0, 4)
        .map(
          (node, index) => `
            <div class="flow-node ${node.kind}">
              <span class="flow-index">${index + 1}</span>
              <div>
                <h3>${node.label}</h3>
                <p>${node.id} | ${node.detail}</p>
              </div>
            </div>
          `
        )
        .join("")}
    </div>
    <div class="compact-list">
      ${graph.nodes
        .map(
          (node) => `
            <div class="compact-row">
              <span>${node.label}</span>
              <span class="muted">${node.detail}</span>
            </div>
          `
        )
        .join("")}
    </div>
  `;

  const configBody = `
    <div class="form-stack">
      <div class="form-section">
        <h3>Time range</h3>
        <div class="field-grid">
          <div class="field">
            <label for="start-time">Start</label>
            <input class="input" id="start-time" value="${setup.start}" />
          </div>
          <div class="field">
            <label for="end-time">End</label>
            <input class="input" id="end-time" value="${setup.end}" />
          </div>
        </div>
        <div class="field">
          <label for="interval">Interval</label>
          <select class="select" id="interval">
            <option selected>1h</option>
            <option>4h</option>
            <option>1d</option>
          </select>
        </div>
      </div>

      <div class="form-section">
        <h3>Initial portfolio</h3>
        ${dataTable({
          columns: [
            { key: "venue", label: "Venue" },
            { key: "asset", label: "Asset" },
            { key: "amount", label: "Amount" },
            { key: "percent", label: "% total" }
          ],
          rows: setup.portfolio,
          className: "portfolio-table"
        })}
      </div>

      <div class="form-section">
        <h3>Policy profile</h3>
        <div class="segment" role="group" aria-label="Policy profile">
          <button aria-pressed="true" data-policy="strict_v1">Strict v1</button>
          <button aria-pressed="false" data-policy="conservative_v1">Conservative v1</button>
          <button aria-pressed="false" data-policy="research_v1">Research v1</button>
        </div>
      </div>
    </div>
  `;

  const dataBody = `
    ${coverageRows(setup.coverage)}
    <div class="section-kicker">Warnings</div>
    <div class="compact-list">
      ${setup.warnings
        .map(
          (warning) => `
            <div class="compact-row">
              <span>${warning}</span>
              ${statusPill("Review", "warning")}
            </div>
          `
        )
        .join("")}
    </div>
    <div class="section-kicker">Resolved assumptions</div>
    ${assumptionsList(setup.assumptions)}
  `;

  const historyBody = `
    <div class="history-layout">
      <div>
        ${runHistory.length
          ? dataTable({
              columns: [
                { key: "id", label: "Run ID" },
                {
                  key: "status",
                  label: "Status",
                  render: (row) => statusPill(row.status === "success" ? "Success" : row.status === "warning" ? "Warnings" : "Failed", row.status)
                },
                { key: "policy", label: "Policy" },
                { key: "range", label: "Range" },
                { key: "interval", label: "Interval" },
                { key: "duration", label: "Duration" },
                {
                  key: "returnUsd",
                  label: "Return",
                  render: (row) => `<span class="${row.returnUsd.startsWith("+") ? "positive" : "muted"}">${row.returnUsd}</span>`
                }
              ],
              rows: runHistory,
              className: "timeline-table"
            })
          : emptyState("No runs yet", "Run this graph once to create history.")}
      </div>
      <aside class="run-panel" aria-label="Run actions">
        ${button("Run backtest", { variant: "primary", icon: "icon-play", id: "run-backtest" })}
        ${button("Save setup")}
        ${button("Export setup JSON", { variant: "ghost" })}
      </aside>
    </div>
  `;

  return `
    <div class="page">
      ${contextStrip({
        eyebrow: "Run Setup",
        title: `Graph: ${graph.name}`,
        pills: [
          { label: "Validated", tone: "success" },
          { label: "API ready", tone: "info" }
        ],
        meta: [`Hash ${graph.hash}`, `Version ${graph.version}`, `Updated ${graph.updatedAt}`],
        stats: [
          { label: "Nodes", value: String(graph.nodeCount) },
          { label: "Edges", value: String(graph.edgeCount) },
          { label: "Last run", value: "Success", tone: "positive" }
        ]
      })}

      <div class="workspace-grid">
        ${surface({ title: "Graph summary", note: "Compact read-only graph context.", body: graphBody })}
        ${surface({ title: "Run configuration", note: "Range, interval, portfolio, and policy.", body: configBody })}
        ${surface({ title: "Data and assumptions", note: "Coverage is checked before the run.", body: dataBody })}
      </div>

      ${surface({ title: "Run history", note: "Recent runs for this graph.", body: historyBody })}
    </div>
  `;
}
