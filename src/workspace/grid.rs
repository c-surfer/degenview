//! The chart grid: drawing the columns of chart cards, dragging cards between them, and applying what the pass collected.

use super::ChartWorkspace;
use super::chart::displayed_candles;
use crate::host::paper::PaperHost;
use crate::lock::lock;
use crate::model::watchlist::WatchlistError;
use crate::model::{
    ChartColumn, ChartLayout, Drawing, DrawingCommand, DrawingStyle, InstrumentId, Tool,
};
use crate::ui::ViewMove;
use crate::ui::chart::settings::ChartSettingsState;
use crate::ui::{self, ChartCardAction, CrosshairInfo, DrawingFrame, PickKind, ReplayChartFrame};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

pub const COLUMN_SPLITTER_WIDTH: f32 = 6.0;
pub const ROW_SPLITTER_HEIGHT: f32 = 6.0;
pub const MIN_COLUMN_WIDTH: f32 = 180.0;
pub const MIN_ROW_HEIGHT: f32 = 80.0;

/// A deferred mutation the grid collects while drawing, applied after the pass.
enum ChartCommand {
    Focus(u64),
    Settings(u64),
    Remove(u64),
    Indicator(u64, crate::ui::chart::indicator_strip::Action),
    Move {
        chart_id: u64,
        to_column: u64,
        before: Option<u64>,
    },
    NewColumn(u64),
}

/// The crosshair as it stays while the pointer is on `chart_id`'s overlay card, which takes the
/// pointer from the plot: where it was, drawn as readouts only. Only the chart that owned the
/// crosshair keeps it.
fn freeze(previous: Option<CrosshairInfo>, chart_id: u64) -> Option<CrosshairInfo> {
    previous
        .filter(|crosshair| crosshair.owner_id == chart_id)
        .map(|crosshair| CrosshairInfo {
            frozen: true,
            ..crosshair
        })
}

/// What a star or membership toggle has to report: finding the market already in (or already out
/// of) the list is no failure; anything else says why it was refused.
fn star_result(result: Result<bool, WatchlistError>) -> Result<(), WatchlistError> {
    match result {
        Err(WatchlistError::Duplicate(_) | WatchlistError::NotFound) => Ok(()),
        other => other.map(|_| ()),
    }
}

/// The markets of the Favorites list, by `InstrumentId::key`, so the star reads "on" for a
/// market however its row was spelled and agrees with what adding to the list refuses.
fn favorite_keys(lists: &[crate::model::watchlist::Watchlist]) -> HashSet<String> {
    lists
        .iter()
        .find(|list| list.is_favorites)
        .into_iter()
        .flat_map(|list| list.instruments())
        .map(|item| item.instrument.key())
        .collect()
}

impl ChartWorkspace {
    /// Applies a grid preset, duplicating the focused chart if more charts are needed,
    /// arranging columns accordingly, and preserving or fixing chart focus.
    pub fn apply_grid_preset(
        &mut self,
        preset: crate::model::GridLayoutPreset,
        ctx: &egui::Context,
    ) {
        if self.charts.is_empty() {
            return;
        }

        let needed = preset.required_charts();
        let initial_focus = self.focused;
        let target_id = self
            .focused
            .filter(|id| self.charts.iter().any(|c| c.id == *id))
            .or_else(|| self.charts.first().map(|c| c.id));

        if let Some(target_id) = target_id {
            while self.charts.len() < needed {
                if self.duplicate_chart(target_id, ctx).is_none() {
                    break;
                }
            }

            if self.charts.len() > needed {
                let mut kept_ids = Vec::with_capacity(needed);
                kept_ids.push(target_id);
                for c in &self.charts {
                    if kept_ids.len() >= needed {
                        break;
                    }
                    if c.id != target_id {
                        kept_ids.push(c.id);
                    }
                }
                let to_remove: Vec<u64> = self
                    .charts
                    .iter()
                    .map(|c| c.id)
                    .filter(|id| !kept_ids.contains(id))
                    .collect();
                for id in to_remove {
                    self.remove_chart(id);
                }
            }

            if let Some(pos) = self.charts.iter().position(|c| c.id == target_id) {
                if pos > 0 {
                    let focused_chart = self.charts.remove(pos);
                    self.charts.insert(0, focused_chart);
                }
            }
        }

        let chart_ids: Vec<u64> = self.charts.iter().map(|c| c.id).collect();
        let mut next_col_id = self.next_id;
        self.columns = preset.arrange_columns(&chart_ids, &mut next_col_id);
        self.next_id = next_col_id;

        self.row_weights.clear();
        let col_count = self.columns.len().max(1);
        match preset {
            crate::model::GridLayoutPreset::Single => {
                self.column_weights = vec![1.0];
            }
            crate::model::GridLayoutPreset::ThreeLeftFocus => {
                if col_count == 2 {
                    self.column_weights = vec![0.65, 0.35];
                } else {
                    self.column_weights = vec![1.0 / col_count as f32; col_count];
                }
            }
            crate::model::GridLayoutPreset::ThreeRightFocus => {
                if col_count == 2 {
                    self.column_weights = vec![0.35, 0.65];
                } else {
                    self.column_weights = vec![1.0 / col_count as f32; col_count];
                }
            }
            crate::model::GridLayoutPreset::FiveSplit => {
                if col_count == 2 {
                    self.column_weights = vec![0.65, 0.35];
                } else {
                    self.column_weights = vec![1.0 / col_count as f32; col_count];
                }
            }
            _ => {
                self.column_weights = vec![1.0 / col_count as f32; col_count];
            }
        }

        match preset {
            crate::model::GridLayoutPreset::FiveSplit if self.columns.len() == 2 => {
                let left_col_id = self.columns[0].id;
                let right_col_id = self.columns[1].id;
                self.row_weights.insert(left_col_id, vec![0.5, 0.5]);
                self.row_weights
                    .insert(right_col_id, vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0]);
            }
            _ => {
                for col in &self.columns {
                    let row_count = col.chart_ids.len().max(1);
                    self.row_weights
                        .insert(col.id, vec![1.0 / row_count as f32; row_count]);
                }
            }
        }

        self.focused = initial_focus
            .filter(|id| self.charts.iter().any(|c| c.id == *id))
            .or_else(|| self.charts.first().map(|c| c.id));

