//! Explain: turns reason codes into the sentences the front-ends show.
//! Definitions carry codes and pointers only, never free text.

use crate::api::{Reason, ReasonCode};

/// A Shadowed Entry's reason, pointing at the Effective Entry `winner`.
pub(crate) fn shadowed_by_precedence(agent_name: &str, winner_scope: &str, winner: &str) -> Reason {
    Reason {
        code: ReasonCode::ShadowedByPrecedence,
        winner: Some(winner.to_owned()),
        sentence: format!(
            "Shadowed by the {winner_scope} Entry {winner}, which {agent_name} ranks higher."
        ),
    }
}
