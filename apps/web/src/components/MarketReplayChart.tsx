import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  CandlestickSeries,
  ColorType,
  HistogramSeries,
  LineSeries,
  createChart,
  type IChartApi,
  type IRange,
  type Time,
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
const secondsPerDay = 86_400;
const overviewGranularityThresholdSeconds = secondsPerDay * 35;
const overviewGranularityMinCandles = 720;

type ChartGranularity = "1h" | "1d";

function dayStart(time: UTCTimestamp): UTCTimestamp {
  return (Math.floor(Number(time) / secondsPerDay) * secondsPerDay) as UTCTimestamp;
}

function timeToSeconds(time: Time): number {
  if (typeof time === "number") return time;
  if (typeof time === "string") return Date.parse(time) / 1000;
  return Date.UTC(time.year, time.month - 1, time.day) / 1000;
}

function rangeSeconds(range: IRange<Time> | null) {
  if (!range) return 0;
  return Math.max(0, timeToSeconds(range.to) - timeToSeconds(range.from));
}

function aggregateCandlesByDay(candles: CandlePoint[]) {
  const buckets = new Map<number, CandlePoint>();
  for (const candle of candles) {
    const bucketTime = dayStart(candle.time);
    const bucket = buckets.get(bucketTime);
    if (!bucket) {
      buckets.set(bucketTime, { ...candle, time: bucketTime });
      continue;
    }
    bucket.high = Math.max(bucket.high, candle.high);
    bucket.low = Math.min(bucket.low, candle.low);
    bucket.close = candle.close;
    bucket.volume += candle.volume;
  }
  return Array.from(buckets.values());
}

function alignReplayToCandles(replay: ReplayPoint[], candles: CandlePoint[]) {
  if (!candles.length) return replay;
  return replay.slice(0, candles.length).map((point, index) => ({ ...point, time: candles[index]?.time }));
}

function aggregateReplayByDay(replay: ReplayPoint[]) {
  const buckets = new Map<number, ReplayPoint>();
  for (const point of replay) {
    if (point.time === undefined) continue;
    const bucketTime = dayStart(point.time);
    const bucket = buckets.get(bucketTime);
    if (!bucket) {
      buckets.set(bucketTime, { ...point, time: bucketTime });
      continue;
    }
    bucket.equity = point.equity;
    bucket.drawdown = Math.min(bucket.drawdown, point.drawdown);
    bucket.gas += point.gas;
    bucket.funding = point.funding;
  }
  return Array.from(buckets.values());
}

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
  const [displayedGranularity, setDisplayedGranularity] = useState<ChartGranularity>("1h");
  const eventsAligned = isEventWindowAligned(candles, events);

  useEffect(() => {
    if (!containerRef.current) return;

    const container = containerRef.current;
    const dailyCandles = aggregateCandlesByDay(candles);
    const hourlyReplay = alignReplayToCandles(replay, candles);
    const dailyReplay = aggregateReplayByDay(hourlyReplay);
    const shouldUseAdaptiveGranularity =
      !compact &&
      candles.length >= overviewGranularityMinCandles &&
      candles.length > dailyCandles.length &&
      Number(candles.at(-1)?.time ?? 0) - Number(candles[0]?.time ?? 0) > overviewGranularityThresholdSeconds;
    let activeGranularity: ChartGranularity = shouldUseAdaptiveGranularity ? "1d" : "1h";
    let applyingGranularity = false;

    const candlesForGranularity = (granularity: ChartGranularity) =>
      granularity === "1d" && shouldUseAdaptiveGranularity ? dailyCandles : candles;
    const replayForGranularity = (granularity: ChartGranularity) =>
      granularity === "1d" && shouldUseAdaptiveGranularity ? dailyReplay : hourlyReplay;
    const eventTimeForGranularity = (event: MarketEvent) =>
      activeGranularity === "1d" && shouldUseAdaptiveGranularity ? dayStart(event.time) : event.time;
    const fallbackCandlesForGranularity = () => candlesForGranularity(activeGranularity);

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
    chart.priceScale("volume").applyOptions({
      scaleMargins: {
        top: 0.78,
        bottom: 0,
      },
    });

    let setReplaySeriesData = (_granularity: ChartGranularity) => {};

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
      setReplaySeriesData = (granularity: ChartGranularity) => {
        const replayWindow = replayForGranularity(granularity);
        equitySeries.setData(
          replayWindow.map((point) => ({
            time: point.time,
            value: point.equity,
          })).filter((point): point is { time: UTCTimestamp; value: number } => point.time !== undefined),
        );
        drawdownSeries.setData(
          replayWindow.map((point) => ({
            time: point.time,
            value: point.drawdown,
            color: "#8b5cf680",
          })).filter((point): point is { time: UTCTimestamp; value: number; color: string } => point.time !== undefined),
        );
      };

    }

    const setSeriesData = (granularity: ChartGranularity) => {
      const displayCandles = candlesForGranularity(granularity);
      candleSeries.setData(displayCandles.map(({ time, open, high, low, close }) => ({ time, open, high, low, close })));
      volumeSeries.setData(
        displayCandles.map((candle) => ({
          time: candle.time,
          value: candle.volume,
          color: candle.close >= candle.open ? "#16847766" : "#bd3f3866",
        })),
      );
      setReplaySeriesData(granularity);
      setDisplayedGranularity(granularity);
    };

    const desiredGranularity = (range: IRange<Time> | null): ChartGranularity => {
      if (!shouldUseAdaptiveGranularity) return "1h";
      return rangeSeconds(range) > overviewGranularityThresholdSeconds ? "1d" : "1h";
    };

    const applyGranularity = (nextGranularity: ChartGranularity, visibleRange: IRange<Time> | null) => {
      if (nextGranularity === activeGranularity) return false;
      applyingGranularity = true;
      activeGranularity = nextGranularity;
      setSeriesData(nextGranularity);
      if (visibleRange) {
        chart.timeScale().setVisibleRange(visibleRange);
      }
      window.requestAnimationFrame(() => {
        applyingGranularity = false;
      });
      return true;
    };

    setSeriesData(activeGranularity);
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
        const fallbackCandles = fallbackCandlesForGranularity();
        const fallbackTime = fallbackCandles.length
          ? fallbackCandles[
              Math.min(fallbackCandles.length - 1, Math.round(((index + 1) / (events.length + 1)) * (fallbackCandles.length - 1)))
            ]?.time
          : undefined;
        const coordinateTime = eventsAligned ? eventTimeForGranularity(event) : fallbackTime;
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

    const handleVisibleRangeChange = (range: IRange<Time> | null) => {
      if (!applyingGranularity && applyGranularity(desiredGranularity(range), range)) {
        window.requestAnimationFrame(updateEventRails);
        return;
      }
      updateEventRails();
    };
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
      {!compact && candles.length ? <div className="chart-granularity-badge">{displayedGranularity} candles</div> : null}
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
