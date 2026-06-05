# Crypto Trading Primer

This document explains the major crypto concepts that matter for the Catalyst
backtesting system. It is written for readers who know software systems but are
new to crypto markets.

## The Mental Model

Crypto backtesting combines two worlds:

1. **Market trading:** prices, orders, fills, leverage, PnL, fees, liquidity.
2. **Blockchain execution:** wallets, tokens, chains, gas, smart contracts,
   protocol state, on-chain transactions.

For this project, a Catalyst graph describes what a strategy wants to do:

```text
if ETH price < 1900:
  swap 100 USDC into ETH
```

The backtester must answer:

```text
At each point in historical time, would the signal have fired?
If yes, could the action execute?
At what price, with what fees, slippage, gas, funding, and yield?
What did the portfolio look like afterward?
```

## Core Building Blocks

### Chain

A chain is a blockchain network. Examples:

- Ethereum
- Base
- Arbitrum
- Solana
- Hyperliquid HyperCore / HyperEVM

The problem statement focuses on:

- **Base**, an EVM-compatible chain
- **Hyperliquid**, a crypto exchange/chain ecosystem with spot and perpetual
  trading

### EVM

EVM means Ethereum Virtual Machine. EVM-compatible chains run Ethereum-style
smart contracts and use similar transaction mechanics.

For our system, "EVM action" usually means:

- the user has a wallet
- the action happens through a smart contract
- the action consumes gas
- settlement is on-chain
- execution may depend on block time, pool liquidity, and transaction latency

### Token / Asset

A token is a tradable asset. Examples:

- `ETH`
- `USDC`
- `BTC`
- `HYPE`

Some assets are native to a chain, like ETH on Ethereum. Others are tokens issued
by contracts, like USDC on Base.

### Stablecoin

A stablecoin is intended to track a stable reference value, usually USD. `USDC`
is the stablecoin used in the sample graphs.

Backtests often value the final portfolio in USD or USDC terms.

### Wallet

A wallet is an account that controls assets. On EVM chains, a wallet address
looks like:

```text
0x...
```

In a backtest, we do not need a real wallet. We simulate a portfolio ledger:

```text
Base wallet:
  USDC: 1000
  ETH: 0

Hyperliquid account:
  USDC: 1000
  ETH spot: 0
  ETH perp position: none
```

## Trading Venues

### Centralized Exchange-Like Venue

Some crypto venues behave like traditional exchanges:

- users place orders
- the venue has an order book
- trades fill against other orders
- assets and positions are tracked in exchange accounts

Hyperliquid's spot and perp markets should be modeled more like an exchange
venue than like a simple on-chain AMM swap.

### DEX

DEX means decentralized exchange.

On EVM chains, a DEX is usually a smart contract protocol where users swap tokens
directly on-chain. Examples:

- Uniswap
- Aerodrome
- Curve

For a real EVM swap backtest, `chain: "base"` is not enough. We eventually need
to know the route or venue:

```text
Base + Uniswap V3 ETH/USDC pool
Base + Aerodrome ETH/USDC pool
Base + aggregator route
```

For the first version, we can approximate EVM swaps using historical prices,
fixed slippage, and gas. Later we can use pool-level data.

## Types Of Trading Actions

### Swap

A swap exchanges one asset for another.

Example:

```json
{
  "subtype": "swap",
  "config": {
    "from_asset": "USDC",
    "to_asset": "ETH",
    "amount": "100",
    "chain": "base"
  }
}
```

Meaning:

```text
Spend 100 USDC and receive ETH.
```

Backtest implications:

- Which venue executed the swap?
- What historical ETH price should be used?
- Is `amount` denominated in the from asset?
- What were the swap fees?
- How much slippage should be applied?
- Was there enough balance?
- For EVM swaps, how much gas was paid?

### Spot Buy

A spot buy means buying an asset outright.

Example:

```text
Buy 100 USDC worth of ETH.
```

After the trade, the portfolio owns ETH. There is no leverage and no liquidation
risk.

In the sample graphs, spot buys are represented as swaps:

```text
USDC -> ETH
```

### Spot Sell

A spot sell means selling an asset already owned.

Example:

```text
Sell 0.04 ETH for USDC.
```

In the sample graphs, spot sells are also represented as swaps:

```text
ETH -> USDC
```

Backtest implications:

- If the portfolio has less than `0.04 ETH`, should the action fail?
- Should the system allow partial fills?
- What price and slippage model should be used?

For the first version, use strict balance checks and reject insufficient-balance
actions.

### Market Order

A market order asks to trade immediately at the best available price.

Pros:

- high chance of execution
- simple user intent

Cons:

- execution price can be worse than expected
- large orders can move through the order book

For an MVP, market orders can be approximated with:

```text
fill price = candle price +/- slippage
```

For more realistic simulation, we need order-book snapshots or trade-level data.

### Limit Order

A limit order sets a maximum buy price or minimum sell price.

Example:

```text
Buy ETH only if price is <= 1900.
```

Limit orders are harder to backtest from coarse candles because a candle may show
that the price touched a level, but not whether the user's order would actually
have filled.

The current problem statement mostly uses market-like actions triggered by
signals, not true resting limit orders.

## Perpetual Futures

### Perp

Perp means perpetual futures contract. It lets a trader take price exposure
without owning the underlying asset.

Example:

```text
Open a 5x ETH long.
```

This means the trader profits if ETH rises and loses if ETH falls.

### Long

A long position benefits when the asset price rises.

Example:

```text
ETH entry price: 2000
ETH exit price: 2200
Long position PnL: positive
```

### Short

A short position benefits when the asset price falls.

