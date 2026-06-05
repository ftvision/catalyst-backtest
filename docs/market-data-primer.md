# Market Data Primer

This document explains the data we need for crypto backtesting, where that data
can come from, and why provider choice matters.

## What Is Market Data?

Market data is historical information about prices, trades, liquidity, funding,
gas, and protocol rates.

For a backtester, market data answers questions like:

```text
What was ETH worth at 2024-03-01 12:00 UTC?
Did ETH cross below 1900 during this hour?
What funding rate did ETH perps charge?
How much gas would an EVM transaction have cost?
What was the Aave USDC supply rate?
Was there enough liquidity to execute this swap?
```

The simulation engine should not fetch raw data directly. It should receive a
normalized market data bundle from the Python market-data package.

## Levels Of Price Data

### Candle Data

Candle data compresses market activity into fixed time intervals.

OHLCV means:

```text
O = open price
H = high price
L = low price
C = close price
V = volume
```

Example:

```json
{
  "symbol": "ETH",
  "interval": "1h",
  "timestamp": "2024-01-01T12:00:00Z",
  "open": "2200",
  "high": "2250",
  "low": "2180",
  "close": "2230",
  "volume": "12345"
}
```

Pros:

- easy to fetch
- compact
- good enough for first MVP

Cons:

- does not show exact intrabar sequence
- does not prove that an order would have filled
- can hide volatility inside the interval

Backtest implication:

If a 1h candle has:

```text
high: 2300
low: 1800
```

we do not know whether price went up first or down first. This matters if one
signal buys below 1900 and another sells above 2200 in the same candle.

### Trade Data

Trade data records individual executed trades.

Pros:

- more realistic timing
- better for threshold crossings
- better for execution analysis

Cons:

- much larger data volume
- harder to source historically
- can still miss order-book liquidity

### Order Book Data

Order book data records resting buy and sell orders.

Pros:

- best for modeling market orders and slippage
- can simulate large orders more realistically

Cons:

- expensive to store
- often expensive to acquire
- complex to replay correctly

For the first version, use candles. Later, use order-book data for high-value
Hyperliquid execution modeling.

## Price Types

### Last Price

The price of the most recent trade.

### Mid Price

The midpoint between best bid and best ask:

```text
mid = (best bid + best ask) / 2
```

### Index Price

A reference price, often aggregated from multiple markets. Perp venues may use
index prices for funding or risk calculations.

### Mark Price

A venue-defined price used for unrealized PnL, margin, and liquidation logic.

For perps, the mark price may matter more than the last traded price.

## Data Needed By Action Type

### Spot Swap

Minimum MVP data:

- asset price candle
- fee model
- slippage assumption

Better data:

- bid/ask spread
- venue-specific fees
- DEX pool reserves/liquidity
- order-book snapshots
- actual historical trades

### EVM Swap

Minimum MVP data:

- token price candle
- gas price
- fixed slippage

Better data:

- exact DEX pool state
- route path
- liquidity
- swap fee tier
- block-by-block gas
- MEV or priority fee assumptions

Important: an EVM swap is venue-specific. "Swap on Base" is underspecified
unless we know whether the simulated venue is Uniswap, Aerodrome, an aggregator,
or some other route.

### Hyperliquid Spot

Useful data:

- spot candles
- spot metadata
- fee schedule
- order-book snapshots, if available and needed

Hyperliquid's official info endpoint includes exchange and market data endpoints,
including candles, L2 book snapshots, user fills, and metadata.

### Hyperliquid Perps

Useful data:

- perp candles
- mark/index price, if available
- funding history
- asset metadata
- margin/leverage constraints
- fee schedule
- liquidation rules

Funding is especially important because it is a separate source of PnL.

### Aave / Yield

Useful data:

- historical supply rate
- deposit/withdraw timestamps
- protocol liquidity
- gas cost
- receipt-token mechanics, if modeled

For MVP, we can approximate yield as time-weighted accrual using historical rates.

## Provider Categories

### Exchange APIs

Exchange APIs provide data directly from a trading venue.

Examples:

- Hyperliquid API
- Coinbase API
- Binance API

Best for:

- venue-specific prices
- spot/perp candles
- funding rates
- order book snapshots
- fills and order status

Risk:

- each exchange has its own symbols, intervals, limits, and historical depth
- data may be paginated or rate-limited

For this project, Hyperliquid official API should be the first source for
Hyperliquid spot and perp markets.

### Aggregated Market Data APIs

Aggregators combine data across many venues.

Examples:

- CoinGecko
- CoinMarketCap
- Kaiko
- CoinAPI

Best for:

- broad token price coverage
- fallback price history
- market cap and volume
- cross-exchange reference prices

Risk:

- aggregate prices may not match the exact venue where the strategy trades
- historical granularity may depend on plan or endpoint
- symbol mapping can be tricky

CoinGecko documents historical price, volume, market cap, exchange, derivatives,
and on-chain pool/trade data products.

### DeFi Aggregators

DeFi data aggregators track on-chain protocols.

Examples:

- DefiLlama
- DeFi Pulse-style datasets

