//! End-to-end integration tests for Surfer RSI indicator and licensing service.
//!
//! Verifies:
//! 1. "Cold start without license":
//!    - Indicator registered in `bundled::ALL` with id "surfer-rsi"
//!    - `is_pro_active()` is false
//!    - Free core contains RSI, Stoch, ATR but no PRO functions
//!    - Pine script compiles and executes on test bars without runtime errors
//! 2. "Valid license activation":
//!    - Simulated successful license server response with valid decryption key
//!    - `is_pro_active()` transitions to true
//!    - `effective_source()` decrypts PRO payload and merges it with Free Core
//!    - Combined script compiles and executes on bar stream with all PRO features
//! 3. "License revocation / removal":
//!    - Calling `store.remove()` immediately deactivates PRO mode
//!    - `is_pro_active()` resets to false
//!    - `effective_source()` returns pure Free Core again

use super::*;
use crate::model::Kline;
use crate::model::pine::bundled;
use crate::pine::compiler;
use crate::pine::compiler::program::SymbolInfo;
use crate::pine::syntax::token::{DiagnosticSeverity, Limits};
use crate::service::keychain::api_keys::MemoryBackend;
use crate::store::scripts::registry::LibraryRegistry;
use std::collections::HashMap;

fn generate_test_bars(count: usize) -> Vec<Kline> {
    (0..count)
        .map(|i| {
            let t = i as f64;
            let base = 100.0 + (t * 0.17).sin() * 9.0 + (t * 0.05).cos() * 6.0 + t * 0.04;
            Kline {
                open_time: 1_700_000_000_000 + i as i64 * 3_600_000,
                open: base - 0.4,
                high: base + 1.8 + (t * 0.3).sin().abs(),
                low: base - 1.8 - (t * 0.2).cos().abs(),
                close: base + 0.5,
                volume: 900.0 + (t * 0.4).sin().abs() * 700.0,
                close_time: 1_700_000_000_000 + (i as i64 + 1) * 3_600_000 - 1,
            }
        })
        .collect()
}

fn create_test_symbol() -> SymbolInfo {
    SymbolInfo {
        ticker: "BTCUSDT".into(),
        ticker_id: "BINANCE:BTCUSDT".into(),
        currency: "USD".into(),
        kind: "crypto".into(),
        base_currency: None,
        ..SymbolInfo::default()
    }
}

struct StateGuard;
impl Drop for StateGuard {
    fn drop(&mut self) {
        set_cached_license_state(None);
    }
}

