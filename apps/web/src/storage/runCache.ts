import type {
  BacktestConfig,
  BacktestEvent,
  BacktestMetadata,
  BacktestResult,
  BacktestStatus,
  CatalystGraph,
  MarketDataBundle,
} from "../api/client";

const DB_NAME = "catalyst-backtest";
const DB_VERSION = 1;
const RUN_DETAIL_STORE = "run-details";
const LAST_RUN_ID_KEY = "catalyst:last-run-id";
const CACHE_SCHEMA_VERSION = 1;

export interface CachedRunDetail {
  schemaVersion: typeof CACHE_SCHEMA_VERSION;
  runId: string;
  savedAt: string;
  graphHash?: string;
  strategyId?: string;
  strategyTitle?: string;
  scenarioId?: string;
  scenarioTitle?: string;
  request: {
    graph: CatalystGraph;
    config: BacktestConfig;
    policyProfile: string;
    dataSourceMode: "store" | "inline";
    marketDataId?: string;
  };
  status: BacktestStatus;
  result: BacktestResult;
  metadata: BacktestMetadata;
  events: BacktestEvent[];
  replayMarketData?: MarketDataBundle;
}

function hasIndexedDb() {
  return typeof indexedDB !== "undefined";
}

function openRunCache(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (!hasIndexedDb()) {
      reject(new Error("IndexedDB is unavailable"));
      return;
    }

    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onupgradeneeded = () => {
      const db = request.result;
      if (!db.objectStoreNames.contains(RUN_DETAIL_STORE)) {
        db.createObjectStore(RUN_DETAIL_STORE, { keyPath: "runId" });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Failed to open run cache"));
  });
}

async function runStoreTransaction<T>(
  mode: IDBTransactionMode,
  work: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const db = await openRunCache();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(RUN_DETAIL_STORE, mode);
    const store = transaction.objectStore(RUN_DETAIL_STORE);
    const request = work(store);

    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Run cache request failed"));
    transaction.oncomplete = () => db.close();
    transaction.onabort = () => {
      db.close();
      reject(transaction.error ?? new Error("Run cache transaction aborted"));
    };
    transaction.onerror = () => {
      db.close();
      reject(transaction.error ?? new Error("Run cache transaction failed"));
    };
  });
}

export function makeCachedRunDetail(input: Omit<CachedRunDetail, "schemaVersion" | "savedAt">): CachedRunDetail {
  return {
    ...input,
    schemaVersion: CACHE_SCHEMA_VERSION,
    savedAt: new Date().toISOString(),
  };
}

export async function saveCachedRunDetail(detail: CachedRunDetail): Promise<void> {
  await runStoreTransaction("readwrite", (store) => store.put(detail));
}

export async function loadCachedRunDetail(runId: string): Promise<CachedRunDetail | undefined> {
  const detail = await runStoreTransaction<CachedRunDetail | undefined>("readonly", (store) => store.get(runId));
  if (detail?.schemaVersion !== CACHE_SCHEMA_VERSION) return undefined;
  return detail;
}

/** Every cached run, newest first — the user's local backtest history. */
export async function loadAllCachedRunDetails(): Promise<CachedRunDetail[]> {
  try {
    const all = await runStoreTransaction<CachedRunDetail[]>("readonly", (store) => store.getAll());
    return all
      .filter((detail) => detail?.schemaVersion === CACHE_SCHEMA_VERSION)
      .sort((a, b) => b.savedAt.localeCompare(a.savedAt));
  } catch {
    return [];
  }
}

export function setLastRunId(runId: string) {
  if (typeof localStorage === "undefined") return;
  localStorage.setItem(LAST_RUN_ID_KEY, runId);
}

export function getLastRunId() {
  if (typeof localStorage === "undefined") return undefined;
  return localStorage.getItem(LAST_RUN_ID_KEY) ?? undefined;
}
