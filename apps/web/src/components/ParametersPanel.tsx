import { Button, Group, Stack, Text, TextInput } from "@mantine/core";
import { useEffect, useState } from "react";

type Scalar = string | number | boolean;

function toStrings(vars: Record<string, Scalar>): Record<string, string> {
  return Object.fromEntries(Object.entries(vars).map(([k, v]) => [k, String(v)]));
}

/**
 * Edit a graph's `variables` (the only user-tunable values on an otherwise
 * read-only graph) and re-run preview. Renders nothing if the graph has none.
 */
export function ParametersPanel({
  variables,
  resolved,
  onApply,
  busy = false,
}: {
  variables: Record<string, Scalar>;
  resolved?: Record<string, unknown>;
  onApply: (vars: Record<string, string>) => void;
  busy?: boolean;
}) {
  const names = Object.keys(variables);
  const [draft, setDraft] = useState<Record<string, string>>(() => toStrings(variables));

  // Re-sync when the upstream graph (e.g. a different strategy) changes.
  const signature = JSON.stringify(variables);
  useEffect(() => {
    setDraft(toStrings(variables));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  if (names.length === 0) return null;

  const current = toStrings(variables);
  const dirty = names.some((name) => draft[name] !== current[name]);

  return (
    <Stack gap="sm">
      <Text size="sm" c="dimmed">
        Tune this strategy's parameters, then apply to re-validate and run with the new values.
      </Text>
      {names.map((name) => {
        const resolvedValue = resolved && name in resolved ? String(resolved[name]) : undefined;
        return (
          <TextInput
            key={name}
            label={name}
            value={draft[name] ?? ""}
            onChange={(e) => setDraft((d) => ({ ...d, [name]: e.currentTarget.value }))}
            description={
              resolvedValue !== undefined ? `resolved: ${resolvedValue}` : "graph variable"
            }
            classNames={{ input: "mono" }}
          />
        );
      })}
      <Group justify="flex-end">
        <Button
          size="xs"
          variant="light"
          disabled={!dirty || busy}
          loading={busy}
          onClick={() => onApply(draft)}
        >
          Apply parameters
        </Button>
      </Group>
    </Stack>
  );
}
