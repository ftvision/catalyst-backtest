import { Group, Select, SimpleGrid, Stack, Table, Text, TextInput } from "@mantine/core";
import { useEffect, useMemo, useState } from "react";
import type { MarketDataCatalogItem } from "../api/client";
import { CoverageTimeline } from "../components/CoverageTimeline";
import { SectionHeader } from "../components/SectionHeader";
import { SetupModule } from "../components/SetupModule";
import { StatusBadge } from "../components/StatusBadge";
import { marketCatalogId } from "../components/MarketDataSelector";
import type { GraphSummary, SetupData } from "../types";
import { marketDataKindMatches, normalizeMarketDataKind } from "../utils/marketDataKind";

function unique(values: Array<string | undefined | null>) {
  return Array.from(new Set(values.filter((value): value is string => Boolean(value)))).sort();
}

function selectOptions(values: string[]) {
  return [{ value: "all", label: "All" }, ...values.map((value) => ({ value, label: value }))];
}

function formatUtc(value?: string | null) {
  if (!value) return "-";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toISOString().replace("T", " ").replace(".000Z", " UTC");
}

function formatNumber(value?: number) {
  if (value === undefined) return "-";
  return new Intl.NumberFormat("en-US").format(value);
}

function itemLabel(item: MarketDataCatalogItem) {
  if (item.kind === "candles") return `${item.venue ?? "-"} ${item.symbol ?? "-"} candles`;
  if (item.kind === "gas") return `${item.chain ?? "-"} gas`;
  if (item.kind === "funding") return `${item.venue ?? "-"} ${item.symbol ?? "-"} funding`;
  if (item.kind === "yields") return `${item.chain ?? item.source} yields`;
  return item.kind;
}

function itemCoverageStatus(item: MarketDataCatalogItem, setup: SetupData) {
  const hasDateGaps = Boolean(item.missing_date_ranges?.length);
  const matchingCoverage = setup.coverage.find((row) => marketDataKindMatches(row.kind, item.kind));
  if (matchingCoverage?.status) return hasDateGaps && matchingCoverage.status === "success" ? "warning" : matchingCoverage.status;
  if (hasDateGaps) return "warning";
  if (!item.start || !item.end || !item.points) return "warning";
  return "success";
}

function requiredKinds(setup: SetupData) {
  const kinds = Array.from(new Set(setup.coverage.map((item) => normalizeMarketDataKind(item.kind))));
  return kinds.length ? kinds : ["candles", "gas", "funding"];
}

function graphRequirementText(graph: GraphSummary, item: MarketDataCatalogItem) {
  const kind = normalizeMarketDataKind(item.kind);
  const candidates = graph.nodes.filter((node) => {
    const haystack = `${node.kind} ${node.detail} ${node.label}`.toLowerCase();
    if (kind === "candles") return haystack.includes("price") || haystack.includes("signal") || haystack.includes("swap");
    if (kind === "gas") return haystack.includes("gas") || haystack.includes("base") || haystack.includes("swap");
    if (kind === "funding") return haystack.includes("funding") || haystack.includes("perp") || haystack.includes("long");
    if (kind === "yields") return haystack.includes("yield");
    return haystack.includes(kind);
  });

  if (candidates.length === 0) return "No direct node match";
  return candidates.map((node) => node.id).slice(0, 3).join(", ");
}

function freshnessLabel(item?: MarketDataCatalogItem) {
  if (!item?.end) return "Unknown";
  const end = new Date(item.end).getTime();
  if (!Number.isFinite(end)) return "Unknown";
  const days = Math.max(0, Math.round((Date.now() - end) / 86_400_000));
  if (days === 0) return "Current through today";
  return `${days}d since latest point`;
}

