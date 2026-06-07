export function normalizeMarketDataKind(kind: string) {
  const normalized = kind.trim().toLowerCase();
  if (normalized === "yield") return "yields";
  return normalized;
}

export function marketDataKindMatches(left: string, right: string) {
  return normalizeMarketDataKind(left) === normalizeMarketDataKind(right);
}
