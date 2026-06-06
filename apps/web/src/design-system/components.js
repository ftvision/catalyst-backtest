export function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

export function statusPill(label, tone = "info") {
  return `<span class="status-pill ${tone}">${escapeHtml(label)}</span>`;
}

export function button(label, { variant = "", icon = "", disabled = false, id = "" } = {}) {
  const iconHtml = icon ? `<span class="button-icon ${icon}" aria-hidden="true"></span>` : "";
  const idAttr = id ? ` id="${escapeHtml(id)}"` : "";
  return `
    <button class="button ${variant}"${idAttr}${disabled ? " disabled" : ""}>
      ${iconHtml}
      <span>${escapeHtml(label)}</span>
    </button>
  `;
}

export function surface({ title, note = "", action = "", body = "" }) {
  return `
    <section class="surface" aria-label="${escapeHtml(title)}">
      <div class="surface-header">
        <div>
          <h2 class="surface-title">${escapeHtml(title)}</h2>
          ${note ? `<p class="surface-note">${escapeHtml(note)}</p>` : ""}
        </div>
        ${action}
      </div>
      <div class="surface-body">${body}</div>
    </section>
  `;
}

export function contextStrip({ eyebrow, title, pills = [], meta = [], stats = [] }) {
  const pillHtml = pills.map((pill) => statusPill(pill.label, pill.tone)).join("");
  const metaHtml = meta.map((item) => `<span>${escapeHtml(item)}</span>`).join("");
  const statHtml = stats
    .map(
      (stat) => `
        <div>
          <div class="eyebrow">${escapeHtml(stat.label)}</div>
          <strong class="${stat.tone || ""}">${escapeHtml(stat.value)}</strong>
        </div>
      `
    )
    .join("");

  return `
    <section class="context-strip" aria-label="${escapeHtml(title)} context">
      <div class="context-main">
        <div class="eyebrow">${escapeHtml(eyebrow)}</div>
        <div class="title-row">
          <h2>${escapeHtml(title)}</h2>
          ${pillHtml}
        </div>
        <div class="meta-row">${metaHtml}</div>
      </div>
      ${statHtml}
    </section>
  `;
}

export function metricStrip(metrics) {
  return `
    <section class="metric-strip" aria-label="Backtest metrics">
      ${metrics
        .map(
          (metric) => `
            <div class="metric">
              <div class="metric-label">${escapeHtml(metric.label)}</div>
              <div class="metric-value ${metric.tone || ""}">${escapeHtml(metric.value)}</div>
              <div class="metric-detail">${escapeHtml(metric.detail)}</div>
            </div>
          `
        )
        .join("")}
    </section>
  `;
}

export function progressBar(value, tone = "var(--success)") {
  const safeValue = Math.max(0, Math.min(100, Number(value)));
  return `
    <div class="progress-track" role="meter" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${safeValue}">
      <div class="progress-fill" style="--value: ${safeValue}%; --tone: ${tone};"></div>
    </div>
  `;
}

export function coverageRows(rows) {
  return `
    <div class="coverage-stack">
      <div class="coverage-overview">
        <div class="group-title">
          <span>Overall data coverage</span>
          <span class="positive">90.7%</span>
        </div>
        ${progressBar(90.7)}
      </div>
      <div>
        ${rows
          .map((row) => {
            const tone = row.status === "warning" ? "var(--warning)" : "var(--success)";
            return `
              <div class="coverage-row">
                <span class="data-chip">${escapeHtml(row.kind)}</span>
                <div>
                  <strong>${escapeHtml(row.source)}</strong>
                  <div class="event-meta">${escapeHtml(row.interval)} interval</div>
                </div>
                <div>
                  <strong>${row.coverage.toFixed(1)}%</strong>
                  ${progressBar(row.coverage, tone)}
                </div>
              </div>
            `;
          })
          .join("")}
      </div>
    </div>
  `;
}

export function assumptionsList(rows) {
  return `
    <div class="assumption-grid">
      ${rows
        .map(
          ([label, value]) => `
            <div class="assumption-row">
              <span class="assumption-label">${escapeHtml(label)}</span>
              <strong>${escapeHtml(value)}</strong>
            </div>
          `
        )
        .join("")}
    </div>
  `;
}

