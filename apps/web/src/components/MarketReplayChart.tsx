import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import {
  AreaSeries,
  BaselineSeries,
  CandlestickSeries,
  ColorType,
  HistogramSeries,
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

const equityColor = "#2768ce";
const equityFillTop = "rgba(39, 104, 206, 0.18)";
const equityFillBottom = "rgba(39, 104, 206, 0.00)";
const drawdownColor = "#d64a45";
const drawdownFillTop = "rgba(214, 74, 69, 0.00)";
const drawdownFillBottom = "rgba(214, 74, 69, 0.32)";

const compactLeadBars = 4;
const compactTrailingBars = 24;
const secondsPerDay = 86_400;
const wideGranularityThresholdSeconds = secondsPerDay * 35;
const mediumGranularityThresholdSeconds = secondsPerDay * 10;
const adaptiveGranularityMinCandles = 240;
const granularitySeconds = {
  "1h": 3_600,
  "4h": 14_400,
  "1d": secondsPerDay,
};

type ChartGranularity = keyof typeof granularitySeconds;

interface ReplayChartCache {
  candlesByGranularity: Record<ChartGranularity, CandlePoint[]>;
  hourlyReplay: ReplayPoint[];
  replayByGranularity: Record<ChartGranularity, ReplayPoint[]>;
  fullRangeSeconds: number;
}

const replayChartCache = new WeakMap<CandlePoint[], WeakMap<ReplayPoint[], ReplayChartCache>>();

function bucketStart(time: UTCTimestamp, granularity: ChartGranularity): UTCTimestamp {
  const bucketSeconds = granularitySeconds[granularity];
  return (Math.floor(Number(time) / bucketSeconds) * bucketSeconds) as UTCTimestamp;
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

function granularityForRange(visibleSeconds: number): ChartGranularity {
  if (visibleSeconds > wideGranularityThresholdSeconds) return "1d";
  if (visibleSeconds > mediumGranularityThresholdSeconds) return "4h";
  return "1h";
}

function aggregateCandles(candles: CandlePoint[], granularity: ChartGranularity) {
  if (granularity === "1h") return candles;

  const buckets = new Map<number, CandlePoint>();
  for (const candle of candles) {
    const bucketTime = bucketStart(candle.time, granularity);
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

function aggregateReplay(replay: ReplayPoint[], granularity: ChartGranularity) {
  if (granularity === "1h") return replay;

  const buckets = new Map<number, ReplayPoint>();
  for (const point of replay) {
    if (point.time === undefined) continue;
    const bucketTime = bucketStart(point.time, granularity);
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

function buildReplayChartCache(candles: CandlePoint[], replay: ReplayPoint[]): ReplayChartCache {
  const candlesByGranularity: Record<ChartGranularity, CandlePoint[]> = {
    "1h": candles,
    "4h": aggregateCandles(candles, "4h"),
    "1d": aggregateCandles(candles, "1d"),
  };
  const hourlyReplay = alignReplayToCandles(replay, candles);
  const replayByGranularity: Record<ChartGranularity, ReplayPoint[]> = {
    "1h": hourlyReplay,
    "4h": aggregateReplay(hourlyReplay, "4h"),
    "1d": aggregateReplay(hourlyReplay, "1d"),
  };
  return {
    candlesByGranularity,
    hourlyReplay,
    replayByGranularity,
    fullRangeSeconds: Number(candles.at(-1)?.time ?? 0) - Number(candles[0]?.time ?? 0),
  };
}

function getReplayChartCache(candles: CandlePoint[], replay: ReplayPoint[]) {
  let replayCache = replayChartCache.get(candles);
  if (!replayCache) {
    replayCache = new WeakMap<ReplayPoint[], ReplayChartCache>();
    replayChartCache.set(candles, replayCache);
  }

  const cached = replayCache.get(replay);
  if (cached) return cached;

  const nextCache = buildReplayChartCache(candles, replay);
  replayCache.set(replay, nextCache);
  return nextCache;
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

function isEventInCandleWindow(event: MarketEvent, candles: CandlePoint[]) {
  if (!candles.length) return false;
  const firstCandle = candles[0].time;
  const lastCandle = candles[candles.length - 1].time;
  return event.time >= firstCandle && event.time <= lastCandle;
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
  const chartData = useMemo(() => getReplayChartCache(candles, replay), [candles, replay]);

  useEffect(() => {
    if (!containerRef.current) return;

    const container = containerRef.current;
    const { candlesByGranularity, fullRangeSeconds, hourlyReplay, replayByGranularity } = chartData;
    const shouldUseAdaptiveGranularity =
      !compact &&
      candles.length >= adaptiveGranularityMinCandles &&
      candles.length > candlesByGranularity["4h"].length &&
      fullRangeSeconds > mediumGranularityThresholdSeconds;
    let activeGranularity: ChartGranularity = shouldUseAdaptiveGranularity ? granularityForRange(fullRangeSeconds) : "1h";
    let applyingGranularity = false;

    const candlesForGranularity = (granularity: ChartGranularity) =>
      shouldUseAdaptiveGranularity ? candlesByGranularity[granularity] : candles;
    const replayForGranularity = (granularity: ChartGranularity) =>
      shouldUseAdaptiveGranularity ? replayByGranularity[granularity] : hourlyReplay;
    const eventTimeForGranularity = (event: MarketEvent) => {
      const displayCandles = candlesForGranularity(activeGranularity);
      if (!isEventInCandleWindow(event, displayCandles)) return undefined;
      return shouldUseAdaptiveGranularity ? bucketStart(event.time, activeGranularity) : event.time;
    };
    const fallbackCandlesForGranularity = () => candlesForGranularity(activeGranularity);

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
        vertLine: { color: equityColor, labelBackgroundColor: equityColor },
        horzLine: { color: equityColor, labelBackgroundColor: equityColor },
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
        AreaSeries,
        {
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
        },
        1,
      );

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
          })).filter((point): point is { time: UTCTimestamp; value: number } => point.time !== undefined),
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
      if (!range) return activeGranularity;
      return granularityForRange(rangeSeconds(range));
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
  }, [candles, chartData, compact, events, eventsAligned, selectedEventId]);

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
