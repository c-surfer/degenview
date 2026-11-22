//! Internationalization (i18n) module for DegenView.
//! Provides instantaneous zero-cost switching between English and Russian.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};
use serde::{Deserialize, Serialize};

pub const LANG_EN: u8 = 0;
pub const LANG_RU: u8 = 1;
pub const LANG_KEY: &str = "uiLanguage";

static CURRENT_LANG: AtomicU8 = AtomicU8::new(LANG_EN);

static TRANSLATIONS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
const RU_JSON: &str = include_str!("locales/ru.json");

fn translations() -> &'static HashMap<&'static str, &'static str> {
    TRANSLATIONS.get_or_init(|| {
        if let Ok(map) = serde_json::from_str::<HashMap<&'static str, &'static str>>(RU_JSON) {
            map
        } else if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(RU_JSON) {
            map.into_iter()
                .map(|(k, v)| {
                    let k_static: &'static str = Box::leak(k.into_boxed_str());
                    let v_static: &'static str = Box::leak(v.into_boxed_str());
                    (k_static, v_static)
                })
                .collect()
        } else {
            HashMap::new()
        }
    })
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UiLanguage {
    #[default]
    English,
    Russian,
}

#[allow(dead_code)]
impl UiLanguage {
    pub const ALL: [Self; 2] = [Self::English, Self::Russian];

    pub fn as_code(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Russian => "ru",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
        }
    }

    #[allow(dead_code)]
    pub fn subtitle(&self) -> &'static str {
        match self {
            Self::English => "Default interface language",
            Self::Russian => "Русский интерфейс",
        }
    }
}

pub fn set_lang(lang: u8) {
    CURRENT_LANG.store(lang, Ordering::Relaxed);
}

pub fn set_language(lang: UiLanguage) {
    match lang {
        UiLanguage::English => set_lang(LANG_EN),
        UiLanguage::Russian => set_lang(LANG_RU),
    }
}

pub fn language() -> UiLanguage {
    if is_ru() {
        UiLanguage::Russian
    } else {
        UiLanguage::English
    }
}

#[allow(dead_code)]
pub fn toggle_lang() -> UiLanguage {
    if is_ru() {
        set_lang(LANG_EN);
        UiLanguage::English
    } else {
        set_lang(LANG_RU);
        UiLanguage::Russian
    }
}

pub fn lang() -> u8 {
    CURRENT_LANG.load(Ordering::Relaxed)
}

pub fn is_ru() -> bool {
    lang() == LANG_RU
}

/// Translate a key from the dictionary.
/// Returns Russian translation if Russian language is active and key exists in `ru.json`;
/// otherwise returns `key`.
#[allow(dead_code)]
#[inline]
pub fn t<'a>(key: &'a str) -> &'a str {
    if !is_ru() {
        key
    } else {
        translations().get(key).copied().unwrap_or(key)
    }
}

