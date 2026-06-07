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

const compactLeadBars = 4;
const compactTrailingBars = 24;

function formatChartTime(time: UTCTimestamp) {
  const date = new Date(Number(time) * 1000);
  const hour = date.getUTCHours();
  if (hour === 0) {
    return date.toLocaleDateString("en-US", { month: "short", day: "numeric", timeZone: "UTC" });
  }
  return `${String(hour).padStart(2, "0")}:00`;
}

function isEventWindowAligned(candles: CandlePoint[], events: MarketEvent[]) {
  if (!candles.length || !events.length) return true;
  const firstCandle = candles[0].time;
  const lastCandle = candles[candles.length - 1].time;
  return events.some((event) => event.time >= firstCandle && event.time <= lastCandle);
}

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
  const eventsAligned = isEventWindowAligned(candles, events);

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
        timeVisible: true,
        secondsVisible: false,
        tickMarkFormatter: (time: UTCTimestamp) => formatChartTime(time),
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
      const replayWindow = candles.length
        ? replay.slice(0, candles.length).map((point, index) => ({ ...point, time: candles[index]?.time }))
        : replay;
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
        replayWindow.map((point) => ({
          time: point.time,
          value: point.equity,
        })).filter((point): point is { time: UTCTimestamp; value: number } => point.time !== undefined),
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
        replayWindow.map((point) => ({
          time: point.time,
          value: point.drawdown,
          color: "#8b5cf680",
        })).filter((point): point is { time: UTCTimestamp; value: number; color: string } => point.time !== undefined),
      );

    }
    applyPaneLayout();

    const selectedEvent = events.find((event) => event.id === selectedEventId);
    const selectedCandleIndex = selectedEvent
      ? candles.findIndex((candle) => candle.time >= selectedEvent.time)
      : -1;

    if (compact && selectedCandleIndex >= 0) {
      chart.timeScale().setVisibleLogicalRange({
        from: selectedCandleIndex - compactLeadBars,
        to: selectedCandleIndex + compactTrailingBars,
      });
    } else {
      chart.timeScale().fitContent();
    }

    let disposed = false;
    const updateEventRails = () => {
      if (disposed) return;

      const nextRails = events.flatMap((event, index) => {
        const fallbackTime = candles.length
          ? candles[Math.min(candles.length - 1, Math.round(((index + 1) / (events.length + 1)) * (candles.length - 1)))]?.time
          : undefined;
        const coordinateTime = eventsAligned ? event.time : fallbackTime;
        if (coordinateTime === undefined) return [];

        const coordinate = chart.timeScale().timeToCoordinate(coordinateTime);
        if (coordinate === null) return [];
        if (coordinate < 0 || coordinate > container.clientWidth) return [];

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
  }, [candles, compact, events, eventsAligned, replay, selectedEventId]);

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
