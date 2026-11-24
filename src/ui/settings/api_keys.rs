//! Settings ▸ Alpaca and Settings ▸ CoinMarketCap: the API keys that unlock US stock data and
//! raise the market-index rate limits. Pages over the shared key stores; saving, removing and the
//! CoinMarketCap connection test happen here, since the stores are process-wide, but every rule
//! (what may be saved, what the status says) is a plain function tested against in-memory stores.

use crate::model::DataSource;
use crate::service::ProviderError;
use crate::service::keychain::api_keys::{
    AlpacaCredentials, AlpacaCredentialsStore, CoinMarketCapKeyStore,
};
use crate::service::surfer::{self, SurferKeyStore, SurferLicenseState};
use crate::service::work_queue::{self, Pending};
use crate::ui::icons::{self, Icon};
use crate::ui::kit::loading::LoadingIndicator;
use crate::ui::kit::{Autosave, Tone, destructive_button, field, open_button};
use crate::ui::settings::card::StatusMessage;
use crate::ui::settings::window::{card, section, status_message};
use crate::ui::{kit, theme, tr};
use eframe::egui;

#[derive(Default)]
pub struct AlpacaForm {
    pub key_id: String,
    pub secret_key: String,
    /// Whether the saved credentials have been read into the fields yet (reading may prompt).
    loaded: bool,
    pub is_saved: bool,
    pub status: Option<StatusMessage>,
    test: Option<Pending<Result<(), ProviderError>>>,
    autosave: Autosave,
}

#[derive(Default)]
pub struct CmcForm {
    pub api_key: String,
    loaded: bool,
    pub configured: bool,
    pub status: Option<StatusMessage>,
    test: Option<Pending<Result<(), ProviderError>>>,
    autosave: Autosave,
}

#[derive(Default)]
pub struct SurferForm {
    pub license: String,
    loaded: bool,
    pub configured: bool,
    pub status: Option<StatusMessage>,
    test: Option<Pending<Result<SurferLicenseState, String>>>,
    autosave: Autosave,
}

#[derive(Default)]
pub struct ApiKeysState {
    pub alpaca: AlpacaForm,
    pub cmc: CmcForm,
    pub surfer: SurferForm,
}

// MARK: Alpaca rules

impl AlpacaForm {
    /// Fills the fields from the saved credentials, once.
    pub fn load(&mut self, store: &AlpacaCredentialsStore) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.is_saved = store.is_configured();
        if self.is_saved {
            let saved = store.credentials();
            self.key_id = saved.key_id;
            self.secret_key = saved.secret_key;
        }
    }

    pub fn can_save(&self) -> bool {
        !self.key_id.trim().is_empty() && !self.secret_key.trim().is_empty()
    }

    pub fn badge(&self) -> (&'static str, Tone) {
        if self.is_saved {
            (tr("Keys saved", "Ключи сохранены"), Tone::Good)
        } else {
            (tr("Not configured", "Не настроено"), Tone::Neutral)
        }
    }

    pub fn save(&mut self, store: &AlpacaCredentialsStore) {
        let credentials = AlpacaCredentials {
            key_id: self.key_id.clone(),
            secret_key: self.secret_key.clone(),
        };
        self.status = Some(match store.save(&credentials) {
            Ok(()) => {
                self.is_saved = true;
                StatusMessage::ok(crate::platform::saved_securely())
            }
            Err(error) => StatusMessage::error(format!("Could not save: {}", error.0)),
        });
    }

    pub fn is_testing(&self) -> bool {
        self.test.is_some()
    }

    /// Asks Alpaca for a bar with the typed keys, on Alpaca's workers; `poll_test` picks up the
    /// answer.
    pub fn start_test(
        &mut self,
        repaint: Option<&egui::Context>,
        test: impl FnOnce(String, String) -> Result<(), ProviderError> + Send + 'static,
    ) {
        if self.test.is_some() {
            return;
        }
        self.status = None;
        let (key_id, secret_key) = (self.key_id.clone(), self.secret_key.clone());
        self.test = Some(work_queue::start_for(
            DataSource::Alpaca,
            repaint,
            move || test(key_id, secret_key),
        ));
    }

    /// Takes the connection test's answer when it has arrived.
    pub fn poll_test(&mut self) {
        take_test_answer(&mut self.test, &mut self.status);
    }

    pub fn remove(&mut self, store: &AlpacaCredentialsStore) {
        self.status = Some(match store.save(&AlpacaCredentials::default()) {
            Ok(()) => {
                self.key_id.clear();
                self.secret_key.clear();
                self.is_saved = false;
                StatusMessage::ok("Keys removed. Stock charts need keys to load.")
            }
            Err(error) => StatusMessage::error(format!("Could not remove the keys: {}", error.0)),
        });
    }
}