/// Translate with explicit English and Russian versions.
/// If Russian is active and `ru.json` overrides `en`, the dictionary value is preferred,
/// falling back to `ru`.
#[inline]
pub fn tr<'a>(en: &'a str, ru: &'a str) -> &'a str {
    if is_ru() {
        if let Some(&translated) = translations().get(en) {
            translated
        } else {
            ru
        }
    } else {
        en
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i18n_translation() {
        set_lang(LANG_EN);
        assert_eq!(t("Save layout"), "Save layout");
        assert_eq!(tr("Save layout", "Сохранить"), "Save layout");

        set_lang(LANG_RU);
        assert_eq!(t("Save layout"), "Сохранить слой");
        assert_eq!(tr("Save layout", "Другой перевод"), "Сохранить слой");
        assert_eq!(tr("Nonexistent key xyz", "Фолбэк"), "Фолбэк");
        assert_eq!(t("Nonexistent key xyz"), "Nonexistent key xyz");

        // Screenshot 1
        assert_eq!(t("Choose where to trade"), "Выберите платформу для торговли");
        assert_eq!(
            t("Nothing is connected until you choose. You can add or remove places at any time in Settings."),
            "Ничего не подключено, пока вы не выберете. Вы можете добавить или удалить платформы в любое время в Настройках."
        );
        assert_eq!(t("Connect"), "Подключить");
        assert_eq!(t("Paper trading"), "Демо-торговля");
        assert_eq!(t("Simulated"), "Симуляция");
        assert_eq!(
            t("Practice with pretend money. Nothing leaves this computer."),
            "Тренируйтесь на виртуальных средствах. Никакие данные не покидают этот компьютер."
        );
        assert_eq!(t("Real money"), "Реальные средства");
        assert_eq!(t("Real orders on your Coinbase account."), "Реальные ордера на вашем счете Coinbase.");
        assert_eq!(t("Test"), "Тест");
        assert_eq!(
            t("Practice orders on Coinbase Sandbox's test environment. No real money."),
            "Тренировочные ордера в тестовой среде Coinbase Sandbox. Без реальных денег."
        );

        // Screenshot 2
        assert_eq!(t("Alerts"), "Оповещения");
        assert_eq!(t("No price alerts yet"), "Ценовых оповещений пока нет");
        assert_eq!(t("Foreground evaluation only"), "Проверка только при активном окне");
        assert_eq!(
            t("Alerts are only checked while DegenView is open."),
            "Оповещения проверяются, только пока DegenView открыт."
        );
        assert_eq!(
            t("Alerts keep being checked when DegenView is closed."),
            "Оповещения продолжают проверяться при закрытом DegenView."
        );
        assert_eq!(t("Search asset or name"), "Поиск актива или названия");
        assert_eq!(t("Active"), "Активные");
        assert_eq!(t("Triggered"), "Сработавшие");
        assert_eq!(t("Paused"), "На паузе");
        assert_eq!(t("All"), "Все");
        assert_eq!(t("History"), "История");
        assert_eq!(t("Scripts"), "Скрипты");
        assert_eq!(t("No Active Alerts"), "Нет активных оповещений");
        assert_eq!(
            t("Click the bell on a chart to alert on a price."),
            "Нажмите на колокольчик на графике, чтобы создать оповещение по цене."
        );
        assert_eq!(t("Nothing Triggered"), "Нет сработавших");
        assert_eq!(
            t("An alert that fires once waits here until you re-enable it."),
            "Оповещение, сработавшее один раз, ожидает здесь повторной активации."
        );
        assert_eq!(t("Nothing Paused"), "Нет оповещений на паузе");
        assert_eq!(
            t("Paused alerts wait here until you resume them."),
            "Оповещения на паузе отображаются здесь до их возобновления."
        );
        assert_eq!(t("No Alerts"), "Нет оповещений");
        assert_eq!(t("No Trigger History"), "История срабатываний пуста");
        assert_eq!(
            t("Every time an alert fires it is logged here."),
            "Каждое срабатывание оповещения записывается сюда."
        );
        assert_eq!(t("No Script Alerts"), "Нет скриптовых оповещений");
        assert_eq!(
            t("Alerts raised by your Pine scripts appear here."),
            "Оповещения от ваших Pine-скриптов будут появляться здесь."
        );

        // Screenshot 3 & 8
        assert_eq!(t("Volume"), "Объём");
        assert_eq!(t("Status"), "Статус");
        assert_eq!(t("Live"), "В реальном времени");
        assert_eq!(t("Symbol"), "Символ");
        assert_eq!(t("Updated"), "Обновлено");
        assert_eq!(t("IN LISTS"), "В СПИСКАХ");
        assert_eq!(t("Change over the last 24 hours"), "Изменение за последние 24 часа");

        // Screenshot 4
        assert_eq!(t("Replay"), "Симулятор рынка");
        assert_eq!(t("Choose a start"), "Выберите точку старта");
        assert_eq!(t("Pick a start"), "Выберите точку старта");
        assert_eq!(t("Select bar on chart"), "Выбрать свечу на графике");
        assert_eq!(t("Click a candle"), "Кликните по свече");
        assert_eq!(t("or"), "или");
        assert_eq!(t("Date & Time"), "Дата и время");
        assert_eq!(t("Random"), "Случайно");
        assert_eq!(t("First Bar"), "Первая свеча");

        // Screenshot 5
        assert_eq!(t("Create Price Alert"), "Создать оповещение");
        assert_eq!(t("Edit Price Alert"), "Редактировать оповещение");
        assert_eq!(t("Current price"), "Текущая цена");
        assert_eq!(t("Alert me when the price"), "Оповестить, когда цена");
        assert_eq!(t("Above"), "Выше");
        assert_eq!(t("Below"), "Ниже");
        assert_eq!(t("Rises"), "Растёт");
        assert_eq!(t("Falls"), "Падает");
        assert_eq!(
            t("Notify me when the price crosses above a level."),
            "Оповестить при пересечении уровня снизу вверх."
        );
        assert_eq!(
            t("Notify me when the price crosses below a level."),
            "Оповестить при пересечении уровня сверху вниз."
        );
        assert_eq!(
            t("Notify me when the price rises by a percentage."),
            "Оповестить при росте цены на процент."
        );
        assert_eq!(
            t("Notify me when the price falls by a percentage."),
            "Оповестить при падении цены на процент."
        );
        assert_eq!(t("Target price"), "Целевая цена");
        assert_eq!(t("How often"), "Как часто");
        assert_eq!(t("Once"), "Однократно");
        assert_eq!(t("Every time"), "Каждый раз");
        assert_eq!(t("Add a note (optional)"), "Добавить заметку (необязательно)");
        assert_eq!(t("Send to webhooks"), "Отправка на вебхук");
        assert_eq!(
            t("Also post this alert to your own server or bot."),
            "Также отправлять это оповещение на ваш сервер или боту."
        );
        assert_eq!(
            t("Set a level to create the alert."),
            "Укажите уровень для создания оповещения."
        );
        assert_eq!(t("Cancel"), "Отмена");
        assert_eq!(t("Create Alert"), "Создать оповещение");
        assert_eq!(t("Save Changes"), "Сохранить изменения");

        // Screenshot 6
        assert_eq!(t("Undo ({})"), "Отменить ({})");
        assert_eq!(t("Redo ({})"), "Повторить ({})");

        // Screenshot 7
        assert_eq!(t("Remove from watchlist"), "Удалить из избранного");
        assert_eq!(t("Add to watchlist"), "Добавить в избранное");

        // Screenshot 9
        assert_eq!(
            t("The chart leaves this tab. You can add it again from the symbol search."),
            "График будет удален из этой вкладки. Вы можете снова добавить его через поиск символов."
        );
        assert_eq!(t("Remove"), "Удалить");

        set_lang(LANG_EN);
    }
}
