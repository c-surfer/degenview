//! Visual density of the left drawing toolbar (`ToolSidebar`).
//!
//! Kept in `model` so every layer (`model`, `ui`, `host`, `app`) can read the active choice
//! without importing `service` or `store`.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

/// Size and spacing preset for the left drawing toolbar.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRailStyle {
    #[default]
    Default,
    Compact,
}

impl ToolRailStyle {
    /// Every selectable style in display order.
    pub const ALL: [Self; 2] = [Self::Default, Self::Compact];

    /// Human-readable label shown in Settings.
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => crate::i18n::tr!("Default"),
            Self::Compact => crate::i18n::tr!("Compact"),
        }
    }

    const fn index(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::Compact => 1,
        }
    }

    const fn from_index(index: u8) -> Self {
        match index {
            1 => Self::Compact,
            _ => Self::Default,
        }
    }
}

static ACTIVE_STYLE: AtomicU8 = AtomicU8::new(0);

/// Active toolbar style read by `tool_sidebar` and Settings.
pub fn active_tool_rail_style() -> ToolRailStyle {
    ToolRailStyle::from_index(ACTIVE_STYLE.load(Ordering::Relaxed))
}

/// Updates the active toolbar style in memory.
pub fn set_active_tool_rail_style(style: ToolRailStyle) {
    ACTIVE_STYLE.store(style.index(), Ordering::Relaxed);
}