export function MarketDataPage({
  catalog,
  warnings,
  setup,
  graph,
}: {
  catalog: MarketDataCatalogItem[];
  warnings: string[];
  setup: SetupData;
  graph: GraphSummary;
}) {
  const [selectedId, setSelectedId] = useState(() => (catalog[0] ? marketCatalogId(catalog[0]) : undefined));
  const [kindFilter, setKindFilter] = useState("all");
  const [venueFilter, setVenueFilter] = useState("all");
  const [symbolFilter, setSymbolFilter] = useState("all");
  const [intervalFilter, setIntervalFilter] = useState("all");
  const [search, setSearch] = useState("");

  useEffect(() => {
    if (!catalog.some((item) => marketCatalogId(item) === selectedId)) {
      setSelectedId(catalog[0] ? marketCatalogId(catalog[0]) : undefined);
    }
  }, [catalog, selectedId]);

  const required = useMemo(() => requiredKinds(setup), [setup]);
  const filtered = useMemo(
    () =>
      catalog.filter((item) => {
        const venue = item.venue ?? item.chain ?? "-";
        const text = `${item.kind} ${item.source} ${venue} ${item.symbol ?? ""} ${item.quote ?? ""} ${item.interval ?? ""}`.toLowerCase();
        return (
          (kindFilter === "all" || item.kind === kindFilter) &&
          (venueFilter === "all" || venue === venueFilter) &&
          (symbolFilter === "all" || item.symbol === symbolFilter) &&
          (intervalFilter === "all" || item.interval === intervalFilter) &&
          (!search || text.includes(search.toLowerCase()))
        );
      }),
    [catalog, intervalFilter, kindFilter, search, symbolFilter, venueFilter],
  );
  const selected = catalog.find((item) => marketCatalogId(item) === selectedId) ?? filtered[0] ?? catalog[0];
  const presentRequired = required.filter((kind) => catalog.some((item) => marketDataKindMatches(item.kind, kind)));
  const missingRequired = required.filter((kind) => !catalog.some((item) => marketDataKindMatches(item.kind, kind)));
  const incomplete = catalog.filter((item) => itemCoverageStatus(item, setup) !== "success");
  const status = missingRequired.length ? "danger" : incomplete.length || warnings.length ? "warning" : "success";

  return (
    <Stack gap="md">
      <SectionHeader
        title="Market Data"
        subtitle="Inspect local series, required coverage, and gaps before selecting a replay window."
      />

      <SimpleGrid cols={{ base: 1, md: 4 }} spacing="xs">
        <div className="metric-cell panel-muted market-data-summary-cell">
          <Text size="xs" c="dimmed">Catalog series</Text>
          <Text fw={750}>{catalog.length}</Text>
        </div>
        <div className="metric-cell panel-muted market-data-summary-cell">
          <Text size="xs" c="dimmed">Required matched</Text>
          <Text fw={750}>{presentRequired.length} / {required.length}</Text>
        </div>
        <div className="metric-cell panel-muted market-data-summary-cell">
          <Text size="xs" c="dimmed">Missing required</Text>
          <Text fw={750} c={missingRequired.length ? "red" : undefined}>{missingRequired.length}</Text>
        </div>
        <div className="metric-cell panel-muted market-data-summary-cell">
          <Group justify="space-between" align="flex-start">
            <Stack gap={0}>
              <Text size="xs" c="dimmed">Inspection status</Text>
              <Text fw={750}>{warnings.length ? `${warnings.length} warning` : "Ready"}</Text>
            </Stack>
            <StatusBadge status={status} />
          </Group>
        </div>
      </SimpleGrid>

      <div className="market-data-layout">
        <Stack gap="md">
          <SetupModule title="Catalog" subtitle="Filter the local market-data inventory." status={status}>
            <SimpleGrid cols={{ base: 1, md: 5 }} spacing="sm">
              <Select label="Kind" value={kindFilter} data={selectOptions(unique(catalog.map((item) => item.kind)))} onChange={(value) => setKindFilter(value ?? "all")} />
              <Select label="Venue" value={venueFilter} data={selectOptions(unique(catalog.map((item) => item.venue ?? item.chain)))} onChange={(value) => setVenueFilter(value ?? "all")} />
              <Select label="Symbol" value={symbolFilter} data={selectOptions(unique(catalog.map((item) => item.symbol)))} onChange={(value) => setSymbolFilter(value ?? "all")} />
              <Select label="Interval" value={intervalFilter} data={selectOptions(unique(catalog.map((item) => item.interval)))} onChange={(value) => setIntervalFilter(value ?? "all")} />
              <TextInput label="Search" value={search} onChange={(event) => setSearch(event.currentTarget.value)} placeholder="source, quote, venue" />
            </SimpleGrid>

            <div className="market-data-table-wrap">
              <Table highlightOnHover withTableBorder>
                <Table.Thead>
                  <Table.Tr>
                    <Table.Th>Series</Table.Th>
                    <Table.Th>Source</Table.Th>
                    <Table.Th>UTC start</Table.Th>
                    <Table.Th>UTC end</Table.Th>
                    <Table.Th>Points</Table.Th>
                    <Table.Th>Status</Table.Th>
                  </Table.Tr>
                </Table.Thead>
                <Table.Tbody>
                  {filtered.map((item) => {
                    const id = marketCatalogId(item);
                    const rowStatus = itemCoverageStatus(item, setup);
                    return (
                      <Table.Tr
                        key={id}
                        className={id === marketCatalogId(selected) ? "selected-row" : undefined}
                        onClick={() => setSelectedId(id)}
                      >
                        <Table.Td>
                          <Stack gap={0}>
                            <Text fw={650} size="sm">{itemLabel(item)}</Text>
                            <Text size="xs" c="dimmed" className="mono">
                              {[item.quote, item.interval].filter(Boolean).join(" / ") || item.kind}
                            </Text>
                          </Stack>
                        </Table.Td>
                        <Table.Td className="mono">{item.source}</Table.Td>
                        <Table.Td className="mono">{formatUtc(item.start)}</Table.Td>
                        <Table.Td className="mono">{formatUtc(item.end)}</Table.Td>
                        <Table.Td className="mono">{formatNumber(item.points)}</Table.Td>
                        <Table.Td><StatusBadge status={rowStatus} /></Table.Td>
                      </Table.Tr>
                    );
                  })}
                </Table.Tbody>
              </Table>
            </div>
          </SetupModule>

          <SetupModule title="Coverage" subtitle="Series availability across the selected catalog." status={status}>
            <CoverageTimeline items={filtered.length ? filtered : catalog} requiredKinds={required} />
            {warnings.length ? (
              <Text size="sm" c="dimmed">
                {warnings.join(" ")}
              </Text>
            ) : null}
          </SetupModule>
        </Stack>

        <SetupModule title="Series details" subtitle="Read-only metadata for the selected row." status={selected ? itemCoverageStatus(selected, setup) : "warning"}>
          {selected ? (
            <Stack gap="md">
              <Stack gap={2}>
                <Text fw={750}>{itemLabel(selected)}</Text>
                <Text size="xs" c="dimmed" className="mono">
                  {marketCatalogId(selected)}
                </Text>
              </Stack>

              <div className="market-data-detail-grid">
                <Text size="xs" c="dimmed">Kind</Text>
                <Text size="sm">{selected.kind}</Text>
                <Text size="xs" c="dimmed">Source</Text>
                <Text size="sm" className="mono">{selected.source}</Text>
                <Text size="xs" c="dimmed">Venue / chain</Text>
                <Text size="sm">{selected.venue ?? selected.chain ?? "-"}</Text>
                <Text size="xs" c="dimmed">Symbol</Text>
                <Text size="sm">{selected.symbol ?? "-"}</Text>
                <Text size="xs" c="dimmed">Quote</Text>
                <Text size="sm">{selected.quote ?? "-"}</Text>
                <Text size="xs" c="dimmed">Interval</Text>
                <Text size="sm">{selected.interval ?? "-"}</Text>
                <Text size="xs" c="dimmed">Files</Text>
                <Text size="sm" className="mono">{formatNumber(selected.files)}</Text>
                <Text size="xs" c="dimmed">Date gaps</Text>
                <Text size="sm" className="mono">
                  {selected.missing_date_ranges?.length
                    ? selected.missing_date_ranges.map((range) => `${range.start} to ${range.end}`).slice(0, 3).join(", ")
                    : "-"}
                </Text>
                <Text size="xs" c="dimmed">Points</Text>
                <Text size="sm" className="mono">{formatNumber(selected.points)}</Text>
                <Text size="xs" c="dimmed">Latest point</Text>
                <Text size="sm">{freshnessLabel(selected)}</Text>
                <Text size="xs" c="dimmed">Required by</Text>
                <Text size="sm">{graphRequirementText(graph, selected)}</Text>
              </div>

              <Stack gap="xs">
                <Group justify="space-between">
                  <Text fw={650} size="sm">Window</Text>
                  <StatusBadge status={itemCoverageStatus(selected, setup)} />
                </Group>
                <Text size="sm" className="mono">
                  {formatUtc(selected.start)} to {formatUtc(selected.end)}
                </Text>
              </Stack>
            </Stack>
          ) : (
            <Text size="sm" c="dimmed">No market-data series found.</Text>
          )}
        </SetupModule>
      </div>
    </Stack>
  );
}
