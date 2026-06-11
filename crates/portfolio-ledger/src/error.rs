//! Ledger accounting errors.

use std::fmt;

use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    InsufficientBalance {
        venue: String,
        asset: String,
        requested: Decimal,
        /// The spendable figure the request was checked against: balance minus
        /// any amounts earmarked by resting orders (#124).
        available: Decimal,
        /// How much of the raw balance is earmarked by resting-order
        /// reservations (#124). Zero when nothing is reserved, in which case
        /// `available` equals the raw balance and the message omits it.
        reserved: Decimal,
    },
    /// A balance operation was asked to move a negative amount (#165). Credits
    /// and debits are direction-typed: a negative credit is a hidden unguarded
    /// debit and vice versa, so both reject signed amounts instead of silently
    /// flipping direction. `op` names the rejected operation.
    NegativeAmount {
        op: &'static str,
        venue: String,
        asset: String,
        amount: Decimal,
    },
    NoSuchPerp {
        venue: String,
        symbol: String,
    },
    NoSuchYield {
        protocol: String,
        asset: String,
        chain: String,
    },
    InsufficientYield {
        protocol: String,
        asset: String,
        requested: Decimal,
        available: Decimal,
    },
}

impl fmt::Display for LedgerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LedgerError::InsufficientBalance { venue, asset, requested, available, reserved } => {
                if reserved.is_zero() {
                    write!(
                        f,
                        "insufficient balance: need {requested} {asset} on {venue}, have {available}"
                    )
                } else {
                    write!(
                        f,
                        "insufficient balance: need {requested} {asset} on {venue}, have {available} available ({reserved} reserved by resting orders)"
                    )
                }
            }
            LedgerError::NegativeAmount { op, venue, asset, amount } => write!(
                f,
                "negative amount in {op}: {amount} {asset} on {venue} (amounts must be non-negative)"
            ),
            LedgerError::NoSuchPerp { venue, symbol } => {
                write!(f, "no open perp position for {symbol} on {venue}")
            }
            LedgerError::NoSuchYield { protocol, asset, chain } => {
                write!(f, "no yield position for {protocol}/{asset} on {chain}")
            }
            LedgerError::InsufficientYield { protocol, asset, requested, available } => write!(
                f,
                "insufficient yield balance: need {requested} {asset} in {protocol}, have {available}"
            ),
        }
    }
}

impl std::error::Error for LedgerError {}
