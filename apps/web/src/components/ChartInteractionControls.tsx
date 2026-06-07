import { ActionIcon, Group, Tooltip } from "@mantine/core";
import { RotateCcw, ScanLine, ZoomIn, ZoomOut } from "lucide-react";
import { useState, type MouseEvent, type MutableRefObject } from "react";
import type { IChartApi } from "lightweight-charts";

interface ChartInteractionControlsProps {
  ariaLabel: string;
  chartRef: MutableRefObject<IChartApi | null>;
  labelPrefix: string;
  logicalRangeBounds?: { from: number; to: number };
  resetRange: () => void;
}

const minSelectionWidth = 18;

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function ChartInteractionControls({
  ariaLabel,
  chartRef,
  labelPrefix,
  logicalRangeBounds,
  resetRange,
}: ChartInteractionControlsProps) {
  const [selectMode, setSelectMode] = useState(false);
  const [selectionStartX, setSelectionStartX] = useState<number | null>(null);

  const zoomLogicalRange = (factor: number) => {
    const timeScale = chartRef.current?.timeScale();
    const range = timeScale?.getVisibleLogicalRange();
    if (!timeScale || !range) return;

    const span = range.to - range.from;
    if (span <= 0) return;

    const center = (range.from + range.to) / 2;
    const targetSpan = span * factor;
    const boundsSpan = logicalRangeBounds ? logicalRangeBounds.to - logicalRangeBounds.from : 0;

    if (logicalRangeBounds && targetSpan >= boundsSpan) {
      timeScale.setVisibleLogicalRange(logicalRangeBounds);
      return;
    }

    const halfSpan = targetSpan / 2;
    const clampedCenter = logicalRangeBounds
      ? clamp(center, logicalRangeBounds.from + halfSpan, logicalRangeBounds.to - halfSpan)
      : center;
    timeScale.setVisibleLogicalRange({ from: clampedCenter - halfSpan, to: clampedCenter + halfSpan });
  };

  const pointerX = (event: MouseEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    return clamp(event.clientX - rect.left, 0, rect.width);
  };

  const applySelection = (startX: number, currentX: number) => {
    const timeScale = chartRef.current?.timeScale();
    if (!timeScale) return;

    const fromX = Math.min(startX, currentX);
    const toX = Math.max(startX, currentX);
    if (toX - fromX < minSelectionWidth) return;

    const from = timeScale.coordinateToLogical(fromX);
    const to = timeScale.coordinateToLogical(toX);
    if (from === null || to === null) return;

    try {
      timeScale.setVisibleLogicalRange({ from, to });
    } catch {
      return;
    }
  };

  const selectPoint = (event: MouseEvent<HTMLDivElement>) => {
    if (!selectMode || event.button !== 0) return;
    event.preventDefault();
    const nextX = pointerX(event);
    if (selectionStartX === null) {
      setSelectionStartX(nextX);
      return;
    }
    applySelection(selectionStartX, nextX);
    setSelectionStartX(null);
    setSelectMode(false);
  };

  const handleReset = () => {
    resetRange();
    setSelectionStartX(null);
    setSelectMode(false);
  };

  return (
    <>
      <div
        className={selectMode ? "chart-selection-layer active" : "chart-selection-layer"}
        onClick={selectPoint}
      >
        {selectionStartX !== null ? <div className="chart-selection-anchor" style={{ left: selectionStartX }} /> : null}
      </div>
      <Group gap={4} className="chart-controls" aria-label={ariaLabel}>
        <Tooltip label="Zoom in">
          <ActionIcon size="sm" variant="light" aria-label={`Zoom in ${labelPrefix} chart`} onClick={() => zoomLogicalRange(0.7)}>
            <ZoomIn size={14} />
          </ActionIcon>
        </Tooltip>
        <Tooltip label="Zoom out">
          <ActionIcon size="sm" variant="light" aria-label={`Zoom out ${labelPrefix} chart`} onClick={() => zoomLogicalRange(1.45)}>
            <ZoomOut size={14} />
          </ActionIcon>
        </Tooltip>
        <Tooltip label={selectMode ? "Cancel range select" : "Select range"}>
          <ActionIcon
            size="sm"
            variant={selectMode ? "filled" : "light"}
            aria-label={`${selectMode ? "Cancel" : "Select"} ${labelPrefix} chart range`}
            aria-pressed={selectMode}
            onClick={() => {
              setSelectionStartX(null);
              setSelectMode((current) => !current);
            }}
          >
            <ScanLine size={14} />
          </ActionIcon>
        </Tooltip>
        <Tooltip label="Reset full range">
          <ActionIcon size="sm" variant="light" aria-label={`Reset ${labelPrefix} chart range`} onClick={handleReset}>
            <RotateCcw size={14} />
          </ActionIcon>
        </Tooltip>
      </Group>
    </>
  );
}
