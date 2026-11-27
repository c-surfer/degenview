//! The chart settings window and applying one chart's appearance to the others.

use crate::model::DataSource;
use crate::ui::chart::settings::{self, ChartSettingsAction, ChartSettingsContext};
use crate::workspace::{ChartWorkspace, WorkspaceAction};
use eframe::egui;

impl ChartWorkspace {
    /// The chart settings sheet over the focused window; returns what the workspace's host should
    /// do (opening the Pine Studio).
    pub(in crate::workspace) fn show_settings_window(
        &mut self,
        ctx: &egui::Context,
    ) -> Vec<WorkspaceAction> {
        let Some(chart_id) = self.settings_chart else {
            return Vec::new();
        };
        if !self.charts.iter().any(|chart| chart.id == chart_id) {
            self.settings_chart = None;
            self.settings_baseline = None;
            return Vec::new();
        }
        self.ensure_settings_baseline(chart_id);
        let chart = self
            .charts
            .iter_mut()
            .find(|chart| chart.id == chart_id)
            .expect("chart checked above");
        let title = chart.display_title();
        let outcome = settings::show(
            ctx,
            &mut self.settings_state,
            ChartSettingsContext {
                title: &title,
                symbol: &chart.symbol,
                source: chart.source,
                indicators: &mut chart.indicators,
                card: &mut chart.chart,
                scale_mode: chart.scale_mode,
                store: &self.store,
            },
        );
        self.handle_settings_action(chart_id, outcome)
    }

    fn ensure_settings_baseline(&mut self, chart_id: u64) {
        if self
            .settings_baseline
            .as_ref()
            .is_none_or(|(id, _)| *id != chart_id)
            && let Some(appearance) = self
                .charts
                .iter()
                .find(|chart| chart.id == chart_id)
                .map(|chart| chart.chart.appearance())
        {
            self.settings_baseline = Some((chart_id, appearance));
        }
    }

    fn handle_settings_action(
        &mut self,
        chart_id: u64,
        outcome: Option<ChartSettingsAction>,
    ) -> Vec<WorkspaceAction> {
        let mut actions = Vec::new();
        match outcome {
            Some(ChartSettingsAction::Done) => {
                if let Some(current) = self
                    .charts
                    .iter()
                    .find(|chart| chart.id == chart_id && chart.is_market())
                    .map(|chart| chart.chart.appearance())
                {
                    let changed = self
                        .settings_baseline
                        .as_ref()
                        .is_none_or(|(id, base)| *id != chart_id || *base != current);
                    if changed {
                        let _ =
                            crate::lock::lock(&self.store).set_global_chart_appearance(&current);
                    }
                }
                self.settings_chart = None;
                self.settings_baseline = None;
                self.script_popover = None;
                self.overlay_popover = None;
            }
            Some(ChartSettingsAction::RemoveChart) => {
                self.settings_chart = None;
                self.settings_baseline = None;
                self.script_popover = None;
                self.overlay_popover = None;
                self.remove_chart(chart_id);
            }
            Some(ChartSettingsAction::OpenPineStudio) => {
                self.settings_chart = None;
                self.settings_baseline = None;
                self.script_popover = None;
                self.overlay_popover = None;
                actions.push(WorkspaceAction::OpenPineScripts);
            }
            Some(ChartSettingsAction::ConfigureScript(id, anchor)) => {
                self.toggle_script_popover(chart_id, id, anchor);
            }
            Some(ChartSettingsAction::SetScaleMode(mode)) => self.set_scale_mode(chart_id, mode),
            Some(ChartSettingsAction::Overlay(command)) => {
                self.apply_overlay_command(chart_id, command);
            }
            Some(ChartSettingsAction::AddOverlay) => {
                // The picker is a modal of its own: the sheet gives way to it.
                self.settings_chart = None;
                self.settings_baseline = None;
                self.script_popover = None;
                self.overlay_popover = None;
                self.focus_chart(chart_id);
                self.open_compare_picker();
            }
            Some(ChartSettingsAction::ApplyToAll(scope)) => {
                let appearance = self
                    .charts
                    .iter()
                    .find(|chart| chart.id == chart_id)
                    .map(|chart| chart.chart.appearance());
                if let Some(appearance) = appearance {
                    let message = self.apply_appearance_to_all(Some(chart_id), &appearance, scope);
                    self.settings_state.say(message);
                    {
                        let store = crate::lock::lock(&self.store);
                        let global = match scope {
                            crate::model::chart::template::ApplyScope::AppearanceAndCanvas => {
                                appearance
                            }
                            crate::model::chart::template::ApplyScope::ColorsAndCanvas => {
                                let base = store.default_chart_appearance().unwrap_or_default();
                                scope.merge(&base, &appearance)
                            }
                        };
                        let _ = store.set_global_chart_appearance(&global);
                    }
                    // Keep the source chart's current appearance as the baseline so a follow-up
                    // `Done` without further edits does not overwrite the `ColorsAndCanvas` merge.
                    self.settings_baseline = Some((chart_id, appearance));
                    actions.push(WorkspaceAction::ApplyAppearanceToAllTabs {
                        source_tab_id: self.id,
                        appearance: Box::new(appearance),
                        scope,
                    });
                }
            }
            None => {}
        }
        actions
    }

