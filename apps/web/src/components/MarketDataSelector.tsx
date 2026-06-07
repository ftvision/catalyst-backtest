import { Group, Select, SimpleGrid, Stack, Text, TextInput } from "@mantine/core";
import type { MarketDataCatalogItem } from "../api/client";
import { marketDataKindMatches, normalizeMarketDataKind } from "../utils/marketDataKind";
import { CoverageTimeline } from "./CoverageTimeline";
import { StatusBadge } from "./StatusBadge";

export function marketCatalogId(item: MarketDataCatalogItem) {
  return [
    item.kind,
    item.source,
    item.venue ?? item.chain ?? "-",
    item.symbol ?? item.asset ?? "-",
    item.protocol ?? "-",
    item.pool ?? "-",
    item.interval ?? "-",
    item.start ?? "-",
  ].join(":");
}

function labelFor(item: MarketDataCatalogItem) {
  if (item.kind === "candles") {
    return `${item.venue ?? "-"} ${item.symbol ?? "-"} / ${item.interval ?? "-"} candles`;
  }
  if (item.kind === "gas") return `${item.chain ?? "-"} gas / ${item.interval ?? "-"}`;
  if (item.kind === "funding") return `${item.venue ?? "-"} ${item.symbol ?? "-"} funding`;
  if (item.kind === "yields") {
    return `${item.protocol ?? "-"} ${item.asset ?? "-"} on ${item.chain ?? "-"} yields / ${item.interval ?? "-"}`;
  }
  return item.kind;
}

function fieldFor(item?: MarketDataCatalogItem, ...fields: Array<keyof MarketDataCatalogItem>) {
  for (const field of fields) {
    const value = item?.[field];
    if (typeof value === "string" && value.length) return value;
  }
  return "-";
}

function sameYield(left: MarketDataCatalogItem, right: MarketDataCatalogItem) {
  return (
    left.kind === "yields" &&
    right.kind === "yields" &&
    left.protocol === right.protocol &&
    left.asset === right.asset &&
    left.chain === right.chain &&
    (left.pool ?? undefined) === (right.pool ?? undefined)
  );
}

function formatRange(item?: MarketDataCatalogItem) {
  if (!item?.start || !item.end) return "-";
  return `${item.start.replace("T", " ").replace(":00Z", " UTC")} to ${item.end
    .replace("T", " ")
    .replace(":59Z", " UTC")}`;
}

export function MarketDataSelector({
  catalog,
  selectedId,
  onSelect,
  disabled = false,
  warnings = [],
  requiredKinds = ["candles", "gas", "funding"],
}: {
  catalog: MarketDataCatalogItem[];
  selectedId?: string;
  onSelect?: (id: string) => void;
  disabled?: boolean;
  warnings?: string[];
  requiredKinds?: string[];
}) {
  const normalizedRequiredKinds = new Set(requiredKinds.map((kind) => normalizeMarketDataKind(kind)));
  const selected = selectedId
    ? catalog.find((item) => marketCatalogId(item) === selectedId)
    : undefined;
  const related = selected
    ? catalog.filter((item) => {
        if (selected.kind === "candles") {
          if (item.kind === "gas") return item.chain === selected.venue;
          if (item.kind === "funding") return item.venue === selected.venue && item.symbol === selected.symbol;
          return marketCatalogId(item) === marketCatalogId(selected);
        }
        if (selected.kind === "gas") {
          if (item.kind === "candles") return item.venue === selected.chain;
          if (item.kind === "yields") return item.chain === selected.chain;
          return item.kind === "gas" && item.chain === selected.chain;
        }
        if (selected.kind === "funding") {
          if (item.kind === "candles") return item.venue === selected.venue && item.symbol === selected.symbol;
          return item.kind === "funding" && item.venue === selected.venue && item.symbol === selected.symbol;
        }
        if (selected.kind === "yields") {
          if (item.kind === "gas") return item.chain === selected.chain;
          return sameYield(item, selected);
        }
        return marketCatalogId(item) === marketCatalogId(selected);
      }).filter((item) =>
        normalizedRequiredKinds.has(normalizeMarketDataKind(item.kind)) ||
        marketCatalogId(item) === marketCatalogId(selected),
      )
    : [];
  const missingRequiredKinds = Array.from(normalizedRequiredKinds).filter(
    (kind) => !related.some((item) => marketDataKindMatches(item.kind, kind)),
  );
  const coverageStatus = warnings.length || missingRequiredKinds.length ? "danger" : related.length ? "success" : "warning";
  const options = catalog.map((item) => ({
    value: marketCatalogId(item),
    label: labelFor(item),
  }));
  const fallbackSource = catalog.length ? "Parquet store" : "Inline fallback";

  return (
    <Stack gap="md">
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="sm">
        <Select
          label="Local market data"
          value={selected ? marketCatalogId(selected) : undefined}
          data={options}
          onChange={(value) => value && onSelect?.(value)}
          disabled={disabled || options.length === 0}
          placeholder={options.length ? "Choose local series" : "No local Parquet series"}
          searchable
        />
        <TextInput label="Source" value={selected?.source ?? fallbackSource} readOnly />
        <TextInput label="Venue" value={fieldFor(selected, "venue", "chain")} readOnly />
        <TextInput label="Symbol" value={fieldFor(selected, "symbol", "asset")} readOnly />
        <TextInput label="Interval" value={selected?.interval ?? "-"} readOnly />
        <TextInput label="UTC coverage" value={formatRange(selected)} readOnly />
      </SimpleGrid>

      <Stack gap="xs">
        <Group justify="space-between">
          <Text fw={650}>Local coverage</Text>
          <StatusBadge status={coverageStatus} />
        </Group>
        <CoverageTimeline items={related} requiredKinds={requiredKinds} />
        {warnings.length ? (
          <Text size="xs" c="dimmed">
            {warnings.join(" ")}
          </Text>
        ) : null}
      </Stack>
    </Stack>
  );
}
