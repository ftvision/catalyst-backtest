import { ActionIcon, Button, Group, NumberInput, ScrollArea, Select, Stack, Table, Text } from "@mantine/core";
import { Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { BacktestConfig } from "../api/client";

type InitialPortfolio = BacktestConfig["initial_portfolio"];

const VENUE_OPTIONS = [
  { value: "base", label: "Base" },
  { value: "hyperliquid", label: "Hyperliquid" },
];

const ASSET_OPTIONS_BY_VENUE: Record<string, Array<{ value: string; label: string }>> = {
  base: [
    { value: "USDC", label: "USDC" },
    { value: "ETH", label: "ETH" },
  ],
  hyperliquid: [
    { value: "USDC", label: "USDC" },
    { value: "ETH", label: "ETH" },
  ],
};

const DEFAULT_ASSET_OPTIONS = [
  { value: "USDC", label: "USDC" },
  { value: "ETH", label: "ETH" },
];

interface PortfolioDraftRow {
  id: string;
  venue: string;
  asset: string;
  amount: string;
}

function rowsFromPortfolio(portfolio: InitialPortfolio): PortfolioDraftRow[] {
  return Object.entries(portfolio).flatMap(([venue, assets], venueIndex) =>
    Object.entries(assets).map(([asset, amount], assetIndex) => ({
      id: `${venueIndex}-${assetIndex}-${venue}-${asset}`,
      venue,
      asset,
      amount,
    })),
  );
}

function portfolioFromRows(rows: PortfolioDraftRow[]): InitialPortfolio {
  return rows.reduce<InitialPortfolio>((next, row) => {
    const venue = row.venue.trim();
    const asset = row.asset.trim();
    const amount = row.amount.trim();
    if (!venue || !asset || !amount) return next;
    next[venue] = { ...(next[venue] ?? {}), [asset]: amount };
    return next;
  }, {});
}

function signature(portfolio: InitialPortfolio) {
  return JSON.stringify(portfolio);
}

function isPositiveAmount(value: string) {
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed > 0;
}

function rowKey(row: Pick<PortfolioDraftRow, "venue" | "asset">) {
  return `${row.venue.trim().toLowerCase()}::${row.asset.trim().toUpperCase()}`;
}

function assetOptionsForVenue(venue: string) {
  return ASSET_OPTIONS_BY_VENUE[venue] ?? DEFAULT_ASSET_OPTIONS;
}

export function InitialPortfolioEditor({
  portfolio,
  onApply,
  disabled = false,
}: {
  portfolio: InitialPortfolio;
  onApply: (portfolio: InitialPortfolio) => void;
  disabled?: boolean;
}) {
  const portfolioSignature = signature(portfolio);
  const [draftRows, setDraftRows] = useState<PortfolioDraftRow[]>(() => rowsFromPortfolio(portfolio));

  useEffect(() => {
    setDraftRows(rowsFromPortfolio(portfolio));
  }, [portfolioSignature]);

  const appliedRows = useMemo(() => rowsFromPortfolio(portfolio), [portfolioSignature]);
  const draftSignature = JSON.stringify(
    draftRows.map(({ venue, asset, amount }) => ({
      venue: venue.trim(),
      asset: asset.trim(),
      amount: amount.trim(),
    })),
  );
  const appliedSignature = JSON.stringify(
    appliedRows.map(({ venue, asset, amount }) => ({ venue, asset, amount })),
  );
  const completeRows = draftRows.filter((row) => row.venue.trim() && row.asset.trim() && row.amount.trim());
  const hasIncompleteRow = draftRows.length !== completeRows.length;
  const hasInvalidRow = completeRows.some((row) => !isPositiveAmount(row.amount));
  const completeKeys = completeRows.map(rowKey);
  const hasDuplicateRow = new Set(completeKeys).size !== completeKeys.length;
  const canApply =
    completeRows.length > 0 &&
    !hasIncompleteRow &&
    !hasInvalidRow &&
    !hasDuplicateRow &&
    draftSignature !== appliedSignature;

  function updateRow(id: string, patch: Partial<PortfolioDraftRow>) {
    setDraftRows((current) => current.map((row) => (row.id === id ? { ...row, ...patch } : row)));
  }

  function updateVenue(id: string, venue: string | null) {
    setDraftRows((current) =>
      current.map((row) => {
        if (row.id !== id) return row;
        const nextVenue = venue ?? "";
        const nextAssets = assetOptionsForVenue(nextVenue).map((option) => option.value);
        return {
          ...row,
          venue: nextVenue,
          asset: row.asset && nextAssets.includes(row.asset) ? row.asset : "",
        };
      }),
    );
  }

  function addRow() {
    setDraftRows((current) => [
      ...current,
      { id: `new-${Date.now()}`, venue: "", asset: "", amount: "" },
    ]);
  }

  function removeRow(id: string) {
    setDraftRows((current) => current.filter((row) => row.id !== id));
  }

  return (
    <Stack gap="sm">
      <ScrollArea className="table-scroll portfolio-editor-scroll">
        <Table withTableBorder highlightOnHover>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>Venue</Table.Th>
              <Table.Th>Asset</Table.Th>
              <Table.Th>Amount</Table.Th>
              <Table.Th className="portfolio-editor-action-cell">Action</Table.Th>
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {draftRows.map((row) => (
              <Table.Tr key={row.id}>
                <Table.Td>
                  <Select
                    aria-label={`Venue for ${row.asset || "balance"}`}
                    data={VENUE_OPTIONS}
                    value={row.venue}
                    onChange={(value) => updateVenue(row.id, value)}
                    disabled={disabled}
                    placeholder="Choose venue"
                    classNames={{ input: "mono" }}
                  />
                </Table.Td>
                <Table.Td>
                  <Select
                    aria-label={`Asset for ${row.venue || "venue"}`}
                    data={assetOptionsForVenue(row.venue)}
                    value={row.asset}
                    onChange={(value) => updateRow(row.id, { asset: value ?? "" })}
                    disabled={disabled || !row.venue}
                    placeholder={row.venue ? "Choose asset" : "Choose venue first"}
                    classNames={{ input: "mono" }}
                  />
                </Table.Td>
                <Table.Td>
                  <NumberInput
                    aria-label={`Amount for ${row.venue || "venue"} ${row.asset || "asset"}`}
                    value={row.amount}
                    min={0}
                    allowNegative={false}
                    thousandSeparator=","
                    onChange={(value) => updateRow(row.id, { amount: value === "" ? "" : String(value) })}
                    disabled={disabled}
                    classNames={{ input: "mono" }}
                  />
                </Table.Td>
                <Table.Td className="portfolio-editor-action-cell">
                  <ActionIcon
                    aria-label={`Remove ${row.venue || "venue"} ${row.asset || "asset"}`}
                    variant="subtle"
                    color="red"
                    onClick={() => removeRow(row.id)}
                    disabled={disabled || draftRows.length <= 1}
                  >
                    <Trash2 size={16} />
                  </ActionIcon>
                </Table.Td>
              </Table.Tr>
            ))}
          </Table.Tbody>
        </Table>
      </ScrollArea>

      <Group justify="space-between" align="center">
        <Text size="xs" c={hasIncompleteRow || hasInvalidRow || hasDuplicateRow ? "red" : "dimmed"}>
          {hasIncompleteRow
            ? "Complete or remove blank balance rows."
            : hasInvalidRow
            ? "Amounts must be greater than zero."
            : hasDuplicateRow
              ? "Each venue and asset pair must be unique."
              : `${completeRows.length} balance rows`}
        </Text>
        <Group gap="xs">
          <Button leftSection={<Plus size={14} />} variant="light" size="xs" onClick={addRow} disabled={disabled}>
            Add balance
          </Button>
          <Button size="xs" onClick={() => onApply(portfolioFromRows(completeRows))} disabled={disabled || !canApply}>
            Apply balances
          </Button>
        </Group>
      </Group>
    </Stack>
  );
}
