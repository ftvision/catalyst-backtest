import { Group, Stack, Text } from "@mantine/core";
import type { MarketDataCatalogItem } from "../api/client";
import { marketDataKindMatches, normalizeMarketDataKind } from "../utils/marketDataKind";
import { StatusBadge } from "./StatusBadge";

function timeMs(value?: string | null) {
  if (!value) return undefined;
  const parsed = new Date(value).getTime();
  return Number.isFinite(parsed) ? parsed : undefined;
}

function shortUtc(value?: string | null) {
  const ms = timeMs(value);
  if (ms === undefined) return "-";
  const date = new Date(ms);
  return `${date.toISOString().slice(0, 10)} ${String(date.getUTCHours()).padStart(2, "0")}:00 UTC`;
}

function dateBoundaryMs(value: string, end: boolean) {
  if (value.includes("T")) return timeMs(value);
  return timeMs(`${value}T${end ? "23:59:59" : "00:00:00"}Z`);
}

function rowLabel(item: MarketDataCatalogItem) {
  if (item.kind === "candles") return `${item.venue ?? "-"} ${item.symbol ?? "-"} candles`;
  if (item.kind === "gas") return `${item.chain ?? "-"} gas`;
  if (item.kind === "funding") return `${item.venue ?? "-"} ${item.symbol ?? "-"} funding`;
  return item.kind;
}

export function CoverageTimeline({
  items,
  requiredKinds = ["candles", "gas", "funding"],
}: {
  items: MarketDataCatalogItem[];
  requiredKinds?: string[];
}) {
  const spans = items
    .map((item) => ({ item, start: timeMs(item.start), end: timeMs(item.end) }))
    .filter((span): span is { item: MarketDataCatalogItem; start: number; end: number } =>
      span.start !== undefined && span.end !== undefined,
    );
  const min = spans.length ? Math.min(...spans.map((span) => span.start)) : 0;
  const max = spans.length ? Math.max(...spans.map((span) => span.end)) : min + 1;
  const width = Math.max(max - min, 1);
  const rows: Array<{ item?: MarketDataCatalogItem; kind: string; missing: boolean }> = [
    ...items.map((item) => ({ item, kind: item.kind, missing: false })),
    ...requiredKinds
      .map((kind) => normalizeMarketDataKind(kind))
      .filter((kind, index, kinds) => kinds.indexOf(kind) === index)
      .filter((kind) => !items.some((item) => marketDataKindMatches(item.kind, kind)))
      .map((kind) => ({ kind, missing: true })),
  ];

  return (
    <Stack gap="xs">
      <Group justify="space-between">
        <Text size="xs" c="dimmed" className="mono">
          {spans.length ? shortUtc(new Date(min).toISOString()) : "No local coverage"}
        </Text>
        <Text size="xs" c="dimmed" className="mono">
          {spans.length ? shortUtc(new Date(max).toISOString()) : "-"}
        </Text>
      </Group>
      <Stack gap={6}>
        {rows.map((row, index) => {
          const start = timeMs(row.item?.start);
          const end = timeMs(row.item?.end);
          const left = start === undefined ? 0 : ((start - min) / width) * 100;
          const spanWidth = end === undefined || start === undefined ? 100 : Math.max(((end - start) / width) * 100, 2);
          const hasGaps = Boolean(row.item?.missing_date_ranges?.length);
          const status = row.missing || hasGaps ? "warning" : "success";
          return (
            <div key={`${row.kind}-${index}`} className="coverage-row">
              <div className="coverage-row-label">
                <Text size="xs" fw={650}>
                  {row.item ? rowLabel(row.item) : row.kind}
                </Text>
                <StatusBadge status={status} label={row.missing ? "missing" : hasGaps ? "gaps" : "covered"} />
              </div>
              <div className="coverage-track" aria-label={`${row.kind} coverage`}>
                <span
                  className={row.missing ? "coverage-segment missing" : "coverage-segment covered"}
                  style={{ left: `${left}%`, width: `${spanWidth}%` }}
                />
                {row.item?.missing_date_ranges?.map((range) => {
                  const gapStart = dateBoundaryMs(range.start, false);
                  const gapEnd = dateBoundaryMs(range.end, true);
                  if (gapStart === undefined || gapEnd === undefined) return null;
                  const gapLeft = Math.max(0, Math.min(100, ((gapStart - min) / width) * 100));
                  const gapRight = Math.max(0, Math.min(100, ((gapEnd - min) / width) * 100));
                  return (
                    <span
                      key={`${range.start}-${range.end}`}
                      className="coverage-segment missing"
                      style={{ left: `${gapLeft}%`, width: `${Math.max(gapRight - gapLeft, 1)}%` }}
                    />
                  );
                })}
              </div>
            </div>
          );
        })}
      </Stack>
    </Stack>
  );
}