        ctx.request_repaint();
    }

    pub fn validate_grid_weights(&mut self) {
        let col_count = self.columns.len();
        if col_count == 0 {
            self.column_weights.clear();
            self.row_weights.clear();
            return;
        }

        if self.column_weights.len() != col_count {
            self.column_weights = vec![1.0 / col_count as f32; col_count];
        } else {
            let sum: f32 = self.column_weights.iter().sum();
            if sum <= 0.0 || sum.is_nan() || (sum - 1.0).abs() > 0.001 {
                if sum > 0.0 && !sum.is_nan() {
                    for w in &mut self.column_weights {
                        *w /= sum;
                    }
                } else {
                    self.column_weights = vec![1.0 / col_count as f32; col_count];
                }
            }
        }

        let valid_cols: HashSet<u64> = self.columns.iter().map(|c| c.id).collect();
        self.row_weights.retain(|id, _| valid_cols.contains(id));

        for column in &self.columns {
            let row_count = column.chart_ids.len().max(1);
            let weights = self
                .row_weights
                .entry(column.id)
                .or_insert_with(|| vec![1.0 / row_count as f32; row_count]);
            if weights.len() != row_count {
                *weights = vec![1.0 / row_count as f32; row_count];
            } else {
                let sum: f32 = weights.iter().sum();
                if sum <= 0.0 || sum.is_nan() || (sum - 1.0).abs() > 0.001 {
                    if sum > 0.0 && !sum.is_nan() {
                        for w in weights.iter_mut() {
                            *w /= sum;
                        }
                    } else {
                        *weights = vec![1.0 / row_count as f32; row_count];
                    }
                }
            }
        }
    }

    pub fn resize_column(&mut self, index: usize, delta_x: f32, usable_width: f32) {
        if usable_width <= 1.0 || delta_x == 0.0 {
            return;
        }
        if index + 1 >= self.column_weights.len() {
            return;
        }
        let delta_w = delta_x / usable_width;
        let sum_w = self.column_weights[index] + self.column_weights[index + 1];
        let min_w = (MIN_COLUMN_WIDTH / usable_width)
            .max(0.10)
            .min(sum_w * 0.45);
        let max_w = sum_w - min_w;
        if min_w < max_w {
            let new_left = (self.column_weights[index] + delta_w).clamp(min_w, max_w);
            self.column_weights[index] = new_left;
            self.column_weights[index + 1] = sum_w - new_left;
        }
    }

    pub fn resize_row(
        &mut self,
        column_id: u64,
        row_index: usize,
        delta_y: f32,
        usable_height: f32,
    ) {
        if usable_height <= 1.0 || delta_y == 0.0 {
            return;
        }
        let Some(weights) = self.row_weights.get_mut(&column_id) else {
            return;
        };
        if row_index + 1 >= weights.len() {
            return;
        }
        let delta_w = delta_y / usable_height;
        let sum_w = weights[row_index] + weights[row_index + 1];
        let min_w = (MIN_ROW_HEIGHT / usable_height)
            .max(0.10)
            .min(sum_w * 0.45);
        let max_w = sum_w - min_w;
        if min_w < max_w {
            let new_top = (weights[row_index] + delta_w).clamp(min_w, max_w);
            weights[row_index] = new_top;
            weights[row_index + 1] = sum_w - new_top;
        }
    }

    pub(super) fn place_in_grid(&mut self, chart_id: u64) {
        if self.columns.is_empty() || self.columns.len() < 2 {
            let column_id = self.fresh_id();
            self.columns
                .push(ChartColumn::new(column_id, vec![chart_id]));
        } else if let Some(shortest) = self
            .columns
            .iter()
            .map(|column| column.chart_ids.len())
            .enumerate()
            .min_by_key(|(index, len)| (*len, *index))
            .map(|(index, _)| index)
        {
            self.columns[shortest].chart_ids.push(chart_id);
        }
        self.validate_grid_weights();
    }

    fn move_chart(&mut self, chart_id: u64, to_column: u64, before: Option<u64>) {
        if before == Some(chart_id) || !self.columns.iter().any(|c| c.id == to_column) {
            return;
        }
        let original = self.columns.clone();

        for column in &mut self.columns {
            column.chart_ids.retain(|id| *id != chart_id);
        }
        self.columns
            .retain(|column| !column.chart_ids.is_empty() || column.id == to_column);

        let Some(column_index) = self
            .columns
            .iter()
            .position(|column| column.id == to_column)
        else {
            self.columns = original;
            return;
        };
        let insertion = before
            .and_then(|target| {
                self.columns[column_index]
                    .chart_ids
                    .iter()
                    .position(|id| *id == target)
            })
            .unwrap_or(self.columns[column_index].chart_ids.len());
        self.columns[column_index]
            .chart_ids
            .insert(insertion, chart_id);
        self.validate_grid_weights();
    }

    fn move_to_new_trailing_column(&mut self, chart_id: u64) {
        if !self.charts.iter().any(|chart| chart.id == chart_id) {
            return;
        }
        for column in &mut self.columns {
            column.chart_ids.retain(|id| *id != chart_id);
        }
        self.columns.retain(|column| !column.chart_ids.is_empty());
        let column_id = self.fresh_id();
        self.columns
            .push(ChartColumn::new(column_id, vec![chart_id]));
        self.validate_grid_weights();
    }

    /// While a chart is dragged: finds what is under the pointer, draws its cue (an insert
    /// line, or the new-column strip and its preview) and, on release, queues the move.
    #[allow(clippy::too_many_arguments)]
    fn drive_grid_drag(
        &mut self,
        ctx: &egui::Context,
        dragged: Option<u64>,
        grid_rect: egui::Rect,
        mut zones: ui::workspace::grid_drop::Zones,
        can_add_column: bool,
        previewed: Option<u64>,
        commands: &mut Vec<ChartCommand>,
    ) {
        use ui::workspace::grid_drop::{self, DropTarget};
        let Some(dragged) = dragged else {
            return;
        };
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            egui::DragAndDrop::clear_payload(ctx);
            self.grid_drag.reset();
            return;
        }
        if can_add_column && zones.new_column.is_none() {
            zones.new_column = Some(egui::Rect::from_min_max(
                egui::pos2(
                    grid_rect.right() - grid_drop::NEW_COLUMN_STRIP_WIDTH,
                    grid_rect.top(),
                ),
                grid_rect.right_bottom(),
            ));
        }
        let target = ctx
            .pointer_latest_pos()
            .and_then(|pos| zones.target_at(pos));
        if let Some(wait) = self.grid_drag.hover(dragged, target, Instant::now()) {
            ctx.request_repaint_after(wait);
        }

        // Cues sit above the cards, on the grid's own layer.
        let painter_ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("grid-drop-cues"),
            egui::UiBuilder::new()
                .layer_id(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("grid-drop-cues-layer"),
                ))
                .max_rect(grid_rect),
        );
        if can_add_column
            && previewed.is_none()
            && let Some(strip) = zones.new_column
        {
            grid_drop::new_column_strip(&painter_ui, strip, target == Some(DropTarget::NewColumn));
        }
        match target {
            Some(DropTarget::Before { chart, .. }) if chart != dragged => {
                if let Some((_, _, rect)) = zones.cards.iter().find(|(_, id, _)| *id == chart) {
                    grid_drop::insert_line_above(&painter_ui, *rect);
                }
            }
            Some(DropTarget::Append { column }) => {
                if let Some((_, rect)) = zones.columns.iter().find(|(id, _)| *id == column) {
                    grid_drop::insert_line_below(&painter_ui, *rect);
                }
            }
            _ => {}
        }
        if let Some(chart) = self.charts.iter().find(|chart| chart.id == dragged) {
            grid_drop::drag_label(ctx, &chart.display_title());
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);

        if ctx.input(|input| input.pointer.any_released()) {
            match target {
                Some(DropTarget::Before { column, chart }) if chart != dragged => {
                    commands.push(ChartCommand::Move {
                        chart_id: dragged,
                        to_column: column,
                        before: Some(chart),
                    });
                }
                Some(DropTarget::Append { column }) => commands.push(ChartCommand::Move {
                    chart_id: dragged,
                    to_column: column,
                    before: None,
                }),
                Some(DropTarget::NewColumn) => commands.push(ChartCommand::NewColumn(dragged)),
                _ => {}
            }
            self.grid_drag.reset();
        }
    }

    pub(super) fn render_grid(
        &mut self,
        ui: &mut egui::Ui,
        paper_host: &mut PaperHost,
        window: egui::Rect,
    ) {
        if self.charts.is_empty() {
            self.show_empty_state(ui);
            return;
        }
        // The next visit to the empty page starts from fresh data.
        self.empty_page.reset();

        let available = ui.available_size();
        let column_count = self.columns.len().max(1);
        let max_rows = self
            .columns
            .iter()
            .map(|column| column.chart_ids.len())
            .max()
            .unwrap_or(0);
        let card_height =
            ChartLayout::grid_card_height(available.y, max_rows, ui.spacing().item_spacing.y);

        let plan = self.columns.clone();
        let index_map: HashMap<u64, usize> = self
            .charts
            .iter()
            .enumerate()
            .map(|(index, chart)| (chart.id, index))
            .collect();
        let focused = self.focused;
        let multi = self.charts.len() > 1;
        let crosshair = self.crosshair;
        let mut crosshair_update: Option<CrosshairInfo> = None;
        let mut hover_update: Option<(u64, crate::model::Anchor)> = None;
        self.tools.give_pointer_to_tool();
        self.give_pointer_to_tool_over_selection();
        let selecting_portion = self.is_selecting_portion();
        let (active_tool, zoom_armed) = (self.tools.active_tool(), self.tools.is_zoom_armed());
        let cursor = self.tools.cursor();
        let values_tooltip = self.tools.values_tooltip();
        let selected_drawing = self.tools.selection().cloned();
        let (magnet, keep_drawing, hide_all_drawings) = (
            self.tools.magnet(),
            self.tools.keeps_drawing(),
            self.tools.hides_all(),
        );
        let replay_ms = self.replay.cut_ms();
        let replay_selecting = self.replay.is_selecting_start();
        // The logs pane's drag over bars: only its own chart takes it, and a replay's start wins
        // if both were somehow asked for.
        if replay_selecting {
            self.context.logs.picking = false;
        }
        let logs_chart = self.context.logs.chart;
        let logs_picking = self.context.logs.picking;
        let logs_range = self
            .context
            .logs
            .view
            .filter
            .range
            .map(|range| (range.from_ms, range.to_ms));
        let mut logs_picked: Option<(i64, i64)> = None;
        let replay_active = self.replay.is_active();
        let mut replay_picked: Option<(u64, i64)> = None;
        // Linked navigation needs another chart to follow.
        let sync = self.sync;
        let linked = self.market_charts() > 1 && sync.is_linked();
        let mut navigated = false;
        // Tab Linking moves and listens to the focused chart only. (A change of what is shared is
        // dealt with when the next event arrives, not here: a tab that was hidden when the
        // switches changed must still hear what was said while it was.)
        let link_view = self.link_view_settings();
        let link_focus = link_view
            .is_linked()
            .then(|| self.resolved_focused_id())
            .flatten();
        // The chart that follows the group had no bars to follow with this frame.
        let mut link_waiting = false;

        let mut drawing_commands: Vec<(u64, Vec<DrawingCommand>)> = Vec::new();
        let mut drawing_select: Option<(u64, Vec<u64>)> = None;
        let mut drawing_disarm = false;
        let picture_work = self.content.work_snapshot(self.focused);
        let mut picture_used = false;
        let mut picture_replace: Option<(u64, u64)> = None;
        let mut picture_copy: Option<String> = None;
        let mut link_request: Option<(u64, u64, ui::LinkRequest)> = None;
        let mut drawing_properties_request: Option<(u64, u64)> = None;
        let mut drawing_style_copied: Option<(Tool, DrawingStyle)> = None;
        let style_clipboard = self.tools.style_clipboard();
        let dialog_open =
            self.tools.has_dialog() || self.content.sheet_open() || self.studio_open();
        let mut drawing_reveal = false;
        let mut drawing_copy = false;
        let mut drawing_alert: Option<(u64, f64)> = None;
        let drawing_defaults = self.drawing_templates().defaults().clone();
        let drawing_preview: Option<(u64, Drawing)> = self.tools.previewed_draft();
        let mut drawing_escape = false;
        self.reserve_escape_for_favorites_menu(ui.ctx());
        let mut paper_actions = Vec::new();
        let mut quick_trade = None;
        let mut live_actions = Vec::new();
        let mut live_trade = None;
        let mut context_request: Option<(u64, ui::ContextRequest)> = None;
        let (favorites, watchlist_memberships): (HashSet<String>, Vec<_>) = {
            let store = lock(&self.watchlist);
            let favorites = favorite_keys(store.lists());
            let memberships = store
                .lists()
                .iter()
                .map(|list| {
                    (
                        list.id,
                        list.name.clone(),
                        list.instruments()
                            .map(|item| item.instrument.key())
                            .collect::<HashSet<String>>(),
                    )
                })
                .collect();
            (favorites, memberships)
        };
        let mut favorite_toggles = Vec::new();
        let mut watchlist_changes = Vec::new();
        let card_env = self.card_env.clone();

        let mut commands: Vec<ChartCommand> = Vec::new();
        let can_add_column = ChartLayout::can_add_column(available.x, column_count);
        let ctx = ui.ctx().clone();
        // Nothing about a drag shows until a chart is being dragged.
        let dragged: Option<u64> = egui::DragAndDrop::payload::<u64>(&ctx).map(|payload| *payload);
        if dragged.is_none() {
            self.grid_drag.reset();
        }
        let previewed = if can_add_column {
            self.grid_drag.previewed()
        } else {
            None
        };
        let mut zones = ui::workspace::grid_drop::Zones::default();
        let grid_rect = ui.available_rect_before_wrap();

        self.validate_grid_weights();

        let total_slots = column_count + usize::from(previewed.is_some());
        let total_splitters = total_slots.saturating_sub(1);
        let total_splitters_width = total_splitters as f32 * COLUMN_SPLITTER_WIDTH;
        let usable_width = (available.x - total_splitters_width).max(0.0);

        let preview_weight = if previewed.is_some() {
            1.0 / total_slots as f32
        } else {
            0.0
        };

        let min_col_w = MIN_COLUMN_WIDTH.min(usable_width / total_slots as f32);
        let col_widths: Vec<f32> = (0..total_slots)
            .map(|col_idx| {
                if col_idx < column_count {
                    let w = self
                        .column_weights
                        .get(col_idx)
                        .copied()
                        .unwrap_or(1.0 / column_count as f32)
                        * (1.0 - preview_weight);
                    (usable_width * w).max(min_col_w)
                } else {
                    (usable_width * preview_weight).max(min_col_w)
                }
            })
            .collect();

        let row_weights_snapshot = self.row_weights.clone();
        let mut col_resizes: Vec<(usize, f32)> = Vec::new();
        let mut row_resizes: Vec<(u64, usize, f32)> = Vec::new();

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            for column_index in 0..total_slots {
                let col_width = col_widths[column_index];
                ui.allocate_ui_with_layout(
                    egui::vec2(col_width, available.y),
                    egui::Layout::top_down(egui::Align::Min),
                    |column_ui| {
                        let Some(column) = plan.get(column_index) else {
                            // The previewed new column: a dashed placeholder where it would be.
                            let rect = column_ui.max_rect();
                            let card = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(rect.width(), card_height),
                            );
                            ui::workspace::grid_drop::new_column_placeholder(column_ui, card);
                            zones.new_column = Some(egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(rect.width(), available.y),
                            ));
                            return;
                        };
                        let column_rect = column_ui.max_rect();
                        zones.columns.push((
                            column.id,
                            egui::Rect::from_min_size(
                                column_rect.min,
                                egui::vec2(col_width, available.y),
                            ),
                        ));

                        let row_count = column.chart_ids.len();
                        if row_count == 0 {
                            return;
                        }
                        let total_row_splitters = row_count.saturating_sub(1);
                        let total_row_splitters_h =
                            total_row_splitters as f32 * ROW_SPLITTER_HEIGHT;
                        let usable_h = (available.y - total_row_splitters_h).max(0.0);
                        let r_weights = row_weights_snapshot
                            .get(&column.id)
                            .cloned()
                            .unwrap_or_else(|| vec![1.0 / row_count as f32; row_count]);

                        let min_row_h = MIN_ROW_HEIGHT.min(usable_h / row_count as f32);
                        column_ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);

                        for (row_index, &chart_id) in column.chart_ids.iter().enumerate() {
                            let row_weight = r_weights
                                .get(row_index)
                                .copied()
                                .unwrap_or(1.0 / row_count as f32);
                            let this_card_height = if row_count == 1 {
                                available.y
                            } else {
                                (usable_h * row_weight).max(min_row_h)
                            };

                            let Some(&index) = index_map.get(&chart_id) else {
                                continue;
                            };
                            let is_focused = multi && focused == Some(chart_id);
                            let card = egui::Frame::new()
                                .fill(ui::theme::surface())
                                .corner_radius(10.0)
                                .inner_margin(egui::Margin::same(6))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    if replay_ms.is_some() {
                                        ui::theme::REPLAY.gamma_multiply(0.45)
                                    } else if is_focused {
                                        ui::theme::ACCENT
                                    } else {
                                        ui::theme::border()
                                    },
                                ));

                            let drawn = column_ui.allocate_ui_with_layout(
                                egui::vec2(column_ui.available_width(), this_card_height),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    if previewed == Some(chart_id) {
                                        ui.multiply_opacity(0.35);
                                    }
                                    let chart = &mut self.charts[index];
                                    if let Some(mut shown) = chart.card {
                                        let instrument = shown.instrument_id();
                                        let memberships: Vec<crate::ui::WatchlistMembership> =
                                            watchlist_memberships
                                                .iter()
                                                .map(|(id, name, instruments)| {
                                                    crate::ui::WatchlistMembership {
                                                        id: *id,
                                                        name: name.clone(),
                                                        contains_market: instruments
                                                            .contains(&instrument.key()),
                                                    }
                                                })
                                                .collect();
                                        return card.show(ui, |ui| {
                                            let mut action = ChartCardAction::default();
                                            let data = card_env.as_ref().map(|env| {
                                                let runtime = chart
                                                    .card_runtime
                                                    .get_or_insert_with(|| env.runtime());
                                                env.view(runtime, &shown, ui.ctx())
                                            });
                                            let scope = data.as_ref().map_or_else(
                                                || shown.scope_name(&[]),
                                                |data| data.scope_name.clone(),
                                            );
                                            ui::portfolio::cards::header(
                                                ui,
                                                &shown,
                                                &scope,
                                                chart_id,
                                                favorites.contains(&instrument.key()),
                                                &memberships,
                                                &mut action,
                                            );
                                            if ui::portfolio::cards::body(
                                                ui,
                                                &mut shown,
                                                data.as_ref(),
                                                chart_id,
                                            ) {
                                                action.focus_requested = true;
                                            }
                                            chart.card = Some(shown);
                                            action
                                        });
                                    }
                                    chart.chart.set_series_cutoff(replay_ms);
                                    chart.chart.set_timeframe(chart.timeframe);
                                    let candles = displayed_candles(&chart.candles, replay_ms);
                                    if linked {
                                        chart.chart.follow_sync(
                                            &mut self.sync_hub,
                                            chart_id,
                                            sync,
                                            chart.source,
                                            candles,
                                        );
                                    }
                                    if link_focus == Some(chart_id) && candles.is_empty() {
                                        link_waiting = true;
                                    }
                                    if link_focus == Some(chart_id) && !candles.is_empty() {
                                        // A follower's move is made before the frame's baseline,
                                        // so it is never announced back.
                                        chart.chart.follow_sync(
                                            &mut self.link_state.hub,
                                            chart_id,
                                            link_view,
                                            chart.source,
                                            candles,
                                        );
                                    }
                                    let instrument =
                                        InstrumentId::new(chart.source, &chart.symbol);
                                    let memberships: Vec<crate::ui::WatchlistMembership> =
                                        watchlist_memberships
                                            .iter()
                                            .map(|(id, name, instruments)| {
                                                crate::ui::WatchlistMembership {
                                                    id: *id,
                                                    name: name.clone(),
                                                    contains_market: instruments
                                                        .contains(&instrument.key()),
                                                }
                                            })
                                            .collect();
                                    card.show(ui, |ui| {
                                        let mut action = ChartCardAction::default();
                                        // The chart reaches the card's top edge and the header is drawn
                                        // over it afterwards, so its widgets sit above the plot's own
                                        // pointer area and its empty space still pans the chart.
                                        let header_rect = egui::Rect::from_min_size(
                                            ui.available_rect_before_wrap().min,
                                            egui::vec2(
                                                ui.available_width(),
                                                ui::CHART_HEADER_HEIGHT,
                                            ),
                                        );
                                        chart.chart.set_top_inset(ui::CHART_HEADER_HEIGHT);
                                        let selected = selected_drawing
                                            .as_ref()
                                            .filter(|(chart, _)| *chart == chart_id)
                                            .map(|(_, drawings)| drawings.clone())
                                            .unwrap_or_default();
                                        let (pending, place_now, replace_with) =
                                            picture_work.for_chart(chart_id);
                                        let mut frame = DrawingFrame::new(
                                            &chart.drawings,
                                            active_tool,
                                            None,
                                            chart.next_drawing_id,
                                        )
                                        .with_pending_content(pending)
                                        .with_picture_work(place_now, replace_with)
                                        .with_selection(selected)
                                        .with_view(chart.timeframe, hide_all_drawings)
                                        .with_behavior(magnet, keep_drawing)
                                        .with_style_clipboard(style_clipboard)
                                        .with_keys_blocked(dialog_open)
                                        .with_cursor(cursor)
                                        .with_defaults(&drawing_defaults)
                                        .with_preview(
                                            drawing_preview
                                                .as_ref()
                                                .filter(|(preview_chart, _)| {
                                                    *preview_chart == chart_id
                                                })
                                                .map(|(_, draft)| draft),
                                        );
                                        chart.chart.set_zoom_armed(zoom_armed);
                                        chart.chart.set_cursor_mode(cursor);
                                        chart.chart.set_values_tooltip(values_tooltip);
                                        chart.chart.set_snapshot_select(selecting_portion);
                                        let for_logs = logs_chart == Some(chart_id);
                                        chart
                                            .chart
                                            .set_logs_range(logs_range.filter(|_| for_logs));
                                        chart.chart.set_pick_kind(
                                            if for_logs && logs_picking {
                                                PickKind::LogsRange
                                            } else {
                                                PickKind::Replay
                                            },
                                        );
                                        let mut replay_frame = ReplayChartFrame {
                                            // Select portion has the pointer: nothing is
                                            // picked meanwhile, and the pick waits.
                                            selecting: (replay_selecting
                                                || (for_logs && logs_picking))
                                                && !selecting_portion,
                                            active: replay_active,
                                            picked_ms: None,
                                            picked_range: None,
                                        };
                                        let mut pointer = chart.chart.body(
                                            ui,
                                            &chart.symbol,
                                            candles,
                                            chart.loading,
                                            chart.error.as_ref(),
                                            chart.scale_mode,
                                            chart.source,
                                            &chart.indicators,
                                            crosshair.as_ref(),
                                            chart_id,
                                            &mut frame,
                                            &mut replay_frame,
                                        );
                                        let mut header_ui = ui.new_child(
                                            egui::UiBuilder::new().max_rect(header_rect),
                                        );
                                        chart.chart.header(
                                            &mut header_ui,
                                            &chart.symbol,
                                            chart.source,
                                            &chart.indicators,
                                            candles,
                                            replay_ms.is_some(),
                                            favorites.contains(&instrument.key()),
                                            &memberships,
                                            &mut action,
                                            chart_id,
                                            multi,
                                        );
                                        if let Some(range) = replay_frame.picked_range {
                                            logs_picked = Some(range);
                                        }
                                        if let Some(picked_ms) = replay_frame.picked_ms {
                                            replay_picked = Some((chart_id, picked_ms));
                                        }
                                        if link_focus == Some(chart_id)
                                            && let Some(change) = chart.chart.link_change()
                                        {
                                            self.link_state.outbox.push(
                                                crate::model::tab::link::LinkSignal::View(
                                                    change,
                                                ),
                                            );
                                        }
                                        if linked
                                            && chart.chart.publish_sync(
                                                &mut self.sync_hub,
                                                chart_id,
                                                sync,
                                            )
                                        {
                                            navigated = true;
                                        }
                                        chart.next_drawing_id = frame.next_id;
                                        if let Some(select) = frame.select {
                                            drawing_select = Some((chart_id, select));
                                        }
                                        drawing_escape |= frame.escape_consumed;
                                        drawing_disarm |= frame.disarm;
                                        picture_used |= frame.content_used;
                                        if let Some(drawing_id) = frame.replace_request {
                                            picture_replace = Some((chart_id, drawing_id));
                                        }
                                        if frame.copy_image.is_some() {
                                            picture_copy = frame.copy_image.take();
                                        }
                                        if let Some((drawing_id, request)) = frame.link_request
                                        {
                                            link_request =
                                                Some((chart_id, drawing_id, request));
                                        }
                                        drawing_reveal |= frame.reveal_all;
                                        drawing_copy |= frame.copy_requested;
                                        if let Some(price) = frame.alert_price {
                                            drawing_alert = Some((chart_id, price));
                                        }
                                        if frame.style_copied.is_some() {
                                            drawing_style_copied = frame.style_copied;
                                        }
                                        if let Some(drawing_id) = frame.open_properties {
                                            drawing_properties_request =
                                                Some((chart_id, drawing_id));
                                        }
                                        let frame_commands =
                                            std::mem::take(&mut frame.commands);
                                        if !frame_commands.is_empty() {
                                            drawing_commands.push((chart_id, frame_commands));
                                        }
                                        if pointer.clicked {
                                            action.focus_requested = true;
                                        }
                                        if pointer.on_overlay_card && crosshair_update.is_none()
                                        {
                                            crosshair_update = freeze(crosshair, chart_id);
                                        }
                                        if let Some(overlay_action) =
                                            pointer.overlay_action.take()
                                        {
                                            commands.push(ChartCommand::Indicator(
                                                chart_id,
                                                overlay_action,
                                            ));
                                        }
                                        if let Some(anchor) = pointer.hover_anchor {
                                            hover_update = Some((chart_id, anchor));
                                        }
                                        if let Some(request) = pointer.context.clone() {
                                            context_request = Some((chart_id, request));
                                        }
                                        if let (Some(x_fraction), Some(price)) =
                                            (pointer.hover_x_fraction, pointer.hover_price)
                                        {
                                            crosshair_update = Some(CrosshairInfo {
                                                frozen: false,
                                                owner_id: chart_id,
                                                x_fraction,
                                                time: pointer.hover_time,
                                                price,
                                            });
                                        }
                                        action
                                    })
                                },
                            );

                            if drawn.inner.inner.focus_requested {
                                commands.push(ChartCommand::Focus(chart_id));
                            }
                            if drawn.inner.inner.remove_requested {
                                commands.push(ChartCommand::Remove(chart_id));
                            }
                            if drawn.inner.inner.settings_requested {
                                commands.push(ChartCommand::Settings(chart_id));
                            }
                            if let Some(action) = drawn.inner.inner.indicator_action {
                                commands.push(ChartCommand::Indicator(chart_id, action));
                            }
                            if drawn.inner.inner.toggle_favorite {
                                favorite_toggles.push(chart_id);
                            }
                            if let Some(change) = drawn.inner.inner.watchlist_change {
                                watchlist_changes.push((chart_id, change));
                            }
                            if let Some(side) = drawn.inner.inner.quick_trade {
                                let reference_price =
                                    displayed_candles(&self.charts[index].candles, replay_ms)
                                        .last()
                                        .map(|candle| candle.close);
                                quick_trade = Some((chart_id, side, reference_price));
                            }
                            paper_actions.append(&mut self.charts[index].chart.paper.actions);
                            live_actions.append(&mut self.charts[index].chart.live.actions);
                            if let Some(side) = drawn.inner.inner.live_trade {
                                live_trade = Some((chart_id, side));
                            }
                            zones.cards.push((column.id, chart_id, drawn.response.rect));

                            // Horizontal splitter between rows in this column
                            if row_index + 1 < row_count {
                                let (splitter_rect, response) = column_ui.allocate_exact_size(
                                    egui::vec2(column_ui.available_width(), ROW_SPLITTER_HEIGHT),
                                    egui::Sense::drag(),
                                );
                                if response.hovered() || response.dragged() {
                                    column_ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeRow);
                                }
                                let center_y = splitter_rect.center().y;
                                let line_color = if response.dragged() {
                                    ui::theme::ACCENT
                                } else if response.hovered() {
                                    ui::theme::ACCENT.gamma_multiply(0.7)
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                if line_color != egui::Color32::TRANSPARENT {
                                    column_ui.painter().line_segment(
                                        [
                                            egui::pos2(splitter_rect.left() + 4.0, center_y),
                                            egui::pos2(splitter_rect.right() - 4.0, center_y),
                                        ],
                                        egui::Stroke::new(2.0, line_color),
                                    );
                                }
                                if dragged.is_none() && response.dragged() {
                                    row_resizes.push((column.id, row_index, response.drag_delta().y));
                                }
                            }
                        }
                    },
                );

                // Vertical splitter between columns
                if column_index + 1 < total_slots {
                    let (splitter_rect, response) = ui.allocate_exact_size(
                        egui::vec2(COLUMN_SPLITTER_WIDTH, available.y),
                        egui::Sense::drag(),
                    );
                    if response.hovered() || response.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeColumn);
                    }
                    let center_x = splitter_rect.center().x;
                    let line_color = if response.dragged() {
                        ui::theme::ACCENT
                    } else if response.hovered() {
                        ui::theme::ACCENT.gamma_multiply(0.7)
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    if line_color != egui::Color32::TRANSPARENT {
                        ui.painter().line_segment(
                            [
                                egui::pos2(center_x, splitter_rect.top() + 4.0),
                                egui::pos2(center_x, splitter_rect.bottom() - 4.0),
                            ],
                            egui::Stroke::new(2.0, line_color),
                        );
                    }
                    if dragged.is_none() && response.dragged() && column_index + 1 < column_count {
                        col_resizes.push((column_index, response.drag_delta().x));
                    }
                }
            }
        });

        for (col_idx, delta_x) in col_resizes {
            self.resize_column(col_idx, delta_x, usable_width);
            ctx.request_repaint();
        }
        for (col_id, row_idx, delta_y) in row_resizes {
            let row_count = plan
                .iter()
                .find(|c| c.id == col_id)
                .map_or(1, |c| c.chart_ids.len());
            let total_row_splitters_h = (row_count.saturating_sub(1) as f32) * ROW_SPLITTER_HEIGHT;
            let usable_h = (available.y - total_row_splitters_h).max(0.0);
            self.resize_row(col_id, row_idx, delta_y, usable_h);
            ctx.request_repaint();
        }

        self.sync_hub.end_frame();
        self.link_state.end_frame(link_waiting);
        if navigated {
            // Charts drawn before the one being navigated hear of it one frame later: ask for it
            // at once.
            ctx.request_repaint();
        }

        self.drive_grid_drag(
            &ctx,
            dragged,
            grid_rect,
            zones,
            can_add_column,
            previewed,
            &mut commands,
        );
        drawing_escape |= self.drive_portion_selection(&ctx, grid_rect);
        self.show_favorites_bar(&ctx, grid_rect, window);

        if !favorite_toggles.is_empty() || !watchlist_changes.is_empty() {
            let mut store = lock(&self.watchlist);
            let list_id = store.favorites();
            for chart_id in favorite_toggles {
                if let Some(chart) = self.charts.iter().find(|chart| chart.id == chart_id) {
                    let instrument = Self::chart_instrument(chart);
                    let result = if favorites.contains(&instrument.key()) {
                        store.remove_from_list(list_id, &instrument)
                    } else {
                        store.add_to_list(list_id, self.favorite_item(chart))
                    };
                    self.watchlist_pane.report(star_result(result));
                }
            }
            for (chart_id, (list_id, included)) in watchlist_changes {
                if let Some(chart) = self.charts.iter().find(|chart| chart.id == chart_id) {
                    let instrument = Self::chart_instrument(chart);
                    let result = if included {
                        store.add_to_list(list_id, self.favorite_item(chart))
                    } else {
                        store.remove_from_list(list_id, &instrument)
                    };
                    self.watchlist_pane.report(star_result(result));
                }
            }
        }

        for command in commands {
            match command {
                ChartCommand::Focus(chart_id) => self.focus_chart(chart_id),
                ChartCommand::Remove(chart_id) => self.removing_chart = Some(chart_id),
                ChartCommand::Settings(chart_id) => {
                    self.settings_state = ChartSettingsState::default();
                    self.settings_chart = Some(chart_id);
                }
                ChartCommand::Indicator(chart_id, action) => {
                    use crate::ui::chart::indicator_strip::{Action, Kind};
                    match action {
                        Action::OpenIndicatorSettings => {
                            self.settings_state = ChartSettingsState::default();
                            self.settings_chart = Some(chart_id);
                        }
                        Action::ConfigureScript(id, anchor) => {
                            self.toggle_script_popover(chart_id, id, anchor);
                        }
                        Action::OpenPineLogs(id) => self.open_pine_logs(chart_id, Some(id)),
                        Action::ConfigureOverlay(id, anchor) => {
                            self.toggle_overlay_popover(chart_id, id, anchor);
                        }
                        Action::Overlay(command) => {
                            self.apply_overlay_command(chart_id, command);
                        }
                        Action::Add(_)
                        | Action::Remove(_)
                        | Action::AddScript(_)
                        | Action::AddBundled(_)
                        | Action::RemoveScript(_) => {
                            self.apply_indicator_action(chart_id, &action);
                        }
                        _ => {
                            if let Some(chart) = self.charts.iter_mut().find(|c| c.id == chart_id) {
                                match action {
                                    Action::EmaPeriod(period) => {
                                        chart.indicators.ema_period = period
                                    }
                                    Action::ToggleVisible(Kind::BuiltIn(kind)) => {
                                        kind.toggle_visible(&mut chart.indicators);
                                    }
                                    Action::ToggleVisible(Kind::Script(id)) => {
                                        if let Some(instance) =
                                            chart.chart.pine_instances().iter().find(|i| i.id == id)
                                        {
                                            chart.chart.set_pine_visible(id, !instance.visible);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                ChartCommand::Move {
                    chart_id,
                    to_column,
                    before,
                } => self.move_chart(chart_id, to_column, before),
                ChartCommand::NewColumn(chart_id) => self.move_to_new_trailing_column(chart_id),
            }
        }

        self.crosshair = crosshair_update;
        if let Some((chart_id, anchor)) = hover_update {
            self.note_hover(chart_id, Some(anchor));
        }
        if let Some((chart_id, request)) = context_request {
            self.open_context_menu(chart_id, request);
        }

        if let Some((from_ms, to_ms)) = logs_picked {
            self.context.logs.view.set_range(from_ms, to_ms);
            self.context.logs.picking = false;
        }

        if let Some((chart_id, picked_ms)) = replay_picked {
            let (symbol, timeline) = self
                .charts
                .iter()
                .find(|chart| chart.id == chart_id)
                .map(|chart| {
                    (
                        chart.symbol.clone(),
                        chart.candles.iter().map(|kline| kline.open_time).collect(),
                    )
                })
                .unwrap_or_default();
            self.replay.start(picked_ms, &symbol, timeline);
        }

        for (chart_id, drawing) in drawing_commands {
            self.apply_drawing_commands(chart_id, drawing);
        }
        self.apply_broker_overlay_actions(live_actions);
        if let Some((chart_id, side)) = live_trade
            && let Some(index) = self.chart_index(chart_id)
        {
            self.open_live_ticket(
                index,
                crate::model::chart::context::LiveOrderRequest {
                    side,
                    kind: degenview_brokers::model::ticket::TicketKind::Market,
                    price: None,
                },
            );
        }
        let paper_failures = Self::apply_paper_overlay_actions(paper_host, paper_actions);
        self.trade.report_paper_messages(&paper_failures);
        if let Some((_, side, _)) = quick_trade {
            self.start_order(
                match side {
                    crate::model::paper::PaperOrderSide::Buy => degenview_brokers::Side::Buy,
                    crate::model::paper::PaperOrderSide::Sell => degenview_brokers::Side::Sell,
                },
                Some(crate::trading::model::OrderClass::Market),
                None,
            );
        }
        if let Some((chart_id, select)) = drawing_select {
            if select.is_empty() {
                self.tools.clear_selection();
            } else {
                self.tools.select(chart_id, select);
            }
        }
        if drawing_disarm {
            self.tools.disarm();
        }
        // A zoom puts the tool down, is an undo step, and the chart it was made on becomes the
        // focused one, which is the one Zoom out goes back on. Every chart is asked, so none keeps
        // a stale flag.
        let zoomed: Vec<(u64, _)> = self
            .charts
            .iter_mut()
            .filter_map(|chart| chart.chart.take_zoomed().map(|before| (chart.id, before)))
            .collect();
        for &(chart_id, before) in &zoomed {
            self.remember_view(chart_id, before, ViewMove::Back);
        }
        if let Some(&(chart_id, _)) = zoomed.last() {
            self.tools.disarm_zoom();
            self.focus_chart(chart_id);
        }
        self.picture_work_done();
        if picture_used {
            self.picture_used();
            self.preview_placed_card(&ctx);
        }
        if let Some((chart_id, drawing_id)) = picture_replace {
            self.replace_picture_request(&ctx, chart_id, drawing_id);
        }
        if let Some(asset) = picture_copy {
            self.copy_picture(&ctx, &asset);
        }
        if let Some((chart_id, drawing_id, request)) = link_request {
            self.link_requested(&ctx, chart_id, drawing_id, request);
        }
        if drawing_reveal {
            self.tools.reveal_all();
        }
        if drawing_copy {
            self.copy_selection(&ctx);
        }
        if let Some((chart_id, price)) = drawing_alert {
            self.start_alert_at(chart_id, Some(price));
        }
        if let Some((tool, style)) = drawing_style_copied {
            self.tools.copy_style(tool, style);
        }
        if let Some((chart_id, drawing_id)) = drawing_properties_request {
            self.open_drawing_properties(chart_id, drawing_id);
        }
        let esc_pressed = ui.input(|input| input.key_pressed(egui::Key::Escape));
        if esc_pressed
            && !drawing_escape
            && !self.replay.is_choosing_date()
            && !self.context_menu_open()
            && !self.studio_open()
        {
            if self.replay.is_selecting_start() {
                self.replay.cancel_selection();
            } else if self.context.logs.picking {
                self.context.logs.picking = false;
            } else {
                self.tools.escape();
            }
        }
    }
}

#[cfg(test)]
mod favorite_tests {
    use super::*;
    use crate::model::DataSource;
    use crate::model::watchlist::{Watchlist, WatchlistInstrument};

    #[test]
    fn the_star_is_on_for_a_favorite_spelled_differently_from_the_chart() {
        let mut favorites = Watchlist::new("Favorites");
        favorites.is_favorites = true;
        for (source, symbol) in [
            (DataSource::Binance, "btc"),
            (DataSource::Coinbase, "ETH/USD"),
        ] {
            favorites
                .add(
                    WatchlistInstrument::new(InstrumentId::new(source, symbol), symbol, symbol),
                    None,
                )
                .unwrap();
        }
        let keys = favorite_keys(&[Watchlist::new("Other"), favorites]);
        for (source, symbol) in [
            (DataSource::Binance, "BTCUSDT"),
            (DataSource::Coinbase, "ETH-USD"),
        ] {
            assert!(keys.contains(&InstrumentId::new(source, symbol).key()));
        }
        assert!(!keys.contains(&InstrumentId::new(DataSource::Binance, "ETHUSDT").key()));
    }
}

#[cfg(test)]
mod freeze_tests {
    use super::*;

    fn crosshair(owner_id: u64) -> CrosshairInfo {
        CrosshairInfo {
            frozen: false,
            owner_id,
            x_fraction: 0.25,
            time: Some(1_700_000_000_000.0),
            price: 12.5,
        }
    }

    #[test]
    fn a_frozen_crosshair_stays_where_it_was_and_only_for_its_own_chart() {
        let frozen = freeze(Some(crosshair(3)), 3).expect("kept");
        assert!(frozen.frozen);
        assert_eq!(
            (frozen.x_fraction, frozen.time, frozen.price),
            (0.25, Some(1_700_000_000_000.0), 12.5)
        );
        assert!(
            freeze(Some(crosshair(3)), 4).is_none(),
            "another chart's is dropped"
        );
        assert!(freeze(None, 3).is_none());
        // Freezing again changes nothing: the pointer stays on the card for many frames.
        assert!(freeze(Some(frozen), 3).is_some_and(|again| again.frozen));
    }
}

#[cfg(test)]
mod grid_splitter_tests {
    use super::*;
    use crate::model::GridLayoutPreset;
    use crate::workspace::test_support::{chart_with_drawings, empty_workspace};

    #[test]
    fn three_left_focus_sets_65_35_weights_and_even_rows() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);

        workspace.apply_grid_preset(GridLayoutPreset::ThreeLeftFocus, &ctx);

        assert_eq!(workspace.column_weights(), &[0.65, 0.35]);
        let col0 = workspace.columns[0].id;
        let col1 = workspace.columns[1].id;
        assert_eq!(workspace.row_weights(col0), Some(&[1.0][..]));
        assert_eq!(workspace.row_weights(col1), Some(&[0.5, 0.5][..]));
    }

    #[test]
    fn three_right_focus_sets_35_65_weights_and_even_rows() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);

        workspace.apply_grid_preset(GridLayoutPreset::ThreeRightFocus, &ctx);

        assert_eq!(workspace.column_weights(), &[0.35, 0.65]);
        let col0 = workspace.columns[0].id;
        let col1 = workspace.columns[1].id;
        assert_eq!(workspace.row_weights(col0), Some(&[0.5, 0.5][..]));
        assert_eq!(workspace.row_weights(col1), Some(&[1.0][..]));
    }

    #[test]
    fn single_and_four_grid_preset_weights() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);

        workspace.apply_grid_preset(GridLayoutPreset::Single, &ctx);
        assert_eq!(workspace.column_weights(), &[1.0]);

        workspace.apply_grid_preset(GridLayoutPreset::FourGrid, &ctx);
        assert_eq!(workspace.column_weights(), &[0.5, 0.5]);
        let col0 = workspace.columns[0].id;
        let col1 = workspace.columns[1].id;
        assert_eq!(workspace.row_weights(col0), Some(&[0.5, 0.5][..]));
        assert_eq!(workspace.row_weights(col1), Some(&[0.5, 0.5][..]));
    }

    #[test]
    fn apply_grid_preset_reduces_charts_symmetrically() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);

        // 1 -> 4
        workspace.apply_grid_preset(GridLayoutPreset::FourGrid, &ctx);
        assert_eq!(workspace.charts.len(), 4);
        assert_eq!(workspace.columns.len(), 2);

        // 4 -> 2
        workspace.apply_grid_preset(GridLayoutPreset::TwoColumns, &ctx);
        assert_eq!(workspace.charts.len(), 2, "4 -> 2 must leave exactly 2 charts");
        assert_eq!(workspace.columns.len(), 2);
        assert_eq!(workspace.columns[0].chart_ids.len(), 1);
        assert_eq!(workspace.columns[1].chart_ids.len(), 1);
        assert_eq!(workspace.column_weights(), &[0.5, 0.5]);

        // 2 -> 4
        workspace.apply_grid_preset(GridLayoutPreset::FourGrid, &ctx);
        assert_eq!(workspace.charts.len(), 4);

        // 4 -> 3
        workspace.apply_grid_preset(GridLayoutPreset::ThreeLeftFocus, &ctx);
        assert_eq!(workspace.charts.len(), 3, "4 -> 3 must leave exactly 3 charts");
        assert_eq!(workspace.columns.len(), 2);
        assert_eq!(workspace.columns[0].chart_ids.len(), 1);
        assert_eq!(workspace.columns[1].chart_ids.len(), 2);
        assert_eq!(workspace.column_weights(), &[0.65, 0.35]);

        // 3 -> 2
        workspace.apply_grid_preset(GridLayoutPreset::TwoColumns, &ctx);
        assert_eq!(workspace.charts.len(), 2, "3 -> 2 must leave exactly 2 charts");
        assert_eq!(workspace.columns.len(), 2);
        assert_eq!(workspace.columns[0].chart_ids.len(), 1);
        assert_eq!(workspace.columns[1].chart_ids.len(), 1);
    }

    #[test]
    fn five_split_preset_creates_five_charts_and_sets_weights() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);

        workspace.apply_grid_preset(GridLayoutPreset::FiveSplit, &ctx);
        assert_eq!(workspace.charts.len(), 5);
        assert_eq!(workspace.columns.len(), 2);
        assert_eq!(workspace.columns[0].chart_ids.len(), 2);
        assert_eq!(workspace.columns[1].chart_ids.len(), 3);
        assert_eq!(workspace.column_weights(), &[0.65, 0.35]);

        let col0 = workspace.columns[0].id;
        let col1 = workspace.columns[1].id;
        assert_eq!(workspace.row_weights(col0), Some(&[0.5, 0.5][..]));
        let r1 = workspace.row_weights(col1).unwrap();
        assert_eq!(r1.len(), 3);
        for w in r1 {
            assert!((w - 1.0 / 3.0).abs() < 1e-4);
        }

        // 5 -> 3
        workspace.apply_grid_preset(GridLayoutPreset::ThreeLeftFocus, &ctx);
        assert_eq!(workspace.charts.len(), 3, "5 -> 3 must leave exactly 3 charts");
    }

    #[test]
    fn resize_column_preserves_total_sum_and_clamps() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);
        workspace.apply_grid_preset(GridLayoutPreset::ThreeLeftFocus, &ctx);

        // Usable width 1000px, drag right +50px -> +0.05
        workspace.resize_column(0, 50.0, 1000.0);
        let weights = workspace.column_weights();
        assert!((weights[0] - 0.70).abs() < 1e-4);
        assert!((weights[1] - 0.30).abs() < 1e-4);
        assert!((weights[0] + weights[1] - 1.0).abs() < 1e-4);

        // Huge drag delta should clamp to minimum without panic or NaN
        workspace.resize_column(0, 5000.0, 1000.0);
        let weights = workspace.column_weights();
        assert!(weights[1] >= 0.10);
        assert!((weights[0] + weights[1] - 1.0).abs() < 1e-4);

        workspace.resize_column(0, -5000.0, 1000.0);
        let weights = workspace.column_weights();
        assert!(weights[0] >= 0.10);
        assert!((weights[0] + weights[1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn resize_row_preserves_total_sum_and_clamps() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);
        workspace.apply_grid_preset(GridLayoutPreset::ThreeLeftFocus, &ctx);

        let right_col = workspace.columns[1].id;
        // Right column has 2 charts [0.5, 0.5], usable height 800px, drag down +40px -> +0.05
        workspace.resize_row(right_col, 0, 40.0, 800.0);
        let row_weights = workspace.row_weights(right_col).unwrap();
        assert!((row_weights[0] - 0.55).abs() < 1e-4);
        assert!((row_weights[1] - 0.45).abs() < 1e-4);
        assert!((row_weights[0] + row_weights[1] - 1.0).abs() < 1e-4);

        // Huge drag delta should clamp without panic
        workspace.resize_row(right_col, 0, 9999.0, 800.0);
        let row_weights = workspace.row_weights(right_col).unwrap();
        assert!(row_weights[1] >= 0.10);
        assert!((row_weights[0] + row_weights[1] - 1.0).abs() < 1e-4);
    }

    #[test]
    fn validate_grid_weights_resizes_when_column_count_changes() {
        let mut workspace = empty_workspace(1);
        let first = chart_with_drawings(&mut workspace, "BTCUSDT", &[]);
        let ctx = egui::Context::default();
        workspace.focused = Some(first);
        workspace.apply_grid_preset(GridLayoutPreset::ThreeLeftFocus, &ctx);

        assert_eq!(workspace.column_weights().len(), 2);

        // Add another column
        let new_col = workspace.fresh_id();
        let new_chart = workspace.fresh_id();
        workspace.columns.push(ChartColumn::new(new_col, vec![new_chart]));
        workspace.validate_grid_weights();

        assert_eq!(workspace.column_weights().len(), 3);
        let sum: f32 = workspace.column_weights().iter().sum();
        assert!((sum - 1.0).abs() < 1e-4);
    }
}