Example:

```text
ETH entry price: 2000
ETH exit price: 1800
Short position PnL: positive
```

### Leverage

Leverage means taking a position larger than the collateral committed.

Example:

```text
Collateral: 100 USDC
Leverage: 5x
Position exposure: 500 USDC
```

Leverage amplifies both gains and losses.

Backtest implication:

```text
small price move * leverage = larger equity impact
```

### Margin / Collateral

Margin is the capital used to support a leveraged position.

Example:

```text
Open 500 USDC notional ETH long at 5x.
Required margin is roughly 100 USDC, before venue-specific rules.
```

The exact margin model depends on the venue.

### Liquidation

Liquidation happens when losses consume too much of the margin supporting a
position. The venue forcibly closes the position.

For a backtester, liquidation checks are essential for perps. A strategy can look
profitable in a naive simulation while actually being liquidated during a drawdown.

MVP behavior:

```text
At every tick, check whether each perp position breaches liquidation rules.
If yes, close it at a modeled liquidation price and record a liquidation event.
```

### Funding

Perpetual futures do not expire. Funding payments help keep the perp price close
to the spot/index price.

Depending on market conditions:

- longs may pay shorts
- shorts may pay longs

Backtest implications:

- funding can materially change PnL
- historical funding rates are required for realistic perp results
- funding should be recorded separately from trading PnL

### Reduce-Only

A reduce-only order can only reduce an existing position. It cannot increase or
flip exposure.

Example:

```json
{
  "side": "short",
  "reduce_only": true
}
```

If the account currently has an ETH long, a short reduce-only order closes or
reduces that long.

If there is no long to reduce, the action should be rejected.

## Yield Actions

Yield actions are not trades in the same sense as swaps or perps. They are DeFi
protocol actions that move assets into or out of a yield-generating position.

### Deposit

Deposit means putting assets into a protocol.

Example:

```text
Deposit 250 USDC into Aave on Base.
```

After deposit:

- liquid USDC balance decreases
- protocol position increases
- the position may earn yield over time

### Withdraw

Withdraw means taking assets out of a protocol.

Example:

```text
Withdraw 100 USDC from Aave.
```

Backtest implications:

- did the user have enough deposited balance?
- how much yield accrued before withdrawal?
- were there gas costs?
- was liquidity available in the protocol?

For the MVP, we can model Aave-style deposits as:

```text
deposited principal accrues at historical supply APY/APR
```

Later, we can model tokenized receipt assets and protocol-specific edge cases.

## Signals

### Price Threshold Signal

Example:

```json
{
  "subtype": "price_threshold",
  "config": {
    "symbol": "ETH",
    "operator": "<",
    "threshold": "1900"
  }
}
```

Meaning:

```text
The signal is true when ETH price is below 1900.
```

Backtest ambiguity:

```text
If ETH stays below 1900 for 12 hours, should the action execute once or 12 times?
```

Recommended default:

```text
Use crossing semantics.
```

That means:

```text
false -> true: fire once
true -> true: do not fire again
true -> false: reset
```

If repeated behavior is desired, the graph or backtest config should specify it
explicitly.

## Costs And Realism

### Fee

A fee is charged by a trading venue or protocol.

Examples:

- exchange trading fee
- DEX pool fee
- protocol fee

### Gas

Gas is the transaction cost paid to execute on-chain actions.

EVM swaps and Aave deposits/withdrawals need gas modeling.

Hyperliquid exchange-style actions do not use EVM gas in the same way, though
they may have venue fees or other chain-specific costs.

### Slippage

Slippage is the difference between expected price and actual execution price.

Reasons for slippage:

- low liquidity
- large order size
- volatile market
- coarse backtest data
- DEX pool price impact

### Spread

Spread is the difference between the best buy and sell prices.

If the market is:

```text
best bid: 1999
best ask: 2001
```

Then buying immediately likely fills near `2001`, while selling immediately
likely fills near `1999`.

### Liquidity

Liquidity is the market's ability to absorb trades without large price movement.

Low liquidity makes fills worse and makes historical candle-only backtests less
trustworthy.

## What The Backtester Must Track

At minimum:

- cash balances
- token balances
- open perp positions
- yield positions
- realized PnL
- unrealized PnL
- fees
- gas
- funding payments
- yield earned
- rejected actions
- liquidation events
- assumptions used

## Glossary

| Term | Meaning |
| --- | --- |
| AMM | Automated market maker, a common DEX design |
| APY | Annual percentage yield, often includes compounding |
| APR | Annual percentage rate, usually non-compounded |
| Candle | OHLCV price summary for a time interval |
| Collateral | Capital backing a leveraged position |
| DEX | Decentralized exchange |
| EVM | Ethereum Virtual Machine |
| Fill | The actual execution of an order |
| Funding | Periodic payment between perp longs and shorts |
| Gas | On-chain transaction fee |
| Leverage | Position size divided by margin/collateral |
| Liquidation | Forced close due to insufficient margin |
| Long | Position that profits when price rises |
| Mark price | Venue price used for margin/PnL calculations |
| OHLCV | Open, high, low, close, volume |
| Perp | Perpetual futures contract |
| Short | Position that profits when price falls |
| Slippage | Difference between expected and actual execution price |
| Spot | Owning or selling the asset directly |
| Stablecoin | Token intended to track a stable value, usually USD |

## Sources

- Hyperliquid API docs: https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint
- Aave protocol subgraphs: https://github.com/aave/protocol-subgraphs
- CoinGecko API: https://www.coingecko.com/en/api
- DefiLlama API docs: https://defillama.com/docs/api

