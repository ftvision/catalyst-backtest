# Chains, Ledgers, And Venues

This document answers foundational questions about chains, assets, ledgers, perps,
and AMM swaps.

## What Is A Chain?

A **chain** is a blockchain network: a shared database plus rules for updating
that database.

Examples:

- Ethereum
- Base
- Arbitrum
- Hyperliquid
- Solana

For a backtester, the chain matters because it changes:

- where balances live
- what fees are paid
- how transactions settle
- what data source we need
- what execution model we should use

Example:

```text
Swap 100 USDC to ETH on Base
```

is not the same as:

```text
Swap 100 USDC to ETH on Hyperliquid
```

Even if both actions mention `USDC` and `ETH`, they settle in different places
with different mechanics.

## What Is EVM?

EVM means **Ethereum Virtual Machine**.

It is the execution environment that runs Ethereum-style smart contracts. A chain
is called **EVM-compatible** if Ethereum-style smart contracts and developer
tools mostly work there too.

Simple mental model:

```text
EVM = operating system for Ethereum-style smart contracts
```

Examples of EVM or EVM-compatible chains:

- Ethereum
- Base
- Arbitrum
- Optimism
- Polygon
- BNB Smart Chain
- HyperEVM

Why it matters for us:

- EVM actions use smart contracts.
- EVM actions usually pay gas.
- EVM swaps often happen through AMMs like Uniswap or Aerodrome.
- EVM yield actions can interact with protocols like Aave.

## What Is Base?

**Base** is an Ethereum layer-2 chain incubated by Coinbase.

Layer-2 means it is a separate execution environment designed to make Ethereum
usage cheaper and faster while still settling back to Ethereum.

For our purposes:

```text
Base = an EVM-compatible chain where users can hold tokens, swap on DEXs,
deposit into DeFi protocols, and pay gas in ETH.
```

In the Catalyst graphs, this appears as:

```json
{
  "chain": "base"
}
```

Backtest implication:

- use EVM-style gas modeling
- use Base token prices/liquidity
- use Base protocol data, such as Aave on Base
- if simulating real swaps, choose a Base DEX/route

## What Is Arbitrum?

**Arbitrum** is another Ethereum layer-2 ecosystem.

Like Base, it is EVM-compatible and is designed to scale Ethereum activity.

For our purposes:

```text
Arbitrum = another EVM chain, with its own apps, liquidity, gas costs,
and token balances.
```

If we later support Arbitrum, we should not assume Base data applies to Arbitrum.
The same token pair can have different liquidity, prices, gas costs, and DeFi
rates across chains.

## What Is Hyperliquid?

Hyperliquid is a blockchain and trading venue focused on onchain finance.

The important split:

```text
Hyperliquid
  HyperCore: trading engine, spot order books, perp order books, margin state
  HyperEVM: EVM smart-contract environment on the same Hyperliquid chain
```

For backtesting, this distinction is huge.

### HyperCore

**HyperCore** is where Hyperliquid's native exchange-like trading lives.

It includes:

- spot order books
- perpetual futures order books
- margin state
- matching engine state
- liquidations

In our graph examples, this kind of action targets HyperCore:

```json
{
  "subtype": "perp_order",
  "config": {
    "symbol": "ETH",
    "side": "long",
    "chain": "hyperliquid"
  }
}
```

Backtest implication:

- model it like an exchange venue
- use Hyperliquid candles/funding/metadata
- apply Hyperliquid fees and margin/liquidation rules
- do not model this as a Uniswap-style AMM swap

### HyperEVM

**HyperEVM** is Hyperliquid's EVM-compatible smart-contract environment.

It lets developers build Ethereum-style applications on Hyperliquid while
interacting with HyperCore liquidity over time.

For our purposes:

```text
HyperEVM = smart contracts on Hyperliquid
HyperCore = native spot/perp order books on Hyperliquid
```

Backtest implication:

- HyperEVM actions would look more like EVM smart-contract actions.
- HyperCore actions look more like exchange/order-book actions.
- A balance on HyperCore is not automatically the same as a balance on HyperEVM.

## Can One Chain Have More Than One Token Or Asset?

Yes. Most chains support many assets.

Example: Base can have many tokens:

```text
ETH
USDC
WETH
cbBTC
AERO
DAI
many app-specific tokens
```

Arbitrum can also have many tokens:

```text
ETH
USDC
ARB
WETH
GMX
USDT
many app-specific tokens
```

Hyperliquid can have:

```text
USDC spot balance
HYPE spot balance
ETH spot market exposure
BTC spot market exposure
ETH perp position
BTC perp position
many other listed spot/perp markets
```

So we should not model a chain as having one token.

Better model:

```text
chain / venue / account
  asset A balance
  asset B balance
  asset C balance
  positions
```

## Token vs Asset vs Position

These words are often used loosely, but for the backtester we should be precise.

### Token

A token is usually an on-chain object or native coin.

Examples:

