use super::hero::{composition_shares, win_shares};
use super::*;

#[test]
fn activity_and_tracking_captions_read_naturally() {
    assert_eq!(activity_caption(1, 1), "1 buy · 1 sell");
    assert_eq!(activity_caption(3, 0), "3 buys · 0 sells");
    let day = 86_400_000;
    assert_eq!(tracking_caption(0, 0), "today");
    assert_eq!(tracking_caption(0, day), "1 day ago");
    assert_eq!(tracking_caption(0, 30 * day), "30 days ago");
    assert_eq!(tracking_caption(0, 90 * day), "3 months ago");
    assert_eq!(tracking_caption(0, 800 * day), "2 years ago");
    assert_eq!(
        tracking_caption(10 * day, 0),
        "today",
        "a future first transaction is not negative"
    );
}

#[test]
fn tracking_caption_ru() {
    crate::ui::i18n::set_lang(crate::ui::i18n::LANG_RU);
    let day = 86_400_000;
    assert_eq!(tracking_caption(0, 0), "сегодня");
    assert_eq!(tracking_caption(0, day), "1 день назад");
    assert_eq!(tracking_caption(0, 2 * day), "2 дня назад");
    assert_eq!(tracking_caption(0, 5 * day), "5 дней назад");
    assert_eq!(tracking_caption(0, 30 * day), "30 дней назад");
    assert_eq!(activity_caption(1, 1), "1 покупка · 1 продажа");
    assert_eq!(activity_caption(2, 4), "2 покупки · 4 продажи");
    assert_eq!(activity_caption(5, 10), "5 покупок · 10 продаж");
    crate::ui::i18n::set_lang(crate::ui::i18n::LANG_EN);
}

#[test]
fn the_breakdown_splits_by_magnitude_and_is_empty_without_profit() {
    assert_eq!(composition_shares(300.0, 100.0), Some([0.75, 0.25]));
    assert_eq!(composition_shares(-100.0, 100.0), Some([0.5, 0.5]));
    assert_eq!(composition_shares(0.0, 0.0), None);
    assert_eq!(composition_shares(f64::NAN, 1.0), None);
}

#[test]
fn win_shares_sum_to_one_and_count_the_level_ones() {
    assert_eq!(win_shares(0, 0, 0), [0.0; 3]);
    assert_eq!(win_shares(3, 3, 6), [0.5, 0.5, 0.0]);
    assert_eq!(win_shares(1, 1, 4), [0.25, 0.25, 0.5]);
    let shares = win_shares(5, 2, 9);
    assert!((shares.iter().sum::<f32>() - 1.0).abs() < 1e-6);
}