// MARK: CoinMarketCap rules

impl CmcForm {
    pub fn load(&mut self, store: &CoinMarketCapKeyStore) {
        if !self.loaded {
            self.loaded = true;
            self.configured = store.is_configured();
        }
    }

    pub fn can_save(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub fn is_testing(&self) -> bool {
        self.test.is_some()
    }

    pub fn badge(&self) -> (&'static str, Tone) {
        if self.configured {
            (tr("API key configured", "API-ключ настроен"), Tone::Good)
        } else {
            (tr("Public API mode", "Публичный режим API"), Tone::Neutral)
        }
    }

    pub fn field_title(&self) -> &'static str {
        if self.configured {
            tr("Replace API Key", "Заменить API-ключ")
        } else {
            tr("API Key", "API-ключ")
        }
    }

    pub fn field_hint(&self) -> &'static str {
        if self.configured {
            crate::platform::credential_saved_hint()
        } else {
            crate::platform::credential_stored_hint()
        }
    }

    pub fn save(&mut self, store: &CoinMarketCapKeyStore) {
        self.status = Some(match store.save(&self.api_key) {
            Ok(()) => {
                self.configured = true;
                self.api_key.clear();
                StatusMessage::ok(crate::platform::saved_securely())
            }
            Err(error) => StatusMessage::error(format!("Could not save: {}", error.0)),
        });
    }

    pub fn remove(&mut self, store: &CoinMarketCapKeyStore) {
        self.status = Some(match store.remove() {
            Ok(()) => {
                self.api_key.clear();
                self.configured = false;
                StatusMessage::ok("Key removed. Public API mode is active.")
            }
            Err(error) => StatusMessage::error(format!("Could not remove the key: {}", error.0)),
        });
    }

    /// Runs `test` on a worker that may wait on the user (it reads the saved key, which may
    /// prompt); `poll_test` picks up the answer.
    pub fn start_test(
        &mut self,
        repaint: Option<&egui::Context>,
        test: impl FnOnce() -> Result<(), ProviderError> + Send + 'static,
    ) {
        if self.test.is_some() {
            return;
        }
        self.status = None;
        self.test = Some(work_queue::SNAPSHOTS.start(repaint, test));
    }

    /// Takes the connection test's answer when it has arrived.
    pub fn poll_test(&mut self) {
        take_test_answer(&mut self.test, &mut self.status);
    }
}

// MARK: Surfer rules

impl SurferForm {
    pub fn load(&mut self, store: &SurferKeyStore) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.configured = store.is_configured();
        if self.configured {
            if let Some(license) = store.license() {
                self.license = license;
            }
        }
    }

    pub fn can_save(&self) -> bool {
        !self.license.trim().is_empty()
    }

    pub fn is_testing(&self) -> bool {
        self.test.is_some()
    }

    pub fn badge(&self) -> (&'static str, Tone) {
        if self.configured {
            (tr("Active", "Активна"), Tone::Good)
        } else {
            (tr("Inactive", "Не активна"), Tone::Neutral)
        }
    }

    pub fn save(&mut self, store: &SurferKeyStore) {
        let trimmed = self.license.trim();
        self.status = Some(match store.save(trimmed) {
            Ok(()) => {
                self.configured = !trimmed.is_empty();
                StatusMessage::ok(crate::platform::saved_securely())
            }
            Err(error) => StatusMessage::error(format!("Could not save: {}", error.0)),
        });
    }

    pub fn remove(&mut self, store: &SurferKeyStore) {
        self.status = Some(match store.remove() {
            Ok(()) => {
                self.license.clear();
                self.configured = false;
                StatusMessage::ok(tr(
                    "License removed. Basic RSI mode is active.",
                    "Лицензия удалена. Включен базовый режим RSI.",
                ))
            }
            Err(error) => StatusMessage::error(format!("Could not remove the license: {}", error.0)),
        });
    }

    pub fn start_test(&mut self, ctx: Option<&egui::Context>) {
        self.start_test_with(ctx, |lic| surfer::test_connection(&lic));
    }

    pub fn start_test_with(
        &mut self,
        ctx: Option<&egui::Context>,
        test: impl FnOnce(String) -> Result<SurferLicenseState, String> + Send + 'static,
    ) {
        if self.test.is_some() {
            return;
        }
        self.status = None;
        let license = self.license.trim().to_string();
        self.test = Some(work_queue::start(ctx, move || test(license)));
    }

    pub fn poll_test(&mut self) {
        let Some(pending) = &mut self.test else {
            return;
        };
        let Some(outcome) = pending.poll_or(|| {
            Err("The connection test stopped unexpectedly.".to_string())
        }) else {
            return;
        };
        self.test = None;
        self.status = Some(match outcome {
            Ok(state) => {
                if state.success && state.error.is_none() {
                    let end_date = state.end_date.as_deref().unwrap_or("—");
                    let nickname = state.nickname.as_deref().unwrap_or("—");
                    let msg = if crate::ui::is_ru() {
                        format!("Лицензия активна до {end_date} (Пользователь: {nickname})")
                    } else {
                        format!("License active until {end_date} (User: {nickname})")
                    };
                    StatusMessage::ok(msg)
                } else {
                    let err = state
                        .error
                        .unwrap_or_else(|| tr("License verification failed", "Ошибка проверки лицензии").to_string());
                    StatusMessage::error(err)
                }
            }
            Err(error) => StatusMessage::error(error),
        });
    }
}