- USDC token on Base
- ETH native gas token
- ERC-20 token on an EVM chain

### Asset

An asset is the economic thing being valued or traded.

Examples:

- ETH
- USDC
- BTC
- HYPE

The same asset can appear in different forms:

```text
ETH on Ethereum
ETH on Base
WETH on Base
ETH spot balance on Hyperliquid
ETH perp contract on Hyperliquid
```

They are related economically, but not identical for settlement and execution.

### Position

A position is exposure, not necessarily a token balance.

Example:

```text
Long 500 USDC notional ETH perp at 5x leverage
```

This gives ETH price exposure, but the user does not own ETH tokens.

## What Does Portfolio Ledger Mean?

A **portfolio ledger** is the backtester's accounting book.

It records every balance and position over time.

Simple example:

```text
Start:
  Base wallet:
    USDC: 1000
    ETH: 0

Action:
  Swap 100 USDC to ETH
  Gas cost: 0.002 ETH
  Fee/slippage: 0.10 USDC equivalent

End:
  Base wallet:
    USDC: 900
    ETH: 0.0498
```

The ledger should answer:

- how much USDC do we have?
- how much ETH do we have?
- which chain or venue holds it?
- what perp positions are open?
- what yield positions are deposited?
- what fees have been paid?
- what gas has been paid?
- what funding has been paid or earned?
- what yield has accrued?
- what is the current total portfolio value?

For our system, the ledger is not a blockchain. It is an internal accounting model
inside the simulator.

Suggested shape:

```text
PortfolioLedger
  balances:
    base:
      USDC: 900
      ETH: 0.0498
    hyperliquid_core:
      USDC: 1000
      ETH: 0
    hyperevm:
      HYPE: 5
  positions:
    hyperliquid_core:
      ETH-PERP:
        side: long
        notional: 500
        leverage: 5
        entry_price: 2000
  yield_positions:
    base/aave/USDC:
      principal: 250
      accrued_yield: 1.25
```

## What Is "Perp" In ETH Perp?

`Perp` means **perpetual futures contract**.

`ETH perp` means:

```text
a perpetual futures contract whose price exposure is tied to ETH
```

It is not the same as owning ETH.

If you buy spot ETH:

```text
You own ETH tokens.
```

If you open a long ETH perp:

```text
You have a leveraged contract position that profits if ETH rises.
You do not own ETH tokens.
```

If you open a short ETH perp:

```text
You have a contract position that profits if ETH falls.
```

Perps matter because they introduce:

- leverage
- margin
- liquidation risk
- funding payments
- mark price / index price
- venue-specific position accounting

Example:

```text
Open 500 USDC notional ETH long at 5x leverage.
```

Roughly means:

```text
Use about 100 USDC of margin to get 500 USDC of ETH price exposure.
```

If ETH rises 10%, the position gains roughly 50 USDC before fees/funding.
If ETH falls 10%, the position loses roughly 50 USDC before fees/funding.

## What Is An On-Chain AMM Swap?

An **AMM** is an automated market maker.

An **on-chain AMM swap** is a token trade executed by a smart contract instead of
a traditional order book.

Traditional order book:

```text
Buyers and sellers place orders.
Your market order matches with resting orders.
```

AMM:

```text
A smart contract holds pools of tokens.
You trade against the pool.
The pool's formula determines the price.
```

Example pool:

```text
ETH / USDC pool
```

If you swap:

```text
100 USDC -> ETH
```

then the smart contract:

1. takes USDC from your wallet
2. sends ETH from the pool to your wallet
3. charges a pool fee
4. updates the pool reserves/liquidity

Backtest implications:

- price depends on pool state at that historical block
- large trades can move the price
- the DEX charges fees
- the user pays gas
- execution happens at block time

For MVP, we can approximate:

```text
execution price = historical ETH price adjusted by fixed slippage
```

For realistic AMM simulation, we eventually need:

- DEX venue
- pool address
- historical pool liquidity
- fee tier
- token decimals
- block timestamp
- gas estimate

## Why This Matters For Catalyst Graphs

This graph node:

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

is under-specified for realistic execution.

It tells us:

```text
intent: buy ETH using USDC
chain: Base
```

It does not tell us:

```text
venue: Uniswap? Aerodrome? aggregator?
route: direct pool or multi-hop?
slippage tolerance?
gas assumptions?
transaction latency?
```

So the backtester needs either:

1. more graph config, or
2. explicit simulation defaults.

## Sources

- Ethereum EVM docs: https://ethereum.org/developers/docs/evm/
- Base docs: https://docs.base.org/
- Base chain overview: https://www.base.org/chain
- Arbitrum docs: https://docs.arbitrum.io/
- Hyperliquid overview: https://hyperliquid.gitbook.io/hyperliquid-docs
- HyperEVM docs: https://hyperliquid.gitbook.io/hyperliquid-docs/hyperevm
- HyperCore overview: https://hyperliquid.gitbook.io/hyperliquid-docs/hypercore/overview

