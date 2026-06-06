import { useEffect, useRef } from "react";
import {
  ColorType,
  HistogramSeries,
  LineSeries,
  createChart,
  type IChartApi,
  type UTCTimestamp,
} from "lightweight-charts";
import { formatNumber, formatPercent } from "../utils/format";

export interface EquityDrawdownPoint {
  time: UTCTimestamp;
  label: string;
  equity: number;
  drawdown: number;
}

export function EquityDrawdownChart({ data }: { data: EquityDrawdownPoint[] }) {
  const containerRef = useRef<HTMLDivElement | null>(null);

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
      },
      grid: {
        vertLines: { color: "#e4e8ee" },
        horzLines: { color: "#e4e8ee" },
      },
      rightPriceScale: {
        borderColor: "#d4dae3",
      },
      timeScale: {
        borderColor: "#d4dae3",
        timeVisible: false,
      },
      crosshair: {
        vertLine: { color: "#2768ce", labelBackgroundColor: "#2768ce" },
        horzLine: { color: "#2768ce", labelBackgroundColor: "#2768ce" },
      },
    });

    const equitySeries = chart.addSeries(LineSeries, {
      color: "#2768ce",
      lineWidth: 2,
      priceFormat: {
        type: "custom",
        formatter: formatNumber,
      },
      title: "Equity (USDC)",
      priceLineVisible: true,
      lastValueVisible: true,
    });
    equitySeries.setData(data.map((point) => ({ time: point.time, value: point.equity })));

    const drawdownSeries = chart.addSeries(
      HistogramSeries,
      {
        color: "#8b5cf6",
        base: 0,
        priceFormat: {
          type: "custom",
          formatter: formatPercent,
        },
        priceLineVisible: true,
        title: "Drawdown (%)",
      },
      1,
    );
    drawdownSeries.setData(
      data.map((point) => ({
        time: point.time,
        value: point.drawdown,
        color: "#8b5cf680",
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
    };
  }, [data]);

  return <div ref={containerRef} className="equity-drawdown-chart" />;
}
