import {
  assumptionsList,
  contextStrip,
  dataTable,
  escapeHtml,
  marketReplayChart,
  progressBar,
  statusPill,
  surface
} from "../design-system/components.js";

function renderTimeline(events, selectedId) {
  return `
    <div class="timeline" aria-label="Audit trace">
      ${events
        .map(
          (event) => `
            <button class="timeline-item" data-event-id="${event.id}" aria-pressed="${event.id === selectedId}">
              <span class="timeline-time">${event.time}</span>
              <span class="event-dot ${event.status}" aria-hidden="true"></span>
              <span>
                <span class="event-kind">${event.kind}</span>
                <span class="event-meta">${event.node}</span>
              </span>
              <span class="event-node">${event.venue}</span>
            </button>
          `
        )
        .join("")}
    </div>
  `;
}

function balances(title, rows) {
  return `
    <div>
      <div class="surface-note">${title}</div>
      ${rows
        .map(
          (row) => `
            <div class="balance-row">
              <strong>${row.asset}</strong>
              <div class="balance-bar"><span style="--value: ${row.percent}%"></span></div>
              <span>${row.value}</span>
            </div>
          `
        )
        .join("")}
    </div>
  `;
}

function policyMatrix(rows) {
  return `
    <div class="matrix-scroll" tabindex="0" aria-label="Policy profile comparison">
      <table class="policy-matrix">
        <thead>
          <tr>
            <th>Category</th>
            <th class="selected-col">Strict v1</th>
            <th>Conservative v1</th>
            <th>Research v1</th>
          </tr>
        </thead>
        <tbody>
          ${rows
            .map(
              (row) => `
                <tr>
                  <td>${escapeHtml(row[0])}</td>
                  <td class="selected-col">${escapeHtml(row[1])}</td>
                  <td>${escapeHtml(row[2])}</td>
                  <td>${escapeHtml(row[3])}</td>
                </tr>
              `
            )
            .join("")}
        </tbody>
      </table>
    </div>
  `;
}

function nodeFilters(graph, selectedNode) {
  return `
    <div class="event-overview-list">
      ${graph.nodes
        .map(
          (node) => `
            <button class="node-filter ${node.id === selectedNode ? "selected" : ""}">
              <span class="node-filter-title">
                <span>${escapeHtml(node.label)}</span>
                ${statusPill(node.kind, node.kind === "action" ? "success" : node.kind === "data" ? "info" : "warning")}
              </span>
              <span class="event-meta">${escapeHtml(node.id)}</span>
              <span>${escapeHtml(node.detail)}</span>
            </button>
          `
        )
        .join("")}
    </div>
  `;
}

function graphPath() {
  return `
    <div class="graph-path" aria-label="Graph path around selected event">
      <div class="path-node">
        <span class="data-chip">Signal</span>
        <strong>signal-eth-breakdown</strong>
        <span class="event-meta">ETH breaks below threshold</span>
      </div>
      <span class="path-arrow">-></span>
      <div class="path-node">
        <span class="data-chip">Action</span>
        <strong>buy-eth-on-base</strong>
        <span class="event-meta">Execute market buy</span>
      </div>
      <span class="path-arrow">-></span>
      <div class="path-node">
        <span class="data-chip">Position</span>
        <strong>long-eth</strong>
        <span class="event-meta">3.3442 ETH</span>
      </div>
    </div>
  `;
}

function evidenceGrid(rows) {
  return `
    <div class="lens-evidence">
      ${rows
        .map(
          ([label, value]) => `
            <div class="evidence-cell">
              <span>${escapeHtml(label)}</span>
              <strong>${escapeHtml(value)}</strong>
            </div>
          `
        )
        .join("")}
    </div>
  `;
}

export function renderExplainAudit({ graph, setup, result, audit, marketReplay }) {
  const selected = audit.selected;
  const detailBody = `
    <div class="event-detail">
      <div>
        ${statusPill(selected.kind, "success")}
        <h2 class="surface-title" style="margin-top: var(--space-3);">${selected.node}</h2>
        <p class="surface-note">${selected.explanation}</p>
      </div>

      ${assumptionsList([
        ["Instrument", selected.instrument],
        ["Side", selected.side],
        ["Leverage", selected.leverage],
        ["Order type", selected.orderType],
        ["Venue", selected.venue]
      ])}

      <div class="before-after">
        ${balances("Before", selected.before)}
        ${balances("After", selected.after)}
      </div>

      ${assumptionsList(selected.pricing)}

      <pre class="code-preview">${escapeHtml(JSON.stringify(selected.raw, null, 2))}</pre>
    </div>
  `;

  const costBody = `
    <div class="coverage-stack">
      ${result.costs
        .map(
          (cost) => `
            <div class="coverage-row">
              <span>${cost.label}</span>
              <div>${progressBar(cost.percent, cost.tone === "positive" ? "var(--success)" : "var(--danger)")}</div>
              <strong class="${cost.tone}">${cost.value}</strong>
            </div>
          `
        )
        .join("")}
    </div>
  `;

  const rejectedBody = dataTable({
    columns: [
      { key: "time", label: "Time" },
      { key: "node", label: "Node" },
      { key: "action", label: "Action" },
      { key: "reason", label: "Policy reason" }
    ],
    rows: audit.rejected,
    className: "timeline-table"
  });

  const lensBody = `
    <div class="market-lens">
      ${graphPath()}
      ${marketReplayChart({
        candles: marketReplay.candles,
        equity: marketReplay.equity,
        drawdown: marketReplay.drawdown,
        events: marketReplay.events,
        selectedEventId: marketReplay.selectedEventId,
        compact: true
      })}
      ${evidenceGrid(marketReplay.evidence)}
    </div>
  `;

  return `
    <div class="page">
      ${contextStrip({
        eyebrow: "Event Lens",
        title: graph.name,
        pills: [
          { label: "Strict v1", tone: "info" },
          { label: "Market data window", tone: "success" }
        ],
        meta: [`Run ${setup.runId}`, "Selected 2024-05-14 14:30 UTC", "Data coverage 98.6%"],
        stats: [
          { label: "Fill", value: "$2,988.40" },
          { label: "Gas", value: "$0.18", tone: "negative" },
          { label: "Fees", value: "$6.64", tone: "negative" }
        ]
      })}

      <div class="event-lens-grid">
        ${surface({ title: "Graph event filter", note: "Choose the node or action family to analyze.", body: nodeFilters(graph, "buy-eth-on-base") })}
        ${surface({ title: "Market data lens", note: "Selected time window with price, equity, drawdown, and event markers.", body: lensBody })}
        ${surface({ title: "Selected event analysis", note: "Why it fired, what data it read, and what changed.", body: detailBody })}
      </div>

      <div class="results-grid">
        ${surface({ title: "Assumptions inspector", note: "Profile differences that affect event behavior.", body: policyMatrix(audit.policyMatrix) })}
        <div class="page">
          ${surface({ title: "Cost waterfall", note: "Gas, fees, slippage, funding, and net effect.", body: costBody })}
          ${surface({ title: "Rejected actions", note: "Policy reasons remain visible.", body: rejectedBody })}
        </div>
      </div>
    </div>
  `;
}
