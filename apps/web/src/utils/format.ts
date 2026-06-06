export function formatNumber(value: number, maximumFractionDigits = 4) {
  if (!Number.isFinite(value)) return String(value);
  const normalized = Math.abs(value) < 10 ** -maximumFractionDigits ? 0 : value;

  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits,
  }).format(normalized);
}

export function formatPercent(value: number, maximumFractionDigits = 4) {
  return `${formatNumber(value, maximumFractionDigits)}%`;
}