/// Turns a finished connection test into the status under its button.
fn take_test_answer(
    test: &mut Option<Pending<Result<(), ProviderError>>>,
    status: &mut Option<StatusMessage>,
) {
    let Some(pending) = test else {
        return;
    };
    let Some(outcome) = pending.poll_or(|| {
        Err(ProviderError::other(
            "The connection test stopped unexpectedly.",
        ))
    }) else {
        return;
    };
    *test = None;
    *status = Some(match outcome {
        Ok(()) => StatusMessage::ok("Connection successful."),
        Err(error) => StatusMessage::error(error.to_string()),
    });
}

// MARK: Views

/// A numbered instruction line.
fn step(ui: &mut egui::Ui, number: usize, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (badge, _) = ui.allocate_exact_size(egui::Vec2::splat(20.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(badge.center(), 10.0, theme::ink(14));
        let digit = kit::galley(
            ui,
            &number.to_string(),
            kit::size::CAPTION + 0.5,
            kit::BOLD,
            theme::text_secondary(),
            20.0,
        );
        ui.painter().galley(
            badge.center() - digit.size() / 2.0,
            digit,
            theme::text_secondary(),
        );
        ui.label(kit::text(text, 12.0, kit::REGULAR, theme::text_secondary()));
    });
}

/// A link with a leading "open" glyph: gray and underlined, as every link is.
pub(crate) fn link(ui: &mut egui::Ui, label: &str, url: &str) {
    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            icons::inline(ui, Icon::ArrowUpRightSquare, 12.0, theme::text_secondary());
            ui.add(
                egui::Label::new(
                    kit::text(
                        label,
                        kit::size::BODY,
                        kit::REGULAR,
                        theme::text_secondary(),
                    )
                    .underline(),
                )
                .sense(egui::Sense::click()),
            )
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(url);
    if response.clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
}

