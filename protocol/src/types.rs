//! Closed-enum game kinds shared by the server and the client. Pure (no ECS,
//! no I/O). The wire carries these as their `as_str()` strings.

/// Resource kinds (trees today; rock/ore later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Tree,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Tree => "tree",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tree" => Some(ResourceKind::Tree),
            _ => None,
        }
    }
}

/// Item kinds — the *type* of a stackable substance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Item {
    Wood,
    /// Harvested from a Carcass; the food economy.
    Meat,
    /// Harvested from a Carcass; a crafting material.
    Hide,
}

impl Item {
    pub fn as_str(self) -> &'static str {
        match self {
            Item::Wood => "wood",
            Item::Meat => "meat",
            Item::Hide => "hide",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "wood" => Some(Item::Wood),
            "meat" => Some(Item::Meat),
            "hide" => Some(Item::Hide),
            _ => None,
        }
    }
}

/// Structure kinds (only the wooden palisade "wall" in v1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    Wall,
}

impl StructureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StructureKind::Wall => "wall",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "wall" => Some(StructureKind::Wall),
            _ => None,
        }
    }
}

/// Portal role discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalDirection {
    IntoInstance,
    OutOfInstance,
}

impl PortalDirection {
    /// Every direction — what the showcase enumerates to display them all. The
    /// guard match breaks this const's compile when a variant is added.
    pub const ALL: [Self; 2] = {
        let all = [PortalDirection::IntoInstance, PortalDirection::OutOfInstance];
        match all[0] {
            PortalDirection::IntoInstance | PortalDirection::OutOfInstance => {}
        }
        all
    };

    pub fn as_str(self) -> &'static str {
        match self {
            PortalDirection::IntoInstance => "into_instance",
            PortalDirection::OutOfInstance => "out_of_instance",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "into_instance" => Some(PortalDirection::IntoInstance),
            "out_of_instance" => Some(PortalDirection::OutOfInstance),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalKind {
    Dungeon,
}

impl PortalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PortalKind::Dungeon => "dungeon",
        }
    }
}

/// NPC kinds (the v1 wildlife pair).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpcKind {
    Wolf,
    Deer,
}

impl NpcKind {
    /// Every kind — what the showcase enumerates to display them all. The
    /// guard match below breaks this const's compile when a variant is added,
    /// so the list cannot silently fall behind the enum.
    pub const ALL: [Self; 2] = {
        let all = [NpcKind::Wolf, NpcKind::Deer];
        match all[0] {
            NpcKind::Wolf | NpcKind::Deer => {}
        }
        all
    };

    pub fn as_str(self) -> &'static str {
        match self {
            NpcKind::Wolf => "wolf",
            NpcKind::Deer => "deer",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "wolf" => Some(NpcKind::Wolf),
            "deer" => Some(NpcKind::Deer),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_direction_wire_strings_roundtrip() {
        for d in PortalDirection::ALL {
            assert_eq!(PortalDirection::parse(d.as_str()), Some(d));
        }
        assert_eq!(PortalDirection::parse("sideways"), None);
    }

    #[test]
    fn npc_kind_wire_strings_roundtrip() {
        for k in NpcKind::ALL {
            assert_eq!(NpcKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(NpcKind::parse("gibberish"), None);
    }
}