export function lineChart(values, { kind = "equity", label = "" } = {}) {
  const width = 760;
  const height = kind === "drawdown" ? 180 : 300;
  const pad = 28;
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  const points = values
    .map((value, index) => {
      const x = pad + (index / (values.length - 1)) * (width - pad * 2);
      const y = height - pad - ((value - min) / span) * (height - pad * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  const area = `${pad},${height - pad} ${points} ${width - pad},${height - pad}`;
  const lineClass = kind === "drawdown" ? "chart-drawdown" : "chart-equity";

  return `
    <div class="chart" aria-label="${escapeHtml(label)}">
      <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(label)}">
        <line class="chart-grid" x1="${pad}" x2="${width - pad}" y1="${pad}" y2="${pad}" />
        <line class="chart-grid" x1="${pad}" x2="${width - pad}" y1="${height / 2}" y2="${height / 2}" />
        <line class="chart-axis" x1="${pad}" x2="${width - pad}" y1="${height - pad}" y2="${height - pad}" />
        ${kind === "equity" ? `<polygon class="chart-fill" points="${area}" />` : ""}
        <polyline class="${lineClass}" points="${points}" />
      </svg>
    </div>
  `;
}

function scalePoints(values, width, height, pad, invert = false) {
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min || 1;
  return values
    .map((value, index) => {
      const x = pad + (index / (values.length - 1)) * (width - pad * 2);
      const ratio = (value - min) / span;
      const y = invert
        ? pad + ratio * (height - pad * 2)
        : height - pad - ratio * (height - pad * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

export function marketReplayChart({ candles, equity, drawdown, events, selectedEventId, compact = false }) {
  const width = 900;
  const candleHeight = compact ? 260 : 340;
  const equityHeight = compact ? 120 : 150;
  const drawdownHeight = compact ? 90 : 110;
  const totalHeight = candleHeight + equityHeight + drawdownHeight;
  const pad = 34;
  const closes = candles.map((item) => item.close);
  const lows = candles.map((item) => item.low);
  const highs = candles.map((item) => item.high);
  const min = Math.min(...lows);
  const max = Math.max(...highs);
  const span = max - min || 1;
  const candleWidth = Math.max(6, (width - pad * 2) / candles.length - 8);

  const yForPrice = (value) => candleHeight - pad - ((value - min) / span) * (candleHeight - pad * 2);
  const xForIndex = (index) => pad + (index / (candles.length - 1)) * (width - pad * 2);
  const equityPoints = scalePoints(equity, width, equityHeight, pad);
  const drawdownPoints = scalePoints(drawdown, width, drawdownHeight, pad, true);
  const eventMarkers = events
    .map((event) => {
      const x = pad + (event.x / 100) * (width - pad * 2);
      const selected = event.id === selectedEventId;
      return `
        <g class="replay-event ${event.status} ${selected ? "selected" : ""}">
          <line x1="${x}" x2="${x}" y1="${pad}" y2="${totalHeight - pad}" />
          <circle cx="${x}" cy="${yForPrice(closes[Math.min(candles.length - 1, Math.round((event.x / 100) * (candles.length - 1)))]).toFixed(1)}" r="${selected ? 6 : 4}" />
          <text x="${x + 8}" y="${Math.max(18, yForPrice(closes[Math.min(candles.length - 1, Math.round((event.x / 100) * (candles.length - 1)))]) - 10).toFixed(1)}">${event.kind}</text>
        </g>
      `;
    })
    .join("");

  const candleBars = candles
    .map((item, index) => {
      const x = xForIndex(index);
      const prev = index === 0 ? item.close : candles[index - 1].close;
      const up = item.close >= prev;
      const yHigh = yForPrice(item.high);
      const yLow = yForPrice(item.low);
      const yOpen = yForPrice(prev);
      const yClose = yForPrice(item.close);
      const rectY = Math.min(yOpen, yClose);
      const rectHeight = Math.max(3, Math.abs(yClose - yOpen));
      return `
        <g class="candle ${up ? "up" : "down"}">
          <line x1="${x}" x2="${x}" y1="${yHigh.toFixed(1)}" y2="${yLow.toFixed(1)}" />
          <rect x="${(x - candleWidth / 2).toFixed(1)}" y="${rectY.toFixed(1)}" width="${candleWidth.toFixed(1)}" height="${rectHeight.toFixed(1)}" rx="1" />
        </g>
      `;
    })
    .join("");

  return `
    <div class="replay-chart" aria-label="Historical market replay chart">
      <svg viewBox="0 0 ${width} ${totalHeight}" role="img" aria-label="Market data, equity, drawdown, and events">
        <rect class="lane-bg" x="0" y="0" width="${width}" height="${candleHeight}" />
        <rect class="lane-bg" x="0" y="${candleHeight}" width="${width}" height="${equityHeight}" />
        <rect class="lane-bg" x="0" y="${candleHeight + equityHeight}" width="${width}" height="${drawdownHeight}" />
        <line class="chart-grid" x1="${pad}" x2="${width - pad}" y1="${pad}" y2="${pad}" />
        <line class="chart-grid" x1="${pad}" x2="${width - pad}" y1="${candleHeight / 2}" y2="${candleHeight / 2}" />
        <line class="chart-axis" x1="${pad}" x2="${width - pad}" y1="${candleHeight - pad}" y2="${candleHeight - pad}" />
        ${candleBars}
        <text class="lane-label" x="${pad}" y="22">ETH / USDC historical price</text>
        <text class="axis-value" x="${width - pad}" y="${yForPrice(closes.at(-1)).toFixed(1)}">${escapeHtml(String(closes.at(-1)))}</text>

        <g transform="translate(0 ${candleHeight})">
          <text class="lane-label" x="${pad}" y="22">Equity</text>
          <line class="chart-grid" x1="${pad}" x2="${width - pad}" y1="${equityHeight / 2}" y2="${equityHeight / 2}" />
          <polyline class="chart-equity" points="${equityPoints}" />
        </g>
        <g transform="translate(0 ${candleHeight + equityHeight})">
          <text class="lane-label" x="${pad}" y="22">Drawdown</text>
          <line class="chart-grid" x1="${pad}" x2="${width - pad}" y1="${drawdownHeight / 2}" y2="${drawdownHeight / 2}" />
          <polyline class="chart-drawdown" points="${drawdownPoints}" />
        </g>
        ${eventMarkers}
      </svg>
    </div>
  `;
}

export function portfolioTable(assets) {
  return `
    <table class="portfolio-table">
      <thead>
        <tr>
          <th>Asset</th>
          <th>Balance</th>
          <th>Price</th>
          <th>Value</th>
          <th>% total</th>
        </tr>
      </thead>
      <tbody>
        ${assets
          .map(
            (asset) => `
              <tr>
                <td data-label="Asset">${escapeHtml(asset.asset)}</td>
                <td data-label="Balance">${escapeHtml(asset.balance)}</td>
                <td data-label="Price">${escapeHtml(asset.price)}</td>
                <td data-label="Value">${escapeHtml(asset.value)}</td>
                <td data-label="% total">${escapeHtml(asset.percent)}</td>
              </tr>
            `
          )
          .join("")}
      </tbody>
    </table>
  `;
}

export function dataTable({ columns, rows, className = "data-table" }) {
  return `
    <table class="${className}">
      <thead>
        <tr>${columns.map((column) => `<th>${escapeHtml(column.label)}</th>`).join("")}</tr>
      </thead>
      <tbody>
        ${rows
          .map(
            (row) => `
              <tr>
                ${columns
                  .map(
                    (column) => `
                      <td data-label="${escapeHtml(column.label)}">${column.render ? column.render(row) : escapeHtml(row[column.key])}</td>
                    `
                  )
                  .join("")}
              </tr>
            `
          )
          .join("")}
      </tbody>
    </table>
  `;
}

export function emptyState(title, copy) {
  return `
    <div class="empty-state">
      <p class="state-title">${escapeHtml(title)}</p>
      <p class="state-copy">${escapeHtml(copy)}</p>
    </div>
  `;
}

export function errorState(title, copy) {
  return `
    <div class="error-state">
      <p class="state-title">${escapeHtml(title)}</p>
      <p class="state-copy">${escapeHtml(copy)}</p>
    </div>
  `;
}