pub fn alpaca_body(ui: &mut egui::Ui, state: &mut ApiKeysState, store: &AlpacaCredentialsStore) {
    let form = &mut state.alpaca;
    form.load(store);
    form.poll_test();
    if form.is_testing() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
    let (text, tone) = form.badge();
    kit::status_badge(ui, text, tone);
    ui.add_space(18.0);
    section(ui, tr("API credentials", "Ключи API"), None, |ui| {
        card(ui, |ui| {
            ui.vertical(|ui| {
                let key_id = field(ui, tr("API Key ID", "Идентификатор ключа (Key ID)"), None, "PK…", &mut form.key_id, false);
                form.autosave.watch(&key_id, false);
                ui.add_space(12.0);
                let secret = field(
                    ui,
                    tr("Secret Key", "Секретный ключ (Secret Key)"),
                    Some(crate::platform::credential_stored_hint()),
                    "",
                    &mut form.secret_key,
                    true,
                );
                form.autosave.watch(&secret, true);
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if form.is_testing() {
                    ui.add(LoadingIndicator::new().size(14.0));
                } else if kit::sheet_button(ui, tr("Test Connection", "Проверить подключение"), false, form.can_save()).clicked()
                {
                    form.start_test(Some(ui.ctx()), |key_id, secret| {
                        crate::service::alpaca_test_connection(&key_id, &secret)
                    });
                }
                if form.is_saved && destructive_button(ui, tr("Remove Keys", "Удалить ключи")).clicked() {
                    form.remove(store);
                    form.autosave.cancel();
                }
            });
        });
        if let Some(status) = &form.status {
            ui.add_space(8.0);
            status_message(ui, &status.text, status.is_error);
        }
    });
    // Both keys are needed, so nothing is saved until both are there.
    if form.autosave.due(ui.ctx()) && form.can_save() {
        form.save(store);
    }
    section(ui, tr("Getting your keys", "Как получить ключи"), None, |ui| {
        card(ui, |ui| {
            step(ui, 1, tr("Create a free Alpaca account.", "Создайте бесплатный аккаунт Alpaca."));
            step(
                ui,
                2,
                tr("Open the API Keys section of the dashboard and generate a key.", "Откройте раздел API Keys в панели управления и создайте ключ."),
            );
            step(
                ui,
                3,
                tr("Copy both values here. Alpaca only shows the secret once.", "Скопируйте оба значения сюда. Alpaca показывает секретный ключ только один раз."),
            );
            link(
                ui,
                tr("Open the Alpaca dashboard", "Перейти в панель управления Alpaca"),
                "https://app.alpaca.markets/",
            );
        });
    });
}

pub fn coinmarketcap_body(
    ui: &mut egui::Ui,
    state: &mut ApiKeysState,
    store: &CoinMarketCapKeyStore,
) {
    let form = &mut state.cmc;
    form.load(store);
    form.poll_test();
    if form.is_testing() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
    let (text, tone) = form.badge();
    kit::status_badge(ui, text, tone);
    ui.add_space(18.0);
    let (title, hint) = (form.field_title(), form.field_hint());
    section(ui, tr("API access", "Доступ к API"), None, |ui| {
        card(ui, |ui| {
            ui.vertical(|ui| {
                let key = field(ui, title, Some(hint), "", &mut form.api_key, true);
                form.autosave.watch(&key, true);
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if form.is_testing() {
                    ui.add(LoadingIndicator::new().size(14.0));
                } else if kit::sheet_button(ui, tr("Test Connection", "Проверить подключение"), false, true).clicked() {
                    form.start_test(Some(ui.ctx()), crate::service::market::cmc::test_connection);
                }
                open_button(ui, tr("Get API Key", "Получить API-ключ"), "https://pro.coinmarketcap.com/signup/");
                if form.configured && destructive_button(ui, tr("Remove Key", "Удалить ключ")).clicked() {
                    form.remove(store);
                    form.autosave.cancel();
                }
            });
        });
        if let Some(status) = &form.status {
            ui.add_space(8.0);
            status_message(ui, &status.text, status.is_error);
        }
    });
    if form.autosave.due(ui.ctx()) && form.can_save() {
        form.save(store);
    }
    kit::notice_card(
        ui,
        Icon::InfoCircle,
        theme::text_secondary(),
        tr("A key is optional", "Ключ не обязателен"),
        Some(tr(
            "CoinMarketCap works without an API key. Adding one gives DegenView higher API rate limits.",
            "CoinMarketCap работает и без API-ключа. Добавление ключа увеличивает лимиты запросов к API.",
        )),
        None,
    );
}

