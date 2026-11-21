//! The Indicators page: the built-in indicators and their settings.

use super::ChartSettingsContext;
use crate::model::{
    BOLLINGER_MULTIPLIER, BOLLINGER_PERIOD, EMA_PERIODS, RSI_OVERBOUGHT, RSI_OVERSOLD, RSI_PERIOD,
    SUPERTREND_MULTIPLIER, SUPERTREND_PERIOD,
};
use crate::ui::icons::Icon;
use crate::ui::kit;
use crate::ui::settings::card;
use crate::ui::theme;
use crate::ui::tr;
use eframe::egui::{CursorIcon, Label, RichText, Sense, Ui};

const CLASSIC_SCALE_PERCENTAGES: [u8; 8] = [15, 20, 25, 30, 35, 40, 45, 50];
const ADAPTIVE_PERCENTAGES: [u8; 17] = [5, 6, 7, 8, 9, 10, 12, 14, 15, 16, 18, 20, 22, 25, 28, 30, 35];

fn pct_picker(ui: &mut Ui, current: &mut u8) {
    let labels: Vec<String> = ADAPTIVE_PERCENTAGES.iter().map(|p| format!("{p}%")).collect();
    let selected = ADAPTIVE_PERCENTAGES.iter().position(|&p| p == *current).unwrap_or(0);
    if let Some(idx) = kit::menu_picker(ui, 64.0, &labels, selected, true) {
        *current = ADAPTIVE_PERCENTAGES[idx];
    }
}

