//! Which chart template new charts start from. It is one `setting` holding the template's id (a
//! preset's or a saved one's), so renaming or updating the template changes what new charts get.
//! No setting means a new chart starts from the app's own look.

use crate::model::ChartAppearance;
use crate::model::chart::template::{TemplateId, built_in};
use crate::store::Store;

/// Where the choice is kept (`sync::record::policy` lists it as a portable setting).
pub const KEY: &str = "chart.defaultTemplate";

/// Where the latest user-chosen chart appearance is kept so new charts and tabs inherit it.
pub const GLOBAL_APPEARANCE_KEY: &str = "chart.globalAppearance";

/// The template new charts start from, as far as it can be resolved.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DefaultTemplate {
    /// The template the user chose. It stays set when the template cannot be found, which is
    /// what a Mac sees while the template is still on its way from iCloud.
    pub id: Option<TemplateId>,
    /// Its look; `None` with no choice or when the choice no longer exists.
    pub look: Option<ChartAppearance>,
}

impl Store {
    /// The default template, resolved. A table this version cannot read counts as having none
    /// rather than failing a chart.
    pub fn default_chart_template(&self) -> DefaultTemplate {
        let Some(id) = self.setting::<TemplateId>(KEY) else {
            return DefaultTemplate::default();
        };
        let look = match built_in(id) {
            Some(preset) => Some(preset.appearance()),
            None => self
                .chart_templates_strict()
                .ok()
                .and_then(|saved| saved.into_iter().find(|template| template.id == id))
                .map(|template| template.appearance),
        };
        DefaultTemplate { id: Some(id), look }
    }

    /// Makes `id` the template new charts start from, or with `None` puts that back to the app's
    /// own look. Clears any global appearance override so the explicit template choice wins.
    pub fn set_default_chart_template(&self, id: Option<TemplateId>) -> rusqlite::Result<()> {
        self.clear_global_chart_appearance()?;
        match id {
            Some(id) => self.set_setting(KEY, &id),
            None => self.remove_setting(KEY),
        }
    }

    /// The global chart appearance saved from chart settings, if any.
    pub fn global_chart_appearance(&self) -> Option<ChartAppearance> {
        self.setting::<ChartAppearance>(GLOBAL_APPEARANCE_KEY)
    }

    /// Saves `appearance` as the global chart appearance for new charts and tabs.
    /// Clears any starred default template so Manage Templates does not show a stale star.
    pub fn set_global_chart_appearance(
        &self,
        appearance: &ChartAppearance,
    ) -> rusqlite::Result<()> {
        self.remove_setting(KEY)?;
        self.set_setting(GLOBAL_APPEARANCE_KEY, appearance)
    }

    /// Clears the global chart appearance override.
    pub fn clear_global_chart_appearance(&self) -> rusqlite::Result<()> {
        self.remove_setting(GLOBAL_APPEARANCE_KEY)
    }

    /// The look new charts start from: the global appearance if set, otherwise the default
    /// template's look.
    pub fn default_chart_appearance(&self) -> Option<ChartAppearance> {
        self.global_chart_appearance()
            .or_else(|| self.default_chart_template().look)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::chart::canvas::BackgroundMode;
    use crate::model::chart::template::{BUILT_INS, ChartAppearanceTemplate};

    fn saved(id: TemplateId) -> ChartAppearanceTemplate {
        let mut appearance = ChartAppearance::default();
        appearance.canvas.background.mode = BackgroundMode::Gradient;
        ChartAppearanceTemplate {
            id,
            name: "Mine".into(),
            description: None,
            appearance,
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn with_no_choice_a_new_chart_gets_the_app_look() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.default_chart_template(), DefaultTemplate::default());
    }

    #[test]
    fn a_saved_template_is_resolved_to_its_look_and_follows_updates() {
        let store = Store::open_in_memory().expect("open");
        let template = saved(2_000);
        store
            .replace_chart_templates(std::slice::from_ref(&template))
            .expect("saved");
        store.set_default_chart_template(Some(2_000)).expect("set");
        let chosen = store.default_chart_template();
        assert_eq!(chosen.id, Some(2_000));
        assert_eq!(chosen.look, Some(template.appearance));

        let mut updated = template;
        updated.appearance.canvas.margins.right = 33;
        store
            .replace_chart_templates(std::slice::from_ref(&updated))
            .expect("updated");
        assert_eq!(
            store.default_chart_template().look,
            Some(updated.appearance)
        );
    }

    #[test]
    fn a_preset_can_be_the_default() {
        let store = Store::open_in_memory().expect("open");
        let preset = &BUILT_INS[1];
        store
            .set_default_chart_template(Some(preset.id))
            .expect("set");
        assert_eq!(
            store.default_chart_template().look,
            Some(preset.appearance())
        );
    }

    #[test]
    fn a_template_that_is_gone_leaves_the_choice_but_no_look() {
        let store = Store::open_in_memory().expect("open");
        store.set_default_chart_template(Some(9_999)).expect("set");
        assert_eq!(
            store.default_chart_template(),
            DefaultTemplate {
                id: Some(9_999),
                look: None
            }
        );
    }

    #[test]
    fn clearing_the_choice_puts_the_app_look_back() {
        let store = Store::open_in_memory().expect("open");
        store
            .set_default_chart_template(Some(BUILT_INS[0].id))
            .expect("set");
        store.set_default_chart_template(None).expect("cleared");
        assert_eq!(store.default_chart_template(), DefaultTemplate::default());
    }

    #[test]
    fn global_appearance_overrides_default_template_and_clears_on_template_pick() {
        let store = Store::open_in_memory().expect("open");
        assert_eq!(store.global_chart_appearance(), None);
        assert_eq!(store.default_chart_appearance(), None);

        let preset = &BUILT_INS[1];
        store
            .set_default_chart_template(Some(preset.id))
            .expect("set template");
        assert_eq!(store.default_chart_appearance(), Some(preset.appearance()));

        let mut custom = ChartAppearance {
            bullish: [11, 22, 33],
            ..Default::default()
        };
        custom.canvas.margins.right = 44;
        store
            .set_global_chart_appearance(&custom)
            .expect("set global");
        assert_eq!(store.global_chart_appearance(), Some(custom));
        assert_eq!(store.default_chart_appearance(), Some(custom));
        assert_eq!(
            store.default_chart_template(),
            DefaultTemplate::default(),
            "setting a global appearance clears the starred template"
        );

        // Explicit template pick clears the global override so the star choice wins.
        let other_preset = &BUILT_INS[0];
        store
            .set_default_chart_template(Some(other_preset.id))
            .expect("pick template");
        assert_eq!(store.global_chart_appearance(), None);
        assert_eq!(
            store.default_chart_appearance(),
            Some(other_preset.appearance())
        );

        // Clearing the global appearance directly also works.
        store
            .set_global_chart_appearance(&custom)
            .expect("set global again");
        store.clear_global_chart_appearance().expect("clear");
        assert_eq!(store.global_chart_appearance(), None);
        assert_eq!(store.default_chart_appearance(), None);

        // Clearing the default template also clears any global appearance.
        store
            .set_global_chart_appearance(&custom)
            .expect("set global third");
        store.set_default_chart_template(None).expect("clear all");
        assert_eq!(store.global_chart_appearance(), None);
        assert_eq!(store.default_chart_appearance(), None);
    }
}
