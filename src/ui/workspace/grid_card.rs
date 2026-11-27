//! The chart cards of the grid: rounded, bordered panes separated by grid padding and splitters.

use crate::ui::theme;
use eframe::egui::{self, Color32, Margin, Stroke, StrokeKind, Ui};

/// Space between a card's edge and its inner content, keeping plots and controls inside the
/// rounded border.
pub const PADDING: i8 = 3;

/// Corner radius for every grid card.
pub const CORNER_RADIUS: f32 = 6.0;

/// How strongly an outline's colour shows: enough to find the card, not to compete with it.
const OUTLINE_STRENGTH: f32 = 0.65;

/// Returns the 1 px border stroke for a grid card: dimmed accent/replay when `outline` is set,
/// or the default theme border otherwise.
fn border_stroke(outline: Option<Color32>) -> Stroke {
    let color = match outline {
        Some(color) => color.gamma_multiply(OUTLINE_STRENGTH),
        None => theme::border(),
    };
    Stroke::new(1.0, color)
}

/// Draws one card around `contents`, inset by [`PADDING`] on all sides. `outline` marks the
/// card out (focus, replay).
pub fn show<R>(ui: &mut Ui, outline: Option<Color32>, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let drawn = egui::Frame::new()
        .fill(theme::surface())
        .corner_radius(CORNER_RADIUS)
        .inner_margin(Margin::same(PADDING))
        .show(ui, contents);
    let card = drawn.response.rect;
    ui.painter().rect_stroke(
        card,
        CORNER_RADIUS,
        border_stroke(outline),
        StrokeKind::Inside,
    );
    drawn.inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_metrics_match_spec() {
        assert_eq!(PADDING, 3);
        assert_eq!(CORNER_RADIUS, 6.0);
    }

    #[test]
    fn border_stroke_uses_theme_border_by_default_and_multiplies_outline() {
        let default_stroke = border_stroke(None);
        assert_eq!(default_stroke.width, 1.0);
        assert_eq!(default_stroke.color, theme::border());

        let accent = Color32::from_rgb(80, 160, 240);
        let focused_stroke = border_stroke(Some(accent));
        assert_eq!(focused_stroke.width, 1.0);
        assert_eq!(
            focused_stroke.color,
            accent.gamma_multiply(OUTLINE_STRENGTH)
        );
    }
}