Best for:

- protocol TVL
- yield pools
- token prices
- broad DeFi discovery

Risk:

- useful for analytics, but may not be precise enough for execution simulation
- provider methodology can differ from protocol-native data

DefiLlama is useful as a fallback or discovery source for prices, TVL, and yield
data.

### Blockchain RPC Providers

RPC providers expose raw blockchain data.

Examples:

- Alchemy
- QuickNode
- Infura
- public Base RPC endpoints

Best for:

- blocks
- transactions
- logs/events
- gas price history
- contract calls at historical blocks, if supported

Risk:

- raw data needs decoding and indexing
- historical archive access may require paid plans
- high-volume queries are expensive and slow without caching

For Base/EVM gas, RPC data is the right primitive.

### Indexers And Subgraphs

Indexers process raw blockchain events into queryable datasets.

Examples:

- The Graph subgraphs
- Goldsky
- Subsquid
- Dune
- Flipside

Best for:

- protocol-level historical state
- pool swaps
- reserves
- lending rates
- decoded events

Risk:

- indexing lag
- subgraph schemas differ by protocol
- historical state may be approximate or derived
- queries can be expensive or rate-limited

Aave's protocol subgraphs index Aave smart-contract data and expose GraphQL
endpoints through The Graph.

### Protocol-Native APIs

Some protocols expose their own APIs or SDKs.

Best for:

- protocol-specific semantics
- official metadata
- app-like data

Risk:

- API may be optimized for the app, not backtesting
- historical coverage may be limited
- data may not expose every contract-level detail

## Data Source Recommendations For MVP

| Need | Preferred first source | Fallback |
| --- | --- | --- |
| Hyperliquid spot candles | Hyperliquid official API | CoinGecko if only reference price is needed |
| Hyperliquid perp candles | Hyperliquid official API | none, because venue-specific perps matter |
| Hyperliquid funding | Hyperliquid official API | none |
| EVM token price | CoinGecko or DefiLlama | DEX pool subgraph |
| Base gas | Base/EVM RPC | fixed gas model |
| Aave USDC yield | Aave subgraph | DefiLlama yield data |
| DEX swap realism | Uniswap/Aerodrome subgraph | fixed slippage model |

## Normalized Data Bundle

The Rust simulation service should receive normalized data, not provider-specific
responses.

Example shape:

```json
{
  "candles": [
    {
      "venue": "hyperliquid",
      "symbol": "ETH",
      "interval": "1h",
      "timestamp": "2024-01-01T00:00:00Z",
      "open": "2200",
      "high": "2230",
      "low": "2180",
      "close": "2210",
      "volume": "10000"
    }
  ],
  "funding_rates": [
    {
      "venue": "hyperliquid",
      "symbol": "ETH",
      "timestamp": "2024-01-01T00:00:00Z",
      "rate": "0.0001"
    }
  ],
  "gas": [
    {
      "chain": "base",
      "timestamp": "2024-01-01T00:00:00Z",
      "gas_price_gwei": "0.42"
    }
  ],
  "yield_rates": [
    {
      "chain": "base",
      "protocol": "aave",
      "asset": "USDC",
      "timestamp": "2024-01-01T00:00:00Z",
      "supply_apr": "0.052"
    }
  ]
}
```

## Common Data Problems

### Symbol Mapping

Different providers may use different identifiers.

Example:

```text
ETH
ethereum
WETH
ETH/USDC
@107
```

The market-data package needs a symbol registry.

### Venue Mismatch

ETH price on one venue may not equal ETH price on another venue.

For rough backtests, an aggregate price is acceptable. For serious execution
simulation, use the venue where the action occurs.

### Missing Data

Historical data may be incomplete.

Backtest result should say:

```text
Data coverage: 97.2%
Missing period: 2024-03-02 04:00 -> 2024-03-02 07:00
Fallback used: CoinGecko close price
```

### Coarse Granularity

A 1h backtest cannot safely answer every question.

Example:

```text
ETH low = 1800
ETH high = 2300
```

If both buy and sell thresholds are hit in the same candle, the simulator must
choose a rule or flag ambiguity.

### Corporate-Action Equivalent Events

Crypto assets can have:

- token migrations
- redenominations
- delistings
- chain splits
- contract upgrades
- stablecoin depegs

These events can break naive price history assumptions.

### Provider Rate Limits

Most APIs limit request frequency or historical range.

The data package should:

- cache aggressively
- paginate carefully
- record provider and fetch timestamp
- store raw response metadata when useful

## Backtest Result Should Show Data Assumptions

Every result should disclose:

- data providers used
- interval/granularity
- missing data
- fallback data
- slippage model
- gas model
- funding model
- yield model
- whether execution was candle-based, trade-based, or order-book-based

This is not just UI polish. It is part of correctness.

## Sources

- Hyperliquid API docs: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint
- CoinGecko API: https://www.coingecko.com/en/api
- DefiLlama API docs: https://defillama.com/docs/api
- Aave protocol subgraphs: https://github.com/aave/protocol-subgraphs