    /// Gives every market chart of this workspace `appearance`.
    pub fn apply_appearance_to_all_charts(
        &mut self,
        appearance: &crate::model::ChartAppearance,
        scope: crate::model::chart::template::ApplyScope,
    ) {
        self.apply_appearance_to_all(None, appearance, scope);
    }

    /// Gives every chart of this workspace (other than `source_chart`, when given) `appearance`.
    /// The plan is made first, for every chart, so none is left half-restyled; each chart then
    /// takes it through its own card, so a new chart type resets that chart's scroll and bars the
    /// way a pick in its selector does. Other workspaces, saved layouts and everything else a
    /// chart holds are not touched.
    fn apply_appearance_to_all(
        &mut self,
        source_chart: Option<u64>,
        appearance: &crate::model::ChartAppearance,
        scope: crate::model::chart::template::ApplyScope,
    ) -> String {
        let markets: Vec<(u64, DataSource)> = self
            .charts
            .iter()
            .filter(|chart| chart.is_market())
            .map(|chart| (chart.id, chart.source))
            .collect();
        let plan = crate::model::chart::template::plan_apply_to_all(
            &markets,
            appearance,
            scope,
            source_chart,
        );
        for (id, step) in &plan.steps {
            if let Some(chart) = self.charts.iter_mut().find(|chart| chart.id == *id) {
                chart.chart.apply_template(appearance, *step);
            }
        }
        plan.message(appearance.style.chart_type.label())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::InstrumentId;
    use crate::workspace::test_support::empty_workspace;

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn apply_to_all_restyles_the_other_charts_of_this_workspace_only() {
        use crate::model::chart::canvas::BackgroundMode;
        use crate::model::chart::template::ApplyScope;
        let ctx = egui::Context::default();
        let mut workspace = empty_workspace(3);
        let mut control = empty_workspace(4);
        let ids: Vec<u64> = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
            .iter()
            .map(|symbol| {
                workspace
                    .add_chart_for(&InstrumentId::new(DataSource::Binance, *symbol), &ctx)
                    .chart_id
            })
            .collect();
        let other = control
            .add_chart_for(&InstrumentId::new(DataSource::Binance, "XRPUSDT"), &ctx)
            .chart_id;
        let untouched = control.charts[0].chart.appearance();
        // The second chart carries a script and a Kagi type of its own.
        workspace.charts[1].chart.add_pine_instance(
            None,
            "RSI",
            "//@version=6\nindicator(\"x\")\nplot(close)\n",
        );
        workspace.charts[1]
            .chart
            .set_chart_type(crate::model::ChartType::Kagi);
        workspace.mark_clean();

        let mut look = crate::model::ChartAppearance::default();
        look.bullish = [9, 8, 7];
        look.canvas.background.mode = BackgroundMode::Gradient;
        look.canvas.margins.right = 21;
        look.canvas.bar_countdown = false;
        look.style.chart_type = crate::model::ChartType::Renko;
        let message =
            workspace.apply_appearance_to_all(Some(ids[0]), &look, ApplyScope::ColorsAndCanvas);
        assert_eq!(message, "Applied the appearance to 2 charts.");
        for chart in &workspace.charts[1..] {
            let appearance = chart.chart.appearance();
            assert_eq!(appearance.bullish, [9, 8, 7]);
            assert_eq!(appearance.canvas, look.canvas);
        }
        // Colours and canvas only: each chart keeps the type it had.
        assert_eq!(
            workspace.charts[1].chart.appearance().style.chart_type,
            crate::model::ChartType::Kagi
        );
        assert_eq!(
            workspace.charts[2].chart.appearance().style.chart_type,
            crate::model::ChartType::Candles
        );
        assert_eq!(
            workspace
                .charts
                .iter()
                .map(|c| c.symbol.as_str())
                .collect::<Vec<_>>(),
            ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
        );
        assert_eq!(workspace.charts[1].chart.pine_instances().len(), 1);
        assert!(workspace.is_dirty(), "the layout notices the new look");
        // The source chart and the other workspace are untouched.
        assert_eq!(
            workspace.charts[0].chart.appearance().bullish,
            crate::model::ChartAppearance::default().bullish
        );
        assert_eq!(control.charts[0].chart.appearance(), untouched);
        assert_eq!(control.charts[0].id, other);

        // With the type too, charts whose market can draw it take it.
        workspace.apply_appearance_to_all(Some(ids[0]), &look, ApplyScope::AppearanceAndCanvas);
        assert_eq!(
            workspace.charts[2].chart.appearance().style.chart_type,
            crate::model::ChartType::Renko
        );
        let alone =
            control.apply_appearance_to_all(Some(other), &look, ApplyScope::AppearanceAndCanvas);
        assert_eq!(alone, "There are no other charts in this workspace.");
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn apply_appearance_to_all_charts_restyles_every_market_chart() {
        use crate::model::chart::canvas::BackgroundMode;
        use crate::model::chart::template::ApplyScope;
        let ctx = egui::Context::default();
        let mut workspace = empty_workspace(5);
        workspace.add_chart_for(&InstrumentId::new(DataSource::Binance, "BTCUSDT"), &ctx);
        workspace.add_chart_for(&InstrumentId::new(DataSource::Binance, "ETHUSDT"), &ctx);
        workspace.charts[1]
            .chart
            .set_chart_type(crate::model::ChartType::Kagi);

        let mut look = crate::model::ChartAppearance::default();
        look.bullish = [4, 5, 6];
        look.canvas.background.mode = BackgroundMode::Gradient;
        look.style.chart_type = crate::model::ChartType::Renko;

        workspace.apply_appearance_to_all_charts(&look, ApplyScope::ColorsAndCanvas);
        for chart in &workspace.charts {
            let appearance = chart.chart.appearance();
            assert_eq!(appearance.bullish, [4, 5, 6]);
            assert_eq!(appearance.canvas, look.canvas);
        }
        assert_eq!(
            workspace.charts[0].chart.appearance().style.chart_type,
            crate::model::ChartType::Candles
        );
        assert_eq!(
            workspace.charts[1].chart.appearance().style.chart_type,
            crate::model::ChartType::Kagi
        );

        workspace.apply_appearance_to_all_charts(&look, ApplyScope::AppearanceAndCanvas);
        for chart in &workspace.charts {
            assert_eq!(
                chart.chart.appearance().style.chart_type,
                crate::model::ChartType::Renko
            );
        }
    }

    #[test]
    fn done_saves_global_appearance_only_when_changed_and_preserves_apply_to_all_merge() {
        use crate::model::chart::template::ApplyScope;
        let ctx = egui::Context::default();
        let mut workspace = empty_workspace(6);
        let chart_id = workspace
            .add_chart_for(&InstrumentId::new(DataSource::Binance, "BTCUSDT"), &ctx)
            .chart_id;

        let mut initial_global = crate::model::ChartAppearance {
            bullish: [99, 88, 77],
            ..Default::default()
        };
        initial_global.style.chart_type = crate::model::ChartType::HeikinAshi;
        crate::lock::lock(&workspace.store)
            .set_global_chart_appearance(&initial_global)
            .expect("seed global");

        // `Done` without changing the chart's appearance leaves the stored global look untouched.
        workspace.settings_chart = Some(chart_id);
        workspace.ensure_settings_baseline(chart_id);
        workspace.handle_settings_action(chart_id, Some(ChartSettingsAction::Done));
        assert_eq!(
            crate::lock::lock(&workspace.store).global_chart_appearance(),
            Some(initial_global)
        );

        // `Done` after editing the chart's appearance saves the new look.
        workspace.settings_chart = Some(chart_id);
        workspace.ensure_settings_baseline(chart_id);
        let mut edited = workspace.charts[0].chart.appearance();
        edited.bullish = [12, 34, 56];
        edited.style.chart_type = crate::model::ChartType::Renko;
        workspace.charts[0].chart.set_appearance(edited);
        workspace.handle_settings_action(chart_id, Some(ChartSettingsAction::Done));
        assert_eq!(
            crate::lock::lock(&workspace.store).global_chart_appearance(),
            Some(edited)
        );

        // `ApplyToAll(ColorsAndCanvas)` + `Done` preserves the `ColorsAndCanvas`-merged global look.
        let mut base = crate::model::ChartAppearance {
            bullish: [1, 2, 3],
            ..Default::default()
        };
        base.style.chart_type = crate::model::ChartType::Kagi;
        crate::lock::lock(&workspace.store)
            .set_global_chart_appearance(&base)
            .expect("seed base");

        workspace.settings_chart = Some(chart_id);
        workspace.ensure_settings_baseline(chart_id);
        workspace.handle_settings_action(
            chart_id,
            Some(ChartSettingsAction::ApplyToAll(ApplyScope::ColorsAndCanvas)),
        );
        workspace.handle_settings_action(chart_id, Some(ChartSettingsAction::Done));

        let saved = crate::lock::lock(&workspace.store)
            .global_chart_appearance()
            .expect("global");
        assert_eq!(saved.bullish, [12, 34, 56]);
        assert_eq!(
            saved.style.chart_type,
            crate::model::ChartType::Kagi,
            "Done must not overwrite the ColorsAndCanvas merge with the source chart's Renko type"
        );
    }
}
