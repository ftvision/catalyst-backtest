//! Deterministic portfolio accounting for the simulator.
//!
//! The [`Ledger`] is the single source of truth for balances, perp/yield
//! positions, and cumulative costs (fees, gas, funding, yield). Execution models
//! (see `catalyst-execution-models`) drive it through small, explicit operations
//! — `credit`/`debit`, `open_perp`/`close_perp`, `deposit_yield`/`withdraw_yield`
//! — rather than mutating state directly, which keeps accounting deterministic
//! and auditable.
//!
//! Under strict policy the ledger refuses to go negative: a [`Ledger::debit`]
//! that would overdraw returns [`LedgerError::InsufficientBalance`] and leaves
//! balances unchanged.
//!
//! Balance moves are direction-typed (#165): [`Ledger::credit`] and
//! [`Ledger::debit`] both reject negative amounts with
//! [`LedgerError::NegativeAmount`] under *every* policy — `allow_negative`
//! relaxes only the overdraw guard. A caller that means to move money the other
//! way must call the other method, so no sign trick can bypass a guard.

mod error;
mod position;

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use catalyst_contracts::trace::Portfolio;

pub use error::LedgerError;
pub use position::{PerpPosition, PerpSide, YieldKey, YieldPosition};

pub const CRATE_NAME: &str = "catalyst-portfolio-ledger";

type Balances = BTreeMap<String, BTreeMap<String, Decimal>>;

/// Deterministic portfolio ledger.
#[derive(Debug, Clone)]
pub struct Ledger {
    balances: Balances,
    perps: BTreeMap<(String, String), PerpPosition>,
    yields: BTreeMap<YieldKey, YieldPosition>,
    fees_usd: Decimal,
    gas_usd: Decimal,
    funding_usd: Decimal,
    yield_usd: Decimal,
    allow_negative: bool,
}

impl Ledger {
    /// New empty ledger. `allow_negative` disables the overdraw guard (only used
    /// by the `allow_negative` balance policy).
    pub fn new(allow_negative: bool) -> Self {
        Ledger {
            balances: BTreeMap::new(),
            perps: BTreeMap::new(),
            yields: BTreeMap::new(),
            fees_usd: Decimal::ZERO,
            gas_usd: Decimal::ZERO,
            funding_usd: Decimal::ZERO,
            yield_usd: Decimal::ZERO,
            allow_negative,
        }
    }

    /// New ledger seeded with starting balances (venue -> asset -> amount).
    pub fn with_initial(initial: Balances, allow_negative: bool) -> Self {
        let mut ledger = Ledger::new(allow_negative);
        ledger.balances = initial;
        ledger
    }

    // --- Spot/cash balances ---

    /// Current balance of `asset` on `venue` (zero if absent).
    pub fn balance(&self, venue: &str, asset: &str) -> Decimal {
        self.balances
            .get(venue)
            .and_then(|a| a.get(asset))
            .copied()
            .unwrap_or(Decimal::ZERO)
    }

    /// Add `amount` to a balance. Rejects negative amounts (#165): a negative
    /// credit is a hidden, unguarded debit — callers that mean to take money
    /// must call [`Ledger::debit`], which carries the overdraw guard. Zero is
    /// allowed (no-op).
    pub fn credit(&mut self, venue: &str, asset: &str, amount: Decimal) -> Result<(), LedgerError> {
        if amount < Decimal::ZERO {
            return Err(LedgerError::NegativeAmount {
                op: "credit",
                venue: venue.to_string(),
                asset: asset.to_string(),
                amount,
            });
        }
        let entry = self
            .balances
            .entry(venue.to_string())
            .or_default()
            .entry(asset.to_string())
            .or_insert(Decimal::ZERO);
        *entry += amount;
        Ok(())
    }

    /// Subtract `amount` from a balance, refusing to overdraw under strict policy.
    /// Rejects negative amounts under every policy (#165): a negative debit is a
    /// hidden, unguarded credit — `allow_negative` relaxes only the overdraw
    /// guard, never the sign guard. Zero is allowed (no-op).
    pub fn debit(&mut self, venue: &str, asset: &str, amount: Decimal) -> Result<(), LedgerError> {
        if amount < Decimal::ZERO {
            return Err(LedgerError::NegativeAmount {
                op: "debit",
                venue: venue.to_string(),
                asset: asset.to_string(),
                amount,
            });
        }
        let available = self.balance(venue, asset);
        if !self.allow_negative && amount > available {
            return Err(LedgerError::InsufficientBalance {
                venue: venue.to_string(),
                asset: asset.to_string(),
                requested: amount,
                available,
            });
        }
        let entry = self
            .balances
            .entry(venue.to_string())
            .or_default()
            .entry(asset.to_string())
            .or_insert(Decimal::ZERO);
        *entry -= amount;
        Ok(())
    }

