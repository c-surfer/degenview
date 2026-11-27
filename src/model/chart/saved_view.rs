//! A named, saved chart layout — the "Saved View" feature. One row per layout under the
//! `saved_view` document table, restored into a workspace by the layout controller.
//!
//! Mirrors the Swift `SavedView`, with `Decimal`→`f64`/`UUID`→`u64`/`Date`→epoch-ms. Swift's
//! `tickers` + `tickerConfigs` collapse into one `charts` list (each chart carries its symbol).

use crate::model::{ChartColumn, PersistedChart, Timeframe};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SavedView {
    pub id: u64,
    pub name: String,
    pub timeframe: Timeframe,
    pub created_at_ms: i64,
    /// Per-chart configs, in grid order.
    pub charts: Vec<PersistedChart>,
    /// Column membership and order. `None` means the default two-column layout.
    #[serde(default)]
    pub columns: Option<Vec<ChartColumn>>,
    /// The scroll-zoom candle count at save time.
    #[serde(default)]
    pub candle_count: i64,
    /// When this layout was last opened or saved. `None` keeps it out of "Recently used".
    #[serde(default)]
    pub last_opened_at_ms: Option<i64>,
    /// Whether edits write back without asking. `None` = off.
    #[serde(default)]
    pub autosave: Option<bool>,
    /// Which parts of horizontal navigation the charts share. Older rows have none and open linked, the default.
    #[serde(default)]
    pub sync: crate::model::ChartSyncSettings,
    /// Normalized column width weights (sum = 1.0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_weights: Option<Vec<f32>>,
    /// Normalized row height weights per column id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_row_weights"
    )]
    pub row_weights: Option<std::collections::HashMap<u64, Vec<f32>>>,
}

/// Deserializes `Option<HashMap<u64, Vec<f32>>>` via string keys so buffered/untagged JSON
/// deserialization handles integer-keyed maps consistently with `PersistedWorkspace`.
fn deserialize_row_weights<'de, D>(
    deserializer: D,
) -> Result<Option<std::collections::HashMap<u64, Vec<f32>>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(raw) =
        Option::<std::collections::HashMap<String, Vec<f32>>>::deserialize(deserializer)?
    else {
        return Ok(None);
    };
    let mut weights = std::collections::HashMap::with_capacity(raw.len());
    for (key, value) in raw {
        let column_id = key.parse::<u64>().map_err(serde::de::Error::custom)?;
        weights.insert(column_id, value);
    }
    Ok(Some(weights))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DataSource, IndicatorConfig};

    fn view() -> SavedView {
        SavedView {
            id: 1,
            name: "BTC watch".into(),
            timeframe: Timeframe::HOUR_1,
            created_at_ms: 1_000,
            charts: vec![PersistedChart {
                id: 1,
                symbol: "BTCUSDT".into(),
                source: DataSource::Binance,
                timeframe: None,
                log_scale: false,
                scale_mode: None,
                y_zoom: 1.0,
                indicators: IndicatorConfig::default(),
                appearance: crate::model::ChartAppearance::default(),
                drawings: Default::default(),
                pine_instances: Vec::new(),
                candle_count: 200,
                viewport: Default::default(),
                title: None,
                pm_series: Vec::new(),
                image_url: None,
                card: None,
                overlays: Default::default(),
            }],
            columns: Some(vec![ChartColumn::new(1, vec![1])]),
            candle_count: 200,
            last_opened_at_ms: Some(2_000),
            autosave: Some(true),
            sync: crate::model::ChartSyncSettings {
                horizontal_position: true,
                horizontal_zoom: false,
            },
            column_weights: None,
            row_weights: None,
        }
    }

    #[test]
    fn round_trips() {
        let encoded = serde_json::to_string(&view()).expect("serialize");
        let decoded: SavedView = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, view());
    }

    #[test]
    fn old_rows_without_optional_fields_decode() {
        let json = r#"{"id":1,"name":"BTC","timeframe":"Hour","created_at_ms":1000,"charts":[]}"#;
        let decoded: SavedView = serde_json::from_str(json).expect("deserialize");
        assert_eq!(decoded.columns, None);
        assert_eq!(decoded.candle_count, 0);
        assert_eq!(decoded.last_opened_at_ms, None);
        assert_eq!(decoded.autosave, None);
    }
}
