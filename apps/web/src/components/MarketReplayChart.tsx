import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  CandlestickSeries,
  ColorType,
  HistogramSeries,
  LineSeries,
  createChart,
  type IChartApi,
  type UTCTimestamp,
} from "lightweight-charts";
import type { CandlePoint, MarketEvent, ReplayPoint } from "../types";
import { formatNumber, formatPercent } from "../utils/format";

interface EventRail {
  id: string;
  left: number;
  label: string;
  node: string;
  status: MarketEvent["status"];
  selected: boolean;
}

const markerColor = {
  signal: "#2768ce",
  executed: "#168477",
  rejected: "#bd3f38",
  policy: "#7a55c7",
  warning: "#b7791f",
};

const paneStretch = {
  market: 58,
  equity: 29,
  drawdown: 13,
};

export function MarketReplayChart({
  candles,
  replay,
  events,
  selectedEventId,
  compact = false,
}: {
  candles: CandlePoint[];
  replay: ReplayPoint[];
  events: MarketEvent[];
  selectedEventId?: string;
  compact?: boolean;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [eventRails, setEventRails] = useState<EventRail[]>([]);

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
      leftPriceScale: {
        visible: true,
        borderColor: "#d4dae3",
      },
      timeScale: {
        borderColor: "#d4dae3",
      },
      crosshair: {
        vertLine: { color: "#2768ce", labelBackgroundColor: "#2768ce" },
        horzLine: { color: "#2768ce", labelBackgroundColor: "#2768ce" },
      },
    });

    const applyPaneLayout = () => {
      if (compact) return;
      const panes = chart.panes();
      panes[0]?.setStretchFactor(paneStretch.market);
      panes[1]?.setStretchFactor(paneStretch.equity);
      panes[2]?.setStretchFactor(paneStretch.drawdown);
    };

    const candleSeries = chart.addSeries(CandlestickSeries, {
      upColor: "#168477",
      downColor: "#bd3f38",
      wickUpColor: "#168477",
      wickDownColor: "#bd3f38",
      borderVisible: false,
      priceLineColor: "#168477",
      priceFormat: {
        type: "custom",
        formatter: formatNumber,
      },
      title: "Market data",
    });
    candleSeries.setData(candles.map(({ time, open, high, low, close }) => ({ time, open, high, low, close })));

    const volumeSeries = chart.addSeries(HistogramSeries, {
      color: "#78909c",
      priceFormat: {
        type: "volume",
      },
      priceScaleId: "volume",
      priceLineVisible: false,
      lastValueVisible: false,
      title: "Volume",
    });
    volumeSeries.setData(
      candles.map((candle) => ({
        time: candle.time,
        value: candle.volume,
        color: candle.close >= candle.open ? "#16847766" : "#bd3f3866",
      })),
    );
    chart.priceScale("volume").applyOptions({
      scaleMargins: {
        top: 0.78,
        bottom: 0,
      },
    });

    if (!compact) {
      const equitySeries = chart.addSeries(
        LineSeries,
        {
          color: "#2768ce",
          lineWidth: 2,
          priceFormat: {
            type: "custom",
            formatter: formatNumber,
          },
          title: "Equity (USDC)",
          priceLineVisible: true,
          lastValueVisible: true,
        },
        1,
      );
      equitySeries.setData(
        replay.map((point, index) => ({
          time: candles[index].time,
          value: point.equity,
        })),
      );

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
        2,
      );
      drawdownSeries.setData(
        replay.map((point, index) => ({
          time: candles[index].time,
          value: point.drawdown,
          color: "#8b5cf680",
        })),
      );

    }
    applyPaneLayout();

    chart.timeScale().fitContent();

    let disposed = false;
    const updateEventRails = () => {
      if (disposed) return;

      const nextRails = events.flatMap((event) => {
        const coordinate = chart.timeScale().timeToCoordinate(event.time);
        if (coordinate === null) return [];

        return [
          {
            id: event.id,
            left: coordinate,
            label: event.kind,
            node: event.node,
            status: event.status,
            selected: event.id === selectedEventId,
          },
        ];
      });
      setEventRails(nextRails);
    };

    const handleVisibleRangeChange = () => updateEventRails();
    const handleSizeChange = () => updateEventRails();
    chart.timeScale().subscribeVisibleTimeRangeChange(handleVisibleRangeChange);
    chart.timeScale().subscribeSizeChange(handleSizeChange);
    window.requestAnimationFrame(updateEventRails);

    const resizeObserver = new ResizeObserver(([entry]) => {
      chart.resize(entry.contentRect.width, entry.contentRect.height);
      applyPaneLayout();
      window.requestAnimationFrame(updateEventRails);
    });
    resizeObserver.observe(container);

    return () => {
      disposed = true;
      chart.timeScale().unsubscribeVisibleTimeRangeChange(handleVisibleRangeChange);
      chart.timeScale().unsubscribeSizeChange(handleSizeChange);
      resizeObserver.disconnect();
      chart.remove();
    };
  }, [candles, compact, events, replay, selectedEventId]);

  return (
    <div className={compact ? "chart-shell compact" : "chart-shell"}>
      <div ref={containerRef} className={compact ? "chart-frame compact" : "chart-frame"} />
      <div className="chart-event-overlay" aria-hidden="true">
        {eventRails.map((rail) => (
          <div
            key={rail.id}
            className={rail.selected ? "chart-event-rail selected" : "chart-event-rail"}
            style={
              {
                left: `${rail.left}px`,
                "--event-color": markerColor[rail.status],
              } as CSSProperties
            }
          >
            <span className="chart-event-dot" />
            {!compact ? (
              <span className="chart-event-label">
                {rail.label}
                <span>{rail.node}</span>
              </span>
            ) : null}
          </div>
        ))}
      </div>
    </div>
  );
}
