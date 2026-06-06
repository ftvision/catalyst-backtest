import { createTheme, rem } from "@mantine/core";

export const theme = createTheme({
  fontFamily:
    '-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
  fontFamilyMonospace:
    'ui-monospace, SFMono-Regular, "SF Mono", Consolas, "Liberation Mono", monospace',
  headings: {
    fontFamily:
      '-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif',
    fontWeight: "650",
    sizes: {
      h1: { fontSize: rem(22), lineHeight: "1.2" },
      h2: { fontSize: rem(18), lineHeight: "1.25" },
      h3: { fontSize: rem(15), lineHeight: "1.3" },
    },
  },
  defaultRadius: "sm",
  primaryColor: "workbenchBlue",
  colors: {
    workbenchBlue: [
      "oklch(0.97 0.018 245)",
      "oklch(0.93 0.035 245)",
      "oklch(0.86 0.07 245)",
      "oklch(0.77 0.11 245)",
      "oklch(0.68 0.15 245)",
      "oklch(0.58 0.18 245)",
      "oklch(0.49 0.17 245)",
      "oklch(0.42 0.14 245)",
      "oklch(0.36 0.11 245)",
      "oklch(0.29 0.08 245)",
    ],
  },
  components: {
    Button: {
      defaultProps: {
        size: "xs",
      },
    },
    ActionIcon: {
      defaultProps: {
        size: "sm",
        variant: "subtle",
      },
    },
    Badge: {
      defaultProps: {
        size: "sm",
      },
    },
    Table: {
      defaultProps: {
        verticalSpacing: "xs",
        horizontalSpacing: "sm",
      },
    },
  },
});