pub fn surfer_body(
    ui: &mut egui::Ui,
    state: &mut ApiKeysState,
    store: &SurferKeyStore,
) {
    let form = &mut state.surfer;
    form.load(store);
    form.poll_test();
    if form.is_testing() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(150));
    }
    let (text, tone) = form.badge();
    kit::status_badge(ui, text, tone);
    ui.add_space(18.0);
    section(ui, tr("Surfer License", "Лицензия Surfer"), None, |ui| {
        card(ui, |ui| {
            ui.vertical(|ui| {
                let hint = if form.configured {
                    crate::platform::credential_saved_hint()
                } else {
                    crate::platform::credential_stored_hint()
                };
                let key = field(
                    ui,
                    tr("License Key", "Лицензионный ключ"),
                    Some(hint),
                    "CS-...",
                    &mut form.license,
                    false,
                );
                form.autosave.watch(&key, true);
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if form.is_testing() {
                    ui.add(LoadingIndicator::new().size(14.0));
                } else if kit::sheet_button(
                    ui,
                    tr("Test Connection", "Проверить подключение"),
                    false,
                    form.can_save(),
                )
                .clicked()
                {
                    form.start_test(Some(ui.ctx()));
                }
                if form.configured
                    && destructive_button(ui, tr("Remove License", "Удалить лицензию")).clicked()
                {
                    form.remove(store);
                    form.autosave.cancel();
                }
            });
        });
        if let Some(status) = &form.status {
            ui.add_space(8.0);
            status_message(ui, &status.text, status.is_error);
        }
    });
    if form.autosave.due(ui.ctx()) && form.can_save() {
        form.save(store);
    }
    kit::notice_card(
        ui,
        Icon::InfoCircle,
        theme::text_secondary(),
        tr("About Surfer PRO", "О Surfer PRO"),
        Some(tr(
            "Without a license, the [Surfer] RSI indicator operates in basic mode (RSI + Stochastic + ATR). A valid license unlocks Clouds (MRC), MTF Background, Extra Signals, and Divergence Validation.",
            "Без лицензии индикатор [Surfer] RSI работает в базовом режиме (RSI + Stochastic + ATR). Валидная лицензия открывает Облака (MRC), MTF Background, Экстра-сигналы и валидацию дивергенций.",
        )),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::keychain::api_keys::MemoryBackend;
    use std::time::{Duration, Instant};

    fn alpaca_store() -> AlpacaCredentialsStore {
        AlpacaCredentialsStore::with_backend(Box::new(MemoryBackend::default()))
    }

    fn cmc_store() -> CoinMarketCapKeyStore {
        CoinMarketCapKeyStore::with_backend(Box::new(MemoryBackend::default()))
    }

    fn surfer_store() -> SurferKeyStore {
        SurferKeyStore::with_backend(Box::new(MemoryBackend::default()))
    }

    #[test]
    fn alpaca_saves_only_with_both_fields_and_reports_it() {
        let store = alpaca_store();
        let mut form = AlpacaForm::default();
        form.load(&store);
        assert_eq!(form.badge(), ("Not configured", Tone::Neutral));
        assert!(!form.can_save());
        form.key_id = " PK1 ".into();
        assert!(!form.can_save(), "the secret is missing too");
        form.secret_key = "S1".into();
        assert!(form.can_save());
        form.save(&store);
        assert_eq!(
            form.status,
            Some(StatusMessage::ok(crate::platform::saved_securely()))
        );
        assert_eq!(form.badge(), ("Keys saved", Tone::Good));
        assert_eq!(store.credentials().key_id, "PK1", "saved trimmed");
    }

    #[test]
    fn alpaca_loads_saved_keys_into_the_fields_once_and_can_remove_them() {
        let store = alpaca_store();
        store
            .save(&AlpacaCredentials {
                key_id: "PK9".into(),
                secret_key: "S9".into(),
            })
            .unwrap();
        let mut form = AlpacaForm::default();
        form.load(&store);
        assert!(form.is_saved);
        assert_eq!(
            (form.key_id.as_str(), form.secret_key.as_str()),
            ("PK9", "S9")
        );
        form.key_id = "typed".into();
        form.load(&store);
        assert_eq!(
            form.key_id, "typed",
            "a second load does not overwrite edits"
        );
        form.remove(&store);
        assert!(!form.is_saved && form.key_id.is_empty() && form.secret_key.is_empty());
        assert_eq!(
            form.status,
            Some(StatusMessage::ok(
                "Keys removed. Stock charts need keys to load."
            ))
        );
        assert!(!store.is_configured());
    }

    #[test]
    fn coinmarketcap_saves_clears_the_field_and_flips_the_badge() {
        let store = cmc_store();
        let mut form = CmcForm::default();
        form.load(&store);
        assert_eq!(form.badge(), ("Public API mode", Tone::Neutral));
        assert_eq!(form.field_title(), "API Key");
        assert!(!form.can_save());
        form.api_key = "  abc ".into();
        assert!(form.can_save());
        form.save(&store);
        assert!(
            form.api_key.is_empty(),
            "the secret does not linger in the field"
        );
        assert_eq!(form.badge(), ("API key configured", Tone::Good));
        assert_eq!(form.field_title(), "Replace API Key");
        assert_eq!(form.field_hint(), crate::platform::credential_saved_hint());
        assert_eq!(store.api_key().as_deref(), Some("abc"));
        form.remove(&store);
        assert_eq!(
            form.status,
            Some(StatusMessage::ok("Key removed. Public API mode is active."))
        );
        assert!(!form.configured && store.api_key().is_none());
    }

    fn wait_for_test(form: &mut CmcForm) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while form.is_testing() && Instant::now() < deadline {
            form.poll_test();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn the_connection_test_runs_in_the_background_and_reports_either_way() {
        let mut form = CmcForm {
            status: Some(StatusMessage::error("old")),
            ..CmcForm::default()
        };
        form.start_test(None, || Ok(()));
        assert!(form.is_testing());
        assert_eq!(form.status, None, "a new test clears the old message");
        form.start_test(None, || {
            Err(ProviderError::other("ignored: one test at a time"))
        });
        wait_for_test(&mut form);
        assert!(!form.is_testing());
        assert_eq!(
            form.status,
            Some(StatusMessage::ok("Connection successful."))
        );

        form.start_test(None, || Err(ProviderError::other("Invalid API key")));
        wait_for_test(&mut form);
        assert_eq!(form.status, Some(StatusMessage::error("Invalid API key")));
    }

    fn wait_for_surfer_test(form: &mut SurferForm) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while form.is_testing() && Instant::now() < deadline {
            form.poll_test();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn surfer_saves_loads_and_removes_license() {
        let store = surfer_store();
        let mut form = SurferForm::default();
        form.load(&store);
        assert_eq!(form.badge(), ("Inactive", Tone::Neutral));
        assert!(!form.can_save());
        assert!(!form.configured);

        form.license = "  CS-TEST-KEY-1234  ".into();
        assert!(form.can_save());
        form.save(&store);
        assert_eq!(
            form.status,
            Some(StatusMessage::ok(crate::platform::saved_securely()))
        );
        assert!(form.configured);
        assert_eq!(form.badge(), ("Active", Tone::Good));
        assert_eq!(store.license().as_deref(), Some("CS-TEST-KEY-1234"));

        // Second load does not overwrite edits
        let mut form2 = SurferForm::default();
        form2.load(&store);
        assert!(form2.configured);
        assert_eq!(form2.license, "CS-TEST-KEY-1234");
        form2.license = "modified".into();
        form2.load(&store);
        assert_eq!(form2.license, "modified");

        // Remove
        form2.remove(&store);
        assert!(!form2.configured);
        assert!(form2.license.is_empty());
        assert_eq!(store.license(), None);
        assert_eq!(form2.badge(), ("Inactive", Tone::Neutral));
    }

    #[test]
    fn surfer_connection_test_reports_success_and_error() {
        let mut form = SurferForm {
            license: "CS-TEST-KEY".into(),
            status: Some(StatusMessage::error("old")),
            ..SurferForm::default()
        };

        // Successful test
        form.start_test_with(None, |_| {
            Ok(SurferLicenseState {
                success: true,
                decrypt_key: Some("DEC_KEY".into()),
                end_date: Some("2027-01-01".into()),
                nickname: Some("AlphaTrader".into()),
                user_id: Some("42".into()),
                error: None,
            })
        });
        assert!(form.is_testing());
        assert_eq!(form.status, None, "new test clears old status");
        wait_for_surfer_test(&mut form);
        assert!(!form.is_testing());
        let expected_msg = if crate::ui::is_ru() {
            "Лицензия активна до 2027-01-01 (Пользователь: AlphaTrader)"
        } else {
            "License active until 2027-01-01 (User: AlphaTrader)"
        };
        assert_eq!(
            form.status,
            Some(StatusMessage::ok(expected_msg))
        );

        // Failed test (error from server payload)
        form.start_test_with(None, |_| {
            Ok(SurferLicenseState {
                success: false,
                decrypt_key: None,
                end_date: None,
                nickname: None,
                user_id: None,
                error: Some("Key expired".into()),
            })
        });
        wait_for_surfer_test(&mut form);
        assert!(!form.is_testing());
        assert_eq!(
            form.status,
            Some(StatusMessage::error("Key expired"))
        );

        // Network / transport error
        form.start_test_with(None, |_| Err("HTTP timeout".into()));
        wait_for_surfer_test(&mut form);
        assert!(!form.is_testing());
        assert_eq!(
            form.status,
            Some(StatusMessage::error("HTTP timeout"))
        );
    }
}
