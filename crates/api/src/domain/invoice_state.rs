//! Invoice state machine.
//!
//! All state changes route through `try_transition`. Invalid transitions are
//! rejected here, *before* hitting the database, with a typed error.
//!
//! Allowed transitions:
//!
//! ```text
//! draft       --finalize-->            open
//! draft       --void-->                void
//! open        --pay-->                 processing
//! open        --void-->                void
//! open        --mark_uncollectible-->  uncollectible
//! processing  --psp_success-->         paid
//! processing  --psp_failure-->         open
//! ```
//!
//! Terminals: paid, void, uncollectible. None are reversible.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceState {
    Draft,
    Open,
    Processing,
    Paid,
    Void,
    Uncollectible,
}

impl InvoiceState {
    pub fn as_str(self) -> &'static str {
        match self {
            InvoiceState::Draft => "draft",
            InvoiceState::Open => "open",
            InvoiceState::Processing => "processing",
            InvoiceState::Paid => "paid",
            InvoiceState::Void => "void",
            InvoiceState::Uncollectible => "uncollectible",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            InvoiceState::Paid | InvoiceState::Void | InvoiceState::Uncollectible
        )
    }
}

impl fmt::Display for InvoiceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InvoiceState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "draft" => InvoiceState::Draft,
            "open" => InvoiceState::Open,
            "processing" => InvoiceState::Processing,
            "paid" => InvoiceState::Paid,
            "void" => InvoiceState::Void,
            "uncollectible" => InvoiceState::Uncollectible,
            other => return Err(format!("unknown invoice state: {other}")),
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TransitionEvent {
    Finalize,
    Pay,
    Void,
    MarkUncollectible,
    PspSuccess,
    PspFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: InvoiceState,
    pub event: TransitionEvent,
}

impl fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid transition: cannot apply {:?} to invoice in state {}",
            self.event, self.from
        )
    }
}

pub fn try_transition(
    from: InvoiceState,
    event: TransitionEvent,
) -> Result<InvoiceState, InvalidTransition> {
    use InvoiceState as S;
    use TransitionEvent as E;

    let next = match (from, event) {
        (S::Draft, E::Finalize) => S::Open,
        (S::Draft, E::Void) => S::Void,
        (S::Open, E::Pay) => S::Processing,
        (S::Open, E::Void) => S::Void,
        (S::Open, E::MarkUncollectible) => S::Uncollectible,
        (S::Processing, E::PspSuccess) => S::Paid,
        (S::Processing, E::PspFailure) => S::Open,
        _ => return Err(InvalidTransition { from, event }),
    };
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let s = InvoiceState::Draft;
        let s = try_transition(s, TransitionEvent::Finalize).unwrap();
        assert_eq!(s, InvoiceState::Open);
        let s = try_transition(s, TransitionEvent::Pay).unwrap();
        assert_eq!(s, InvoiceState::Processing);
        let s = try_transition(s, TransitionEvent::PspSuccess).unwrap();
        assert_eq!(s, InvoiceState::Paid);
        assert!(s.is_terminal());
    }

    #[test]
    fn pay_only_from_open() {
        assert!(try_transition(InvoiceState::Draft, TransitionEvent::Pay).is_err());
        assert!(try_transition(InvoiceState::Paid, TransitionEvent::Pay).is_err());
        assert!(try_transition(InvoiceState::Void, TransitionEvent::Pay).is_err());
    }

    #[test]
    fn terminal_states_have_no_outgoing() {
        for s in [
            InvoiceState::Paid,
            InvoiceState::Void,
            InvoiceState::Uncollectible,
        ] {
            for e in [
                TransitionEvent::Finalize,
                TransitionEvent::Pay,
                TransitionEvent::Void,
                TransitionEvent::MarkUncollectible,
                TransitionEvent::PspSuccess,
                TransitionEvent::PspFailure,
            ] {
                assert!(try_transition(s, e).is_err(), "{s:?} + {e:?} unexpected");
            }
        }
    }

    #[test]
    fn psp_failure_reverts_to_open() {
        let s = try_transition(InvoiceState::Processing, TransitionEvent::PspFailure).unwrap();
        assert_eq!(s, InvoiceState::Open);
    }
}
