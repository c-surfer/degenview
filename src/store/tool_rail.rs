//! The left drawing toolbar style the user chose (Settings ▸ Appearance ▸ Icon style), kept as
//! one `setting`.

use super::Store;
use crate::model::tool_rail::ToolRailStyle;

pub const KEY: &str = "ui.toolRailStyle";

impl Store {
    /// The saved toolbar style; defaults to `Default` when none was saved or when the saved value
    /// cannot be decoded.
    pub fn tool_rail_style(&self) -> ToolRailStyle {
        self.setting::<ToolRailStyle>(KEY).unwrap_or_default()
    }

    pub fn set_tool_rail_style(&self, style: ToolRailStyle) -> rusqlite::Result<()> {
        self.set_setting(KEY, &style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_saved_uses_default() {
        let store = Store::open_in_memory().expect("store");
        assert_eq!(store.tool_rail_style(), ToolRailStyle::Default);
    }

    #[test]
    fn a_choice_is_read_back() {
        let store = Store::open_in_memory().expect("store");
        store
            .set_tool_rail_style(ToolRailStyle::Compact)
            .expect("save");
        assert_eq!(store.tool_rail_style(), ToolRailStyle::Compact);
        store
            .set_tool_rail_style(ToolRailStyle::Default)
            .expect("save");
        assert_eq!(store.tool_rail_style(), ToolRailStyle::Default);
    }

    #[test]
    fn an_unreadable_value_falls_back_to_default() {
        let store = Store::open_in_memory().expect("store");
        store.set_setting(KEY, &"gigantic").expect("save");
        assert_eq!(store.tool_rail_style(), ToolRailStyle::Default);
    }
}
