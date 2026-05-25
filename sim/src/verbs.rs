//! Verb result errors, with `as_str` matching the contract's `reason` enums
//! exactly (`apps/game_web/priv/contract/contract.json`). The order in which
//! the verb implementations check these mirrors the Elixir `with` chains so the
//! same input yields the same reason.

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