    // --- Cost accounting (accumulators; balance movement is separate) ---

    pub fn record_fee(&mut self, usd: Decimal) {
        self.fees_usd += usd;
    }

    pub fn record_gas(&mut self, usd: Decimal) {
        self.gas_usd += usd;
    }

    /// Funding is signed: positive = paid by us, negative = received.
    pub fn record_funding(&mut self, usd: Decimal) {
        self.funding_usd += usd;
    }

    /// Yield is signed: positive = earned.
    pub fn record_yield(&mut self, usd: Decimal) {
        self.yield_usd += usd;
    }

    pub fn fees_usd(&self) -> Decimal {
        self.fees_usd
    }
    pub fn gas_usd(&self) -> Decimal {
        self.gas_usd
    }
    pub fn funding_usd(&self) -> Decimal {
        self.funding_usd
    }
    pub fn yield_usd(&self) -> Decimal {
        self.yield_usd
    }

    // --- Perp positions ---

    pub fn perp(&self, venue: &str, symbol: &str) -> Option<&PerpPosition> {
        self.perps.get(&(venue.to_string(), symbol.to_string()))
    }

    pub fn perps(&self) -> impl Iterator<Item = &PerpPosition> {
        self.perps.values()
    }

    /// Open a perp: debit its margin from the venue's USDC and record it.
    pub fn open_perp(&mut self, position: PerpPosition) -> Result<(), LedgerError> {
        self.debit(&position.venue, "USDC", position.margin_usd)?;
        self.perps.insert(position.key(), position);
        Ok(())
    }

    /// Replace/insert a position without touching balances (for netting/adds the
    /// execution model has already settled in cash).
    pub fn set_perp(&mut self, position: PerpPosition) {
        self.perps.insert(position.key(), position);
    }

    /// Close a perp: remove it and credit `settlement_usd` (margin ± realized
    /// PnL) back to the venue's USDC.
    ///
    /// Rejects a negative `settlement_usd` before touching any state (#165):
    /// callers floor an underwater settlement at zero (#117), and this guard
    /// makes "no code path can credit unposted collateral" a ledger invariant
    /// rather than a call-site convention.
    pub fn close_perp(
        &mut self,
        venue: &str,
        symbol: &str,
        settlement_usd: Decimal,
    ) -> Result<PerpPosition, LedgerError> {
        if settlement_usd < Decimal::ZERO {
            return Err(LedgerError::NegativeAmount {
                op: "close_perp settlement",
                venue: venue.to_string(),
                asset: "USDC".to_string(),
                amount: settlement_usd,
            });
        }
        let position = self
            .perps
            .remove(&(venue.to_string(), symbol.to_string()))
            .ok_or_else(|| LedgerError::NoSuchPerp {
                venue: venue.to_string(),
                symbol: symbol.to_string(),
            })?;
        self.credit(venue, "USDC", settlement_usd)
            .expect("non-negative by construction (guarded above)");
        Ok(position)
    }

    // --- Yield positions ---

    pub fn yield_position(
        &self,
        protocol: &str,
        asset: &str,
        chain: &str,
        pool: Option<&str>,
    ) -> Option<&YieldPosition> {
        self.yields.get(&(
            protocol.to_string(),
            asset.to_string(),
            chain.to_string(),
            pool.map(str::to_string),
        ))
    }

    pub fn yields(&self) -> impl Iterator<Item = &YieldPosition> {
        self.yields.values()
    }

    /// Deposit into a yield position: debit `amount` of `asset` on `chain` and
    /// add it to the position's principal (creating the position if needed).
    pub fn deposit_yield(
        &mut self,
        protocol: &str,
        asset: &str,
        chain: &str,
        pool: Option<&str>,
        amount: Decimal,
    ) -> Result<(), LedgerError> {
        self.debit(chain, asset, amount)?;
        let key =
            (protocol.to_string(), asset.to_string(), chain.to_string(), pool.map(str::to_string));
        self.yields
            .entry(key)
            .and_modify(|p| p.principal += amount)
            .or_insert_with(|| YieldPosition {
                protocol: protocol.to_string(),
                pool: pool.map(str::to_string),
                asset: asset.to_string(),
                chain: chain.to_string(),
                principal: amount,
                accrued: Decimal::ZERO,
            });
        Ok(())
    }

