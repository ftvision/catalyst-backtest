const API_BASE = window.CATALYST_BACKTEST_API_BASE || "";

async function request(path, options = {}) {
  const response = await fetch(`${API_BASE}${path}`, {
    headers: { "content-type": "application/json", ...(options.headers || {}) },
    ...options
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Request failed with ${response.status}`);
  }

  return response.json();
}

export function createBacktest(payload) {
  return request("/backtests", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function getBacktest(runId) {
  return request(`/backtests/${encodeURIComponent(runId)}`);
}

export function getBacktestResult(runId) {
  return request(`/backtests/${encodeURIComponent(runId)}/result`);
}

export function getBacktestEvents(runId, params = {}) {
  const search = new URLSearchParams(params);
  const suffix = search.toString() ? `?${search.toString()}` : "";
  return request(`/backtests/${encodeURIComponent(runId)}/events${suffix}`);
}

export function previewBacktest(payload) {
  return request("/backtests/preview", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function getPolicyProfiles() {
  return request("/policy-profiles");
}
