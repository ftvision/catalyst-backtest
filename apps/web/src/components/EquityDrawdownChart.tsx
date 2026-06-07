import { useEffect, useRef } from "react";
import {
  AreaSeries,
  BaselineSeries,
  ColorType,
  createChart,
  type IChartApi,
  type UTCTimestamp,
} from "lightweight-charts";
import { ChartInteractionControls } from "./ChartInteractionControls";
import { formatNumber, formatPercent } from "../utils/format";

export interface EquityDrawdownPoint {
  time: UTCTimestamp;
  label: string;
  equity: number;
  drawdown: number;
}

function formatChartTime(time: UTCTimestamp) {
  const date = new Date(Number(time) * 1000);
  const hour = date.getUTCHours();
  if (hour === 0) {
    return date.toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
  }
  return `${String(hour).padStart(2, "0")}:00`;
}

const equityColor = "#2768ce";
const equityFillTop = "rgba(39, 104, 206, 0.18)";
const equityFillBottom = "rgba(39, 104, 206, 0.00)";
const drawdownColor = "#d64a45";
const drawdownFillTop = "rgba(214, 74, 69, 0.00)";
const drawdownFillBottom = "rgba(214, 74, 69, 0.32)";

export function EquityDrawdownChart({ data }: { data: EquityDrawdownPoint[] }) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);

  const resetRange = () => {
    chartRef.current?.timeScale().fitContent();
  };

  useEffect(() => {
    if (!containerRef.current) return;

    const container = containerRef.current;
    const chart: IChartApi = createChart(container, {
      height: container.clientHeight,
      width: container.clientWidth,
      autoSize: false,
      layout: {
        background: { type: ColorType.Solid, color: "#fbfcfd" },
        textColor: "#6b7280",
        panes: {
          enableResize: true,
          separatorColor: "#d4dae3",
          separatorHoverColor: "rgba(39, 104, 206, 0.08)",
        },
      },
      grid: {
        vertLines: { color: "#e4e8ee" },
        horzLines: { color: "#e4e8ee" },
      },
      rightPriceScale: {
        borderColor: "#d4dae3",
      },
      handleScroll: {
        mouseWheel: false,
        pressedMouseMove: true,
        horzTouchDrag: true,
      },
      handleScale: {
        axisPressedMouseMove: true,
        mouseWheel: false,
        pinch: true,
      },
      timeScale: {
        borderColor: "#d4dae3",
        timeVisible: true,
        secondsVisible: false,
        tickMarkFormatter: (time: UTCTimestamp) => formatChartTime(time),
      },
      crosshair: {
        vertLine: { color: equityColor, labelBackgroundColor: equityColor },
        horzLine: { color: equityColor, labelBackgroundColor: equityColor },
      },
    });
    chartRef.current = chart;

    const equitySeries = chart.addSeries(AreaSeries, {
      lineColor: equityColor,
      lineWidth: 2,
      topColor: equityFillTop,
      bottomColor: equityFillBottom,
      priceFormat: {
        type: "custom",
        formatter: formatNumber,
      },
      priceLineColor: equityColor,
      title: "Equity (USDC)",
      priceLineVisible: true,
      lastValueVisible: true,
    });
    equitySeries.setData(data.map((point) => ({ time: point.time, value: point.equity })));

    const drawdownSeries = chart.addSeries(
      BaselineSeries,
      {
        baseValue: { type: "price", price: 0 },
        topLineColor: drawdownColor,
        topFillColor1: drawdownFillTop,
        topFillColor2: drawdownFillTop,
        bottomLineColor: drawdownColor,
        bottomFillColor1: drawdownFillTop,
        bottomFillColor2: drawdownFillBottom,
        lineWidth: 1,
        priceFormat: {
          type: "custom",
          formatter: formatPercent,
        },
        priceLineColor: drawdownColor,
        priceLineVisible: true,
        title: "Drawdown (%)",
      },
      1,
    );
    drawdownSeries.setData(
      data.map((point) => ({
        time: point.time,
        value: point.drawdown,
      })),
    );

    const panes = chart.panes();
    panes[0]?.setHeight(245);
    panes[1]?.setHeight(105);
    chart.timeScale().fitContent();

    const resizeObserver = new ResizeObserver(([entry]) => {
      chart.resize(entry.contentRect.width, entry.contentRect.height);
    });
    resizeObserver.observe(container);

    return () => {
      resizeObserver.disconnect();
      chart.remove();
      chartRef.current = null;
    };
  }, [data]);

  return (
    <div className="chart-shell result-chart-shell">
      <div ref={containerRef} className="equity-drawdown-chart" />
      <ChartInteractionControls ariaLabel="Equity chart controls" chartRef={chartRef} labelPrefix="equity" resetRange={resetRange} />
    </div>
  );
}