pub(super) fn indicators_page(ui: &mut Ui, c: &mut ChartSettingsContext) {
    card::section_title(
        ui,
        tr("Indicators", "Индикаторы"),
        Some(tr(
            "Add context to the chart without changing its underlying data.",
            "Добавление индикаторов на график без изменения исходных данных.",
        )),
    );
    ui.spacing_mut().item_spacing.y = 8.0;
    let source = c.source;
    let loaded = c.loaded_candles;
    let config = &mut *c.indicators;
    // Line sources report one price per timestamp and no turnover at all.
    if !source.is_line_chart() {
        let hint = if source.provides_volume() {
            tr(
                "Turnover per candle, drawn along the bottom of the chart.",
                "Оборот по каждой свече вдоль нижней части графика.",
            ).to_owned()
        } else {
            format!(
                "{} doesn't report per-candle volume, so bars aren't available for this chart.",
                source.display_name()
            )
        };
        card::card(ui, Icon::BarChart, tr("Volume Bars", "Объёмы (Volume)"), Some(&hint), |ui| {
            // Left visible rather than hidden: a greyed-out switch with a reason reads better
            // than a setting that silently does nothing.
            card::switch_enabled(ui, &mut config.show_volume, source.provides_volume());
        });
    }
    // RSI has no value for its first `period` candles, so a short range draws a stub of a
    // line or none at all.
    let rsi_hint = if loaded > 0 && loaded <= RSI_PERIOD {
        if crate::ui::i18n::is_ru() {
            format!("Загружено всего {loaded} свечей — для RSI нужно больше {RSI_PERIOD}.")
        } else {
            format!("Only {loaded} candles loaded — RSI needs more than {RSI_PERIOD}. Zoom out or pick a longer range.")
        }
    } else {
        if crate::ui::i18n::is_ru() {
            format!("Импульс от 0 до 100 внизу графика, с уровнями {} и {}.", RSI_OVERSOLD as i64, RSI_OVERBOUGHT as i64)
        } else {
            format!("Momentum from 0–100 across the bottom, with guides at {} and {}.", RSI_OVERSOLD as i64, RSI_OVERBOUGHT as i64)
        }
    };
    card::card(
        ui,
        Icon::Waveform,
        &format!("RSI ({RSI_PERIOD})"),
        Some(&rsi_hint),
        |ui| {
            card::switch(ui, &mut config.show_rsi);
        },
    );
    card::card(
        ui,
        Icon::ChartLine,
        "EMA",
        Some(tr(
            "Exponential moving average over the closes, drawn on the price scale.",
            "Экспоненциальная скользящая средняя по ценам закрытия.",
        )),
        |ui| {
            card::switch(ui, &mut config.show_ema);
            ui.add_space(4.0);
            let labels: Vec<String> = EMA_PERIODS.iter().map(usize::to_string).collect();
            let selected = EMA_PERIODS
                .iter()
                .position(|&period| period == config.ema_period)
                .unwrap_or(0);
            if let Some(index) = kit::menu_picker(ui, 72.0, &labels, selected, config.show_ema) {
                config.ema_period = EMA_PERIODS[index];
            }
        },
    );
    let bb_hint = if crate::ui::i18n::is_ru() {
        format!("{BOLLINGER_PERIOD}-периодная средняя с полосами на {} стандартных отклонениях — шире при волатильности, уже при штиле.", BOLLINGER_MULTIPLIER as i64)
    } else {
        format!("{BOLLINGER_PERIOD}-period average with bands {} standard deviations either side — wide when volatile, tight when calm.", BOLLINGER_MULTIPLIER as i64)
    };
    card::card(
        ui,
        Icon::Bands,
        tr("Bollinger Bands", "Полосы Боллинджера"),
        Some(&bb_hint),
        |ui| {
            card::switch(ui, &mut config.show_bollinger);
        },
    );
    let tf_hint = if crate::ui::i18n::is_ru() {
        format!("Бычьи и медвежьи метки по {SUPERTREND_PERIOD}-периодному ATR × {}. Сигналы появляются после закрытия свечи.", SUPERTREND_MULTIPLIER as i64)
    } else {
        format!("Bullish and bearish markers from {SUPERTREND_PERIOD}-period ATR × {}. Signals appear after the candle closes and can whipsaw in sideways markets.", SUPERTREND_MULTIPLIER as i64)
    };
    card::card(
        ui,
        Icon::Cycle,
        tr("Trend Flips", "Развороты тренда"),
        Some(&tf_hint),
        |ui| {
            card::switch(ui, &mut config.show_trend_flips);
        },
    );

    // Subpane Scale section
    ui.add_space(16.0);
    card::section_title(
        ui,
        tr("Subpane Scale", "Масштаб подвала"),
        Some(tr(
            "How indicator subpanes share vertical space.",
            "Распределение высоты для подвальных индикаторов.",
        )),
    );
    ui.spacing_mut().item_spacing.y = 8.0;

    // 1. Adaptive Scale (Smart)
    let mut adaptive_on = config.subpane_adaptive;
    let adaptive_summary = if crate::ui::i18n::is_ru() {
        format!(
            "Настраиваемые пропорции (1 инд: {}%, 2 инд: {}%/{}%, 3 инд: {}%/{}%/{}%).",
            config.adaptive_p1,
            config.adaptive_p2_top,
            config.adaptive_p2_bot,
            config.adaptive_p3_top,
            config.adaptive_p3_mid,
            config.adaptive_p3_bot,
        )
    } else {
        format!(
            "Custom proportions (1 ind: {}%, 2 ind: {}%/{}%, 3 ind: {}%/{}%/{}%).",
            config.adaptive_p1,
            config.adaptive_p2_top,
            config.adaptive_p2_bot,
            config.adaptive_p3_top,
            config.adaptive_p3_mid,
            config.adaptive_p3_bot,
        )
    };
    card::card(
        ui,
        Icon::Sliders,
        tr("Adaptive Scale (Smart)", "Адаптивный масштаб (Умный)"),
        Some(&adaptive_summary),
        |ui| {
            if card::switch(ui, &mut adaptive_on) {
                config.subpane_adaptive = adaptive_on;
            }
        },
    );

    if config.subpane_adaptive {
        ui.add_space(2.0);
        card::card(
            ui,
            Icon::ChartLine,
            tr("1 indicator height", "Высота при 1 индикаторе"),
            Some(tr("Total height share for 1 subpane", "Доля высоты окна при одном индикаторе")),
            |ui| {
                pct_picker(ui, &mut config.adaptive_p1);
            },
        );
        let sum2 = config.adaptive_p2_top + config.adaptive_p2_bot;
        let hint2 = format!("{}: {}% + {}% = {}%", tr("Total", "Сумма"), config.adaptive_p2_top, config.adaptive_p2_bot, sum2);
        card::card(
            ui,
            Icon::SplitColumns,
            tr("2 indicators heights", "Высота при 2 индикаторах"),
            Some(&hint2),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(kit::text(tr("Top:", "Верх:"), 11.0, kit::REGULAR, theme::text_secondary()));
                    pct_picker(ui, &mut config.adaptive_p2_top);
                    ui.add_space(8.0);
                    ui.label(kit::text(tr("Bottom:", "Низ:"), 11.0, kit::REGULAR, theme::text_secondary()));
                    pct_picker(ui, &mut config.adaptive_p2_bot);
                });
            },
        );
        let sum3 = config.adaptive_p3_top + config.adaptive_p3_mid + config.adaptive_p3_bot;
        let hint3 = format!("{}: {}% + {}% + {}% = {}%", tr("Total", "Сумма"), config.adaptive_p3_top, config.adaptive_p3_mid, config.adaptive_p3_bot, sum3);
        card::card(
            ui,
            Icon::BarChart,
            tr("3 indicators heights", "Высота при 3 индикаторах"),
            Some(&hint3),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(kit::text(tr("Top:", "Верх:"), 11.0, kit::REGULAR, theme::text_secondary()));
                    pct_picker(ui, &mut config.adaptive_p3_top);
                    ui.add_space(8.0);
                    ui.label(kit::text(tr("Mid:", "Центр:"), 11.0, kit::REGULAR, theme::text_secondary()));
                    pct_picker(ui, &mut config.adaptive_p3_mid);
                    ui.add_space(8.0);
                    ui.label(kit::text(tr("Bot:", "Низ:"), 11.0, kit::REGULAR, theme::text_secondary()));
                    pct_picker(ui, &mut config.adaptive_p3_bot);
                });
            },
        );

        let is_default_adaptive = config.adaptive_p1 == 20
            && config.adaptive_p2_top == 16
            && config.adaptive_p2_bot == 9
            && config.adaptive_p3_top == 15
            && config.adaptive_p3_mid == 8
            && config.adaptive_p3_bot == 7;
        if !is_default_adaptive {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                let reset = ui.add(
                    Label::new(
                        RichText::new(tr("Restore adaptive defaults", "Сбросить пропорции по умолчанию"))
                            .size(kit::size::SUBHEADLINE)
                            .underline()
                            .color(theme::text_secondary()),
                    )
                    .sense(Sense::click()),
                );
                if reset.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    config.adaptive_p1 = 20;
                    config.adaptive_p2_top = 16;
                    config.adaptive_p2_bot = 9;
                    config.adaptive_p3_top = 15;
                    config.adaptive_p3_mid = 8;
                    config.adaptive_p3_bot = 7;
                }
            });
        }
    }

    // 2. Classic Scale (Equal)
    let mut classic_on = !config.subpane_adaptive;
    let pct_labels: Vec<String> = CLASSIC_SCALE_PERCENTAGES.iter().map(|p| format!("{p}%")).collect();
    let selected_pct = CLASSIC_SCALE_PERCENTAGES
        .iter()
        .position(|&p| p == config.subpane_classic_pct)
        .unwrap_or(3); // default 30% is index 3

    card::card(
        ui,
        Icon::Percent,
        tr("Classic Scale (Equal)", "Классический масштаб"),
        Some(tr(
            "Fixed total share divided equally across all open indicators.",
            "Общая фиксированная высота делится поровну между всеми индикаторами.",
        )),
        |ui| {
            if card::switch(ui, &mut classic_on) {
                config.subpane_adaptive = !classic_on;
            }
            ui.add_space(4.0);
            if let Some(index) = kit::menu_picker(ui, 72.0, &pct_labels, selected_pct, classic_on) {
                config.subpane_classic_pct = CLASSIC_SCALE_PERCENTAGES[index];
                config.subpane_adaptive = false;
            }
        },
    );
}
