import type { GraphSummary, SetupData } from "../types";

const API_BASE = import.meta.env.VITE_CATALYST_API_BASE ?? "http://127.0.0.1:8000";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
    ...init,
  });

  if (!response.ok) {
    const body = await response.json().catch(() => undefined);
    throw new Error(body?.error?.message ?? `Catalyst API request failed: ${response.status}`);
  }

  return response.json() as Promise<T>;
}

export const catalystApi = {
  health: () => request<{ status: string; service: string }>("/health"),
  listPolicyProfiles: () => request<{ items: unknown[] }>("/policy-profiles"),
  listBacktests: (graphHash?: string) =>
    request<{ items: unknown[] }>(`/backtests${graphHash ? `?graph_hash=${graphHash}` : ""}`),
  previewGraph: (graph: GraphSummary, policy?: Record<string, unknown>) =>
    request<unknown>("/backtests/preview", {
      method: "POST",
      body: JSON.stringify({ graph, policy }),
    }),
  checkCoverage: (graph: GraphSummary, setup: SetupData) =>
    request<unknown>("/market-data/coverage", {
      method: "POST",
      body: JSON.stringify({
        graph,
        start: setup.start,
        end: setup.end,
        interval: setup.interval,
      }),
    }),
};
