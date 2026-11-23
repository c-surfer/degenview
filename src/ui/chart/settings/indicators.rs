//! The Indicators page: the built-in indicators and their settings.

use super::ChartSettingsContext;
use crate::model::{
    BOLLINGER_MULTIPLIER, BOLLINGER_PERIOD, EMA_PERIODS, RSI_OVERBOUGHT, RSI_OVERSOLD, RSI_PERIOD,
    SUPERTREND_MULTIPLIER, SUPERTREND_PERIOD,
};
use crate::ui::icons::Icon;
use crate::ui::kit;
use crate::ui::settings::card;
use crate::ui::tr;
use eframe::egui::Ui;

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
            format!(
                "Only {loaded} candles loaded — RSI needs more than {RSI_PERIOD}. Zoom out or pick a longer range."
            )
        }
    } else {
        if crate::ui::i18n::is_ru() {
            format!(
                "Импульс от 0 до 100 внизу графика, с уровнями {} и {}.",
                RSI_OVERSOLD as i64, RSI_OVERBOUGHT as i64
            )
        } else {
            format!(
                "Momentum from 0–100 across the bottom, with guides at {} and {}.",
                RSI_OVERSOLD as i64, RSI_OVERBOUGHT as i64
            )
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
        format!(
            "{BOLLINGER_PERIOD}-периодная средняя с полосами на {} стандартных отклонениях — шире при волатильности, уже при штиле.",
            BOLLINGER_MULTIPLIER as i64
        )
    } else {
        format!(
            "{BOLLINGER_PERIOD}-period average with bands {} standard deviations either side — wide when volatile, tight when calm.",
            BOLLINGER_MULTIPLIER as i64
        )
    };
    card::card(
        ui,
        Icon::Bands,
        tr("Bollinger Bands", "Полосы Боллинджера (Bollinger Bands)"),
        Some(&bb_hint),
        |ui| {
            card::switch(ui, &mut config.show_bollinger);
        },
    );
    let trend_hint = if crate::ui::i18n::is_ru() {
        format!(
            "Бычьи и медвежьи сигналы ATR({SUPERTREND_PERIOD}) × {}. Сигналы формируются после закрытия свечи; возможны ложные срабатывания в боковике.",
            SUPERTREND_MULTIPLIER as i64
        )
    } else {
        format!(
            "Bullish and bearish markers from {SUPERTREND_PERIOD}-period ATR × {}. Signals appear after the candle closes and can whipsaw in sideways markets.",
            SUPERTREND_MULTIPLIER as i64
        )
    };
    card::card(
        ui,
        Icon::Cycle,
        tr("Trend Flips", "Развороты тренда (Supertrend)"),
        Some(&trend_hint),
        |ui| {
            card::switch(ui, &mut config.show_trend_flips);
        },
    );
}
