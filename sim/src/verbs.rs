//! Verb result errors, with `as_str` matching the contract's `reason` enums
//! exactly (`contract/contract.json`). The order in which
//! the verb implementations check these mirrors the Elixir `with` chains so the
//! same input yields the same reason.

use crate::components::Inventory;

/// The success outcome of a verb's effect: either the actor's Inventory changed
/// (and should be emitted), or nothing observable resulted. The single uniform
/// return shared by the three verb effects, so resolution maps it to one outcome
/// without per-verb special-casing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionOutcome {
    Inventory(Inventory),
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbError {
    NoPlayer,
    TooFar,
    Depleted,
    NoTarget,
    NoChunk,
    InvalidType,
    OutOfChunk,
    FootprintBlocked,
    InsufficientMaterials,
    NoBuildInInstance,
}

impl VerbError {
    pub fn as_str(self) -> &'static str {
        match self {
            VerbError::NoPlayer => "no_player",
            VerbError::TooFar => "too_far",
            VerbError::Depleted => "depleted",
            VerbError::NoTarget => "no_target",
            VerbError::NoChunk => "no_chunk",
            VerbError::InvalidType => "invalid_type",
            VerbError::OutOfChunk => "out_of_chunk",
            VerbError::FootprintBlocked => "footprint_blocked",
            VerbError::InsufficientMaterials => "insufficient_materials",
            VerbError::NoBuildInInstance => "no_build_in_instance",
        }
    }
}
