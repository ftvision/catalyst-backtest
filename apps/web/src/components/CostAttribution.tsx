import { Group, Stack, Text } from "@mantine/core";
import type { ResultData } from "../types";

type CostItem = ResultData["costs"][number];

function signedCurrency(value: number) {
  const sign = value > 0 ? "+" : value < 0 ? "-" : "";
  return `${sign}$${Math.abs(value).toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}`;
}

function findCost(costs: CostItem[], label: string) {
  return costs.find((cost) => cost.label.toLowerCase() === label.toLowerCase());
}

function isSummaryRow(cost: CostItem) {
  const label = cost.label.toLowerCase();
  return label === "gross pnl" || label === "net pnl";
}

function amountClass(value: number) {
  return value < 0 ? "negative" : "positive";
}

export function CostAttribution({ costs, compact = false }: { costs: CostItem[]; compact?: boolean }) {
  const gross = findCost(costs, "Gross PnL") ?? costs[0];
  const suppliedNet = findCost(costs, "Net PnL");
  const deductions = costs.filter((cost) => !isSummaryRow(cost) && cost.amount < 0);
  const totalCosts = deductions.reduce((sum, cost) => sum + cost.amount, 0);
  const computedNet = gross.amount + totalCosts;
  const net = suppliedNet ? { ...suppliedNet, amount: suppliedNet.amount || computedNet } : { label: "Net PnL", amount: computedNet };
  const denominator = Math.max(Math.abs(gross.amount), Math.abs(net.amount), Math.abs(totalCosts), 1);
  const retainedPct = Math.max(0, Math.min(100, (Math.abs(net.amount) / denominator) * 100));
  const costPct = Math.max(0, Math.min(100, (Math.abs(totalCosts) / denominator) * 100));

  return (
    <Stack className={compact ? "cost-attribution compact" : "cost-attribution"} gap="md">
      <div className="cost-summary">
        <div>
          <Text size="xs" c="dimmed">
            Gross PnL
          </Text>
          <Text className={`cost-number ${amountClass(gross.amount)}`}>{signedCurrency(gross.amount)}</Text>
        </div>
        <div>
          <Text size="xs" c="dimmed">
            Total costs
          </Text>
          <Text className="cost-number negative">{signedCurrency(totalCosts)}</Text>
        </div>
        <div>
          <Text size="xs" c="dimmed">
            Net PnL
          </Text>
          <Text className={`cost-number ${amountClass(net.amount)}`}>{signedCurrency(net.amount)}</Text>
        </div>
      </div>

      <div
        className="cost-retention"
        aria-label={`Gross PnL ${signedCurrency(gross.amount)}, costs ${signedCurrency(totalCosts)}, net PnL ${signedCurrency(
          net.amount,
        )}`}
      >
        <div className="cost-retention-track">
          <span className="cost-retention-net" style={{ width: `${retainedPct}%` }} />
          <span className="cost-retention-loss" style={{ width: `${costPct}%` }} />
        </div>
        <Group justify="space-between" gap="xs">
          <Text size="xs" c="dimmed">
            {retainedPct.toFixed(1)}% retained
          </Text>
          <Text size="xs" c="dimmed">
            {costPct.toFixed(1)}% deducted
          </Text>
        </Group>
      </div>

      <div className="cost-ledger">
        {deductions.map((cost) => (
          <Group key={cost.label} justify="space-between" gap="xs" className="cost-ledger-row">
            <Text size="sm">{cost.label}</Text>
            <Text size="sm" className={cost.amount < 0 ? "mono metric-negative" : "mono metric-positive"}>
              {signedCurrency(cost.amount)}
            </Text>
          </Group>
        ))}
        <Group justify="space-between" gap="xs" className="cost-ledger-row total">
          <Text size="sm" fw={650}>
            Total costs
          </Text>
          <Text size="sm" className="mono metric-negative" fw={650}>
            {signedCurrency(totalCosts)}
          </Text>
        </Group>
      </div>
    </Stack>
  );
}
