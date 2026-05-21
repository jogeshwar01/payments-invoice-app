//! Money is represented as integer minor units (cents). Never float.
//!
//! We use a small newtype so the type system catches accidental mixing of
//! cents with other integers.

use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cents(pub i64);

impl Cents {
    pub const ZERO: Cents = Cents(0);

    pub fn checked_add(self, other: Cents) -> Option<Cents> {
        self.0.checked_add(other.0).map(Cents)
    }

    pub fn checked_mul_quantity(self, qty: i32) -> Option<Cents> {
        if qty < 0 {
            return None;
        }
        self.0.checked_mul(qty as i64).map(Cents)
    }
}

impl From<Cents> for i64 {
    fn from(c: Cents) -> i64 {
        c.0
    }
}