    /// Accrue interest onto a yield position and record it as earned yield.
    ///
    /// `amount` is in **asset units** (interest on an ETH deposit is ETH) and
    /// grows the position's `accrued` balance; `interest_usd` is the same
    /// interest converted to USD at the accrual tick's mark price and is what
    /// feeds the cumulative `yield_usd` counter (#166 — for non-stable assets
    /// the two differ, and the caller owns the conversion).
    pub fn accrue_yield(
        &mut self,
        protocol: &str,
        asset: &str,
        chain: &str,
        pool: Option<&str>,
        amount: Decimal,
        interest_usd: Decimal,
    ) -> Result<(), LedgerError> {
        let key =
            (protocol.to_string(), asset.to_string(), chain.to_string(), pool.map(str::to_string));
        let position = self.yields.get_mut(&key).ok_or_else(|| LedgerError::NoSuchYield {
            protocol: protocol.to_string(),
            asset: asset.to_string(),
            chain: chain.to_string(),
        })?;
        position.accrued += amount;
        self.yield_usd += interest_usd;
        Ok(())
    }

    /// Withdraw `amount` of redeemable value (accrued first, then principal) and
    /// credit it back to the chain balance. Returns the amount withdrawn.
    pub fn withdraw_yield(
        &mut self,
        protocol: &str,
        asset: &str,
        chain: &str,
        pool: Option<&str>,
        amount: Decimal,
    ) -> Result<Decimal, LedgerError> {
        // Negative withdrawals are rejected up front (#165): besides being a
        // hidden deposit, a negative `amount` would *grow* `accrued` through the
        // draw-down math below and then hit the credit sign guard mid-mutation.
        if amount < Decimal::ZERO {
            return Err(LedgerError::NegativeAmount {
                op: "withdraw_yield",
                venue: chain.to_string(),
                asset: asset.to_string(),
                amount,
            });
        }
        let key =
            (protocol.to_string(), asset.to_string(), chain.to_string(), pool.map(str::to_string));
        let position = self.yields.get_mut(&key).ok_or_else(|| LedgerError::NoSuchYield {
            protocol: protocol.to_string(),
            asset: asset.to_string(),
            chain: chain.to_string(),
        })?;
        let available = position.value();
        if amount > available {
            return Err(LedgerError::InsufficientYield {
                protocol: protocol.to_string(),
                asset: asset.to_string(),
                requested: amount,
                available,
            });
        }
        // Draw down accrued interest first, then principal.
        let from_accrued = amount.min(position.accrued);
        position.accrued -= from_accrued;
        position.principal -= amount - from_accrued;
        if position.value().is_zero() {
            self.yields.remove(&key);
        }
        self.credit(chain, asset, amount)
            .expect("non-negative by construction (guarded at entry)");
        Ok(amount)
    }

    /// Full redeemable value of a yield position, for `amount: "all"` withdrawals.
    pub fn yield_value(
        &self,
        protocol: &str,
        asset: &str,
        chain: &str,
        pool: Option<&str>,
    ) -> Decimal {
        self.yield_position(protocol, asset, chain, pool)
            .map(YieldPosition::value)
            .unwrap_or(Decimal::ZERO)
    }

    // --- Snapshot ---

    /// Project current state to a contract [`Portfolio`] (zero balances dropped).
    /// `maintenance_margin_ratio` is the policy's maintenance fraction of mark
    /// notional, used to report each perp's `liquidation_price` (#120).
    pub fn to_portfolio(&self, maintenance_margin_ratio: Decimal) -> Portfolio {
        let mut balances: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        for (venue, assets) in &self.balances {
            let mut out = BTreeMap::new();
            for (asset, amount) in assets {
                if !amount.is_zero() {
                    out.insert(asset.clone(), amount.normalize().to_string());
                }
            }
            if !out.is_empty() {
                balances.insert(venue.clone(), out);
            }
        }
        Portfolio {
            balances,
            perp_positions: self
                .perps
                .values()
                .map(|p| p.to_contract(maintenance_margin_ratio))
                .collect(),
            yield_positions: self.yields.values().map(YieldPosition::to_contract).collect(),
        }
    }
}
