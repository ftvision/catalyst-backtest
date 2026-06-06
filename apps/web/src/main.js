import { audit, graph, marketReplay, result, runHistory, setup } from "./data/mock-data.js";
import { renderExplainAudit } from "./pages/explain-audit.js";
import { renderMarketReplay } from "./pages/market-replay.js";
import { renderResultReview } from "./pages/result-review.js";
import { renderRunSetup } from "./pages/run-setup.js";

const routes = [
  { id: "setup", label: "Run Setup", render: () => renderRunSetup({ graph, setup, runHistory }) },
  { id: "replay", label: "Market Replay", render: () => renderMarketReplay({ graph, setup, result, marketReplay }) },
  { id: "lens", label: "Event Lens", render: () => renderExplainAudit({ graph, setup, result, audit, marketReplay }) },
  { id: "result", label: "Result Review", render: () => renderResultReview({ graph, setup, result }) },
];

const state = {
  route: "replay",
  running: false
};

const app = document.querySelector("#app");

function shell() {
  const activeRoute = routes.find((route) => route.id === state.route) || routes[0];

  app.innerHTML = `
    <div class="app-shell">
      <header class="topbar">
        <div class="brand">
          <span class="brand-mark" aria-hidden="true"></span>
          <div>
            <h1>Catalyst Backtest</h1>
            <p>Graph validation, result review, and simulation audit.</p>
          </div>
        </div>
        <div class="top-actions">
          <span class="status-pill success">API healthy</span>
          <button class="button ghost" id="copy-run">Copy run ID</button>
          <button class="button" id="download-json">Download JSON</button>
        </div>
      </header>

      <nav class="workflow-nav" aria-label="Workbench workflow" role="tablist">
        ${routes
          .map(
            (route) => `
              <button
                class="nav-tab"
                role="tab"
                aria-selected="${route.id === activeRoute.id}"
                data-route="${route.id}"
              >
                ${route.label}
              </button>
            `
          )
          .join("")}
      </nav>

      <main class="main-workspace" id="main-content">
        ${activeRoute.render()}
      </main>
    </div>
  `;

  bindEvents();
}

function bindEvents() {
  document.querySelectorAll("[data-route]").forEach((button) => {
    button.addEventListener("click", () => {
      state.route = button.dataset.route;
      shell();
    });
  });

  document.querySelectorAll("[data-policy]").forEach((button) => {
    button.addEventListener("click", () => {
      document.querySelectorAll("[data-policy]").forEach((item) => {
        item.setAttribute("aria-pressed", String(item === button));
      });
    });
  });

  document.querySelectorAll("[data-event-id]").forEach((button) => {
    button.addEventListener("click", () => {
      document.querySelectorAll("[data-event-id]").forEach((item) => {
        item.setAttribute("aria-pressed", String(item === button));
      });
    });
  });

  const runButton = document.querySelector("#run-backtest");
  if (runButton) {
    runButton.addEventListener("click", () => {
      state.running = true;
      runButton.disabled = true;
      runButton.querySelector("span:last-child").textContent = "Running";
      window.setTimeout(() => {
        state.route = "result";
        state.running = false;
        shell();
      }, 700);
    });
  }

  const copyButton = document.querySelector("#copy-run");
  if (copyButton) {
    copyButton.addEventListener("click", async () => {
      try {
        await navigator.clipboard.writeText(setup.runId);
        copyButton.textContent = "Copied";
        window.setTimeout(() => {
          copyButton.textContent = "Copy run ID";
        }, 900);
      } catch {
        copyButton.textContent = "Copy unavailable";
      }
    });
  }

  const downloadButton = document.querySelector("#download-json");
  if (downloadButton) {
    downloadButton.addEventListener("click", () => {
      const blob = new Blob([JSON.stringify({ graph, setup, marketReplay, result, audit }, null, 2)], {
        type: "application/json"
      });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = "catalyst-backtest-workbench.json";
      link.click();
      URL.revokeObjectURL(url);
    });
  }
}

shell();