#[test]
fn test_surfer_end_to_end_lifecycle() {
    let _lock = crate::lock::lock(&TEST_LICENSE_LOCK);
    let _guard = StateGuard;
    set_cached_license_state(None);

    let bars = generate_test_bars(350);
    let symbol = create_test_symbol();
    let empty_inputs = HashMap::new();

    // ─────────────────────────────────────────────────────────────────────────────
    // 1. Сценарий "Холодный старт без лицензии"
    // ─────────────────────────────────────────────────────────────────────────────
    let indicator = bundled::find("surfer-rsi")
        .expect("Индикатор 'surfer-rsi' должен быть зарегистрирован в каталоге bundled::ALL");

    assert_eq!(indicator.id, "surfer-rsi");
    assert_eq!(indicator.name, "[Surfer] RSI v1");
    assert!(!indicator.overlay, "Surfer RSI должен быть осциллятором (overlay = false)");
    assert!(
        bundled::ALL.iter().any(|ind| ind.id == "surfer-rsi"),
        "Индикатор обязан присутствовать в статическом каталоге bundled::ALL"
    );

    // Проверяем, что PRO режим выключен
    assert!(!is_pro_active(), "При холодном старте PRO режим должен быть неактивен");
    assert_eq!(get_decrypt_key(), None);
    assert_eq!(current_license_state(), None);

    let free_source = indicator.effective_source();
    // effective_source() в Free режиме возвращает ровно базовый исходник
    assert_eq!(free_source.as_ref(), indicator.source);

    // Проверяем состав Free Core
    assert!(
        free_source.contains("CalculateRSI"),
        "Free Core должен содержать расчет RSI"
    );
    assert!(
        free_source.contains("getStochastic"),
        "Free Core должен содержать расчет Stochastic"
    );
    assert!(
        free_source.contains("ATR STOP-LOSS"),
        "Free Core должен содержать функционал ATR Stop-Loss"
    );

    // Проверяем ОТСУТСТВИЕ PRO функций в Free Core
    assert!(
        !free_source.contains("supersmoother"),
        "Free Core НЕ должен содержать функцию supersmoother"
    );
    assert!(
        !free_source.contains("GetBullishRegularDivergence"),
        "Free Core НЕ должен содержать функцию GetBullishRegularDivergence"
    );
    assert!(
        !free_source.contains("get_cloud"),
        "Free Core НЕ должен содержать PRO функцию get_cloud"
    );
    assert!(
        !free_source.contains(crypto::PRO_PAYLOAD_MAGIC),
        "Free Core НЕ должен содержать сигнатуру PRO расширения"
    );

    // Компиляция Free Core
    let free_program = compiler::compile(&free_source, Limits::default(), &LibraryRegistry);
    let free_errors: Vec<_> = free_program
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect();
    assert!(
        free_errors.is_empty(),
        "Free Core должен компилироваться без ошибок: {free_errors:?}"
    );

    // Исполнение Free Core на потоке баров
    let (free_output, free_failure) = crate::host::pine::run_script_checked(
        free_source.as_ref(),
        &bars,
        &symbol,
        &empty_inputs,
    );
    assert!(
        free_failure.is_none(),
        "Free Core должен успешно исполняться без рантайм-сбоев: {free_failure:?}"
    );
    assert!(
        !free_output.plots.is_empty(),
        "Free Core должен отрисовывать графики (RSI, Stoch, ATR)"
    );

    // ─────────────────────────────────────────────────────────────────────────────
    // 2. Сценарий "Активация валидной лицензии"
    // ─────────────────────────────────────────────────────────────────────────────
    let server_response = serde_json::json!({
        "success": true,
        "decrypt_key": crypto::DEFAULT_PRO_KEY,
        "end_date": "2027-12-31",
        "nickname": "DegenMaster",
        "user_id": "888777",
        "error": null
    });

    let state = parse_license_response(&server_response)
        .expect("Парсинг валидного ответа лицензионного сервера должен завершиться успешно");
    assert!(state.success);
    assert_eq!(state.decrypt_key.as_deref(), Some(crypto::DEFAULT_PRO_KEY));

    // Применяем активированное состояние
    set_cached_license_state(Some(state));

    assert!(is_pro_active(), "После активации is_pro_active() должен возвращать true");
    assert_eq!(get_decrypt_key().as_deref(), Some(crypto::DEFAULT_PRO_KEY));

    let pro_source = indicator.effective_source();
    assert_ne!(
        pro_source.as_ref(),
        indicator.source,
        "В PRO режиме effective_source() должен отличаться от исходного Free Core"
    );

    // Проверяем наличие PRO компонентов и сигнатуры
    assert!(
        pro_source.contains(crypto::PRO_PAYLOAD_MAGIC),
        "PRO скрипт должен содержать сигнатуру PRO Extension"
    );
    assert!(
        pro_source.contains("supersmoother"),
        "PRO скрипт должен содержать расшифрованную функцию supersmoother"
    );
    assert!(
        pro_source.contains("GetBullishRegularDivergence"),
        "PRO скрипт должен содержать расшифрованную функцию GetBullishRegularDivergence"
    );
    assert!(
        pro_source.contains("get_cloud"),
        "PRO скрипт должен содержать расшифрованную функцию get_cloud"
    );
    assert!(
        pro_source.contains("f_rsi_htf"),
        "PRO скрипт должен содержать расшифрованную функцию f_rsi_htf"
    );

    // Исходный Free Core также должен быть на месте
    assert!(pro_source.contains("CalculateRSI"));
    assert!(pro_source.contains("getStochastic"));

    // Компиляция объединенного PRO скрипта
    let pro_program = compiler::compile(&pro_source, Limits::default(), &LibraryRegistry);
    let pro_errors: Vec<_> = pro_program
        .diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect();
    assert!(
        pro_errors.is_empty(),
        "Объединенный PRO Pine скрипт должен компилироваться без ошибок: {pro_errors:?}"
    );

    // Исполнение PRO скрипта на потоке баров
    let (pro_output, pro_failure) = crate::host::pine::run_script_checked(
        pro_source.as_ref(),
        &bars,
        &symbol,
        &empty_inputs,
    );
    assert!(
        pro_failure.is_none(),
        "Объединенный PRO Pine скрипт должен успешно исполняться на потоке баров: {pro_failure:?}"
    );
    assert!(
        !pro_output.plots.is_empty(),
        "PRO скрипт должен отрисовывать графики"
    );

    // ─────────────────────────────────────────────────────────────────────────────
    // 3. Сценарий "Отзыв / удаление лицензии"
    // ─────────────────────────────────────────────────────────────────────────────
    let memory_backend = MemoryBackend::default();
    let store = SurferKeyStore::with_backend(Box::new(memory_backend));

    // Сохраняем ключ в хранилище
    store.save("VALID_SURFER_KEY_XYZ").expect("Сохранение ключа должно пройти успешно");
    assert!(store.is_configured());
    assert_eq!(store.license().as_deref(), Some("VALID_SURFER_KEY_XYZ"));

    // Отзываем / удаляем лицензию через store.remove()
    store.remove().expect("Удаление лицензии должно пройти успешно");

    assert!(!store.is_configured(), "После remove() хранилище должно быть не настроено");
    assert_eq!(store.license(), None);
    assert!(!is_pro_active(), "После store.remove() PRO режим должен быть мгновенно сброшен в false");
    assert_eq!(get_decrypt_key(), None);
    assert_eq!(current_license_state(), None);

    let reverted_source = indicator.effective_source();
    assert_eq!(
        reverted_source.as_ref(),
        indicator.source,
        "После отзыва лицензии effective_source() обязан возвращать только Free Core"
    );
    assert!(
        !reverted_source.contains("supersmoother"),
        "Отозванный скрипт НЕ должен содержать supersmoother"
    );
    assert!(
        !reverted_source.contains("GetBullishRegularDivergence"),
        "Отозванный скрипт НЕ должен содержать GetBullishRegularDivergence"
    );
    assert!(
        !reverted_source.contains(crypto::PRO_PAYLOAD_MAGIC),
        "Отозванный скрипт НЕ должен содержать PRO сигнатуру"
    );

    // Исполнение возвращенного Free Core
    let (reverted_output, reverted_failure) = crate::host::pine::run_script_checked(
        reverted_source.as_ref(),
        &bars,
        &symbol,
        &empty_inputs,
    );
    assert!(reverted_failure.is_none());
    assert!(!reverted_output.plots.is_empty());
}
