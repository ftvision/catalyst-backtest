import { Group, Select, SimpleGrid, Stack, Text, TextInput } from "@mantine/core";
import type { MarketDataCatalogItem } from "../api/client";
import { CoverageTimeline } from "./CoverageTimeline";
import { StatusBadge } from "./StatusBadge";

export function marketCatalogId(item: MarketDataCatalogItem) {
  return [
    item.kind,
    item.source,
    item.venue ?? item.chain ?? "-",
    item.symbol ?? "-",
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
  return item.kind;
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
}: {
  catalog: MarketDataCatalogItem[];
  selectedId?: string;
  onSelect?: (id: string) => void;
  disabled?: boolean;
  warnings?: string[];
}) {
  const candleItems = catalog.filter((item) => item.kind === "candles");
  const selected = candleItems.find((item) => marketCatalogId(item) === selectedId) ?? candleItems[0];
  const related = selected
    ? catalog.filter((item) => {
        if (item.kind === "gas") return item.chain === selected.venue;
        if (item.kind === "funding") return item.symbol === selected.symbol;
        return marketCatalogId(item) === marketCatalogId(selected);
      })
    : catalog;
  const options = candleItems.map((item) => ({
    value: marketCatalogId(item),
    label: labelFor(item),
  }));

  return (
    <Stack gap="md">
      <SimpleGrid cols={{ base: 1, md: 2 }} spacing="sm">
        <Select
          label="Local market data"
          value={selected ? marketCatalogId(selected) : undefined}
          data={options}
          onChange={(value) => value && onSelect?.(value)}
          disabled={disabled || options.length === 0}
          placeholder={options.length ? "Choose candle series" : "No local Parquet series"}
          searchable
        />
        <TextInput label="Source" value={selected?.source ?? "Inline fallback"} readOnly />
        <TextInput label="Venue" value={selected?.venue ?? "-"} readOnly />
        <TextInput label="Symbol" value={selected?.symbol ?? "-"} readOnly />
        <TextInput label="Interval" value={selected?.interval ?? "-"} readOnly />
        <TextInput label="UTC coverage" value={formatRange(selected)} readOnly />
      </SimpleGrid>

      <Stack gap="xs">
        <Group justify="space-between">
          <Text fw={650}>Local coverage</Text>
          <StatusBadge status={warnings.length ? "warning" : related.length ? "success" : "warning"} />
        </Group>
        <CoverageTimeline items={related} />
        {warnings.length ? (
          <Text size="xs" c="dimmed">
            {warnings.join(" ")}
          </Text>
        ) : null}
      </Stack>
    </Stack>
  );
}
