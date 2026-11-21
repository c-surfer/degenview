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

        set_lang(LANG_EN);
    }
}
