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
    pub fn as_str(self) -> &'static str {
        match self {
            PortalDirection::IntoInstance => "into_instance",
            PortalDirection::OutOfInstance => "out_of_instance",
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
