//! Model pricing and estimated cost.
//!
//! Costs shown by Mote are **estimates**: local token counts multiplied by the
//! list price in effect on the day of each request. The provider's own console
//! is the source of truth for billing (free tiers, cached-input discounts and
//! batch pricing are not modelled). Pricing is data, not code: users can add or
//! correct rows, and a new row only affects usage from its effective date on.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::settings::is_valid_model_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PricingSource {
    /// Shipped with Mote.
    Builtin,
    /// Added or edited by the user.
    User,
}

impl PricingSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == "builtin" {
            Self::Builtin
        } else {
            Self::User
        }
    }
}

/// List price of one model from a given date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub id: Option<i64>,
    pub provider: String,
    pub model: String,
    /// USD per one million input tokens.
    pub input_cost_per_million: f64,
    /// USD per one million output tokens.
    pub output_cost_per_million: f64,
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub effective_date: NaiveDate,
    pub source: PricingSource,
}

impl ModelPricing {
    /// Estimated cost in USD for the given token counts.
    pub fn cost(&self, input_tokens: u64, output_tokens: u64) -> f64 {
        input_tokens as f64 / 1_000_000.0 * self.input_cost_per_million
            + output_tokens as f64 / 1_000_000.0 * self.output_cost_per_million
    }

    /// Validates a user-supplied pricing row.
    pub fn validate(&self) -> Result<(), String> {
        if self.provider.trim().is_empty() || self.provider.len() > 64 {
            return Err("Provider is required.".into());
        }
        if !is_valid_model_id(&self.model) {
            return Err("Model ID contains invalid characters.".into());
        }
        for (label, value) in [("Input", self.input_cost_per_million), ("Output", self.output_cost_per_million)] {
            if !value.is_finite() || !(0.0..=1_000.0).contains(&value) {
                return Err(format!("{label} price must be between $0 and $1000 per million tokens."));
            }
        }
        let earliest = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap_or_default();
        let latest = NaiveDate::from_ymd_opt(2100, 1, 1).unwrap_or_default();
        if self.effective_date < earliest || self.effective_date > latest {
            return Err("Effective date must be between 2020 and 2100.".into());
        }
        Ok(())
    }
}

/// Date on which the built-in prices were checked against groq.com/pricing.
pub const BUILTIN_PRICING_DATE: (i32, u32, u32) = (2026, 10, 7);

/// Prices shipped with Mote (USD per million tokens), as listed by Groq on
/// [`BUILTIN_PRICING_DATE`].
pub fn builtin_pricing() -> Vec<ModelPricing> {
    let (y, m, d) = BUILTIN_PRICING_DATE;
    let date = NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default();
    [("openai/gpt-oss-20b", 0.075, 0.30), ("openai/gpt-oss-120b", 0.15, 0.60), ("qwen/qwen3.8-27b", 0.80, 4.00)]
        .into_iter()
        .map(|(model, input, output)| ModelPricing {
            id: None,
            provider: "groq".into(),
            model: model.into(),
            input_cost_per_million: input,
            output_cost_per_million: output,
            effective_date: date,
            source: PricingSource::Builtin,
        })
        .collect()
}

/// All known prices, queryable by date.
#[derive(Debug, Clone, Default)]
pub struct PricingCatalog {
    entries: Vec<ModelPricing>,
}

impl PricingCatalog {
    pub fn new(mut entries: Vec<ModelPricing>) -> Self {
        entries.sort_by(|a, b| {
            (a.provider.as_str(), a.model.as_str(), a.effective_date, a.source == PricingSource::User).cmp(&(
                b.provider.as_str(),
                b.model.as_str(),
                b.effective_date,
                b.source == PricingSource::User,
            ))
        });
        Self { entries }
    }

    pub fn entries(&self) -> &[ModelPricing] {
        &self.entries
    }

    /// The price in effect on `date`: the latest row on or before that date
    /// (user rows win ties). Usage older than the earliest row is priced with
    /// the earliest row, since prices rarely rise retroactively.
    pub fn price_at(&self, provider: &str, model: &str, date: NaiveDate) -> Option<&ModelPricing> {
        let rows: Vec<&ModelPricing> =
            self.entries.iter().filter(|e| e.provider == provider && e.model == model).collect();
        rows.iter().rev().find(|e| e.effective_date <= date).or_else(|| rows.first()).copied()
    }

    /// Estimated cost, or `None` when the model has no known price.
    pub fn estimate(&self, provider: &str, model: &str, date: NaiveDate, input: u64, output: u64) -> Option<f64> {
        self.price_at(provider, model, date).map(|p| p.cost(input, output))
    }

    pub fn has_price(&self, provider: &str, model: &str) -> bool {
        self.entries.iter().any(|e| e.provider == provider && e.model == model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn builtin_prices_are_valid() {
        let rows = builtin_pricing();
        assert_eq!(rows.len(), 3);
        for row in &rows {
            row.validate().unwrap();
        }
    }

    #[test]
    fn cost_calculation() {
        let catalog = PricingCatalog::new(builtin_pricing());
        // 1M input + 1M output on gpt-oss-20b = 0.075 + 0.30
        let cost = catalog.estimate("groq", "openai/gpt-oss-20b", date("2026-10-08"), 1_000_000, 1_000_000).unwrap();
        assert!((cost - 0.375).abs() < 1e-9);
        // 123 in / 31 out on qwen: 123*0.8e-6 + 31*4e-6
        let cost = catalog.estimate("groq", "qwen/qwen3.8-27b", date("2026-10-08"), 123, 31).unwrap();
        assert!((cost - (123.0 * 0.8 + 31.0 * 4.0) / 1e6).abs() < 1e-12);
        assert_eq!(catalog.estimate("groq", "unknown-model", date("2026-10-08"), 10, 10), None);
    }

    #[test]
    fn effective_dates_apply_from_their_day() {
        let mut rows = builtin_pricing();
        rows.push(ModelPricing {
            id: Some(9),
            provider: "groq".into(),
            model: "openai/gpt-oss-20b".into(),
            input_cost_per_million: 0.05,
            output_cost_per_million: 0.20,
            effective_date: date("2026-11-01"),
            source: PricingSource::User,
        });
        let c = PricingCatalog::new(rows);
        assert_eq!(c.price_at("groq", "openai/gpt-oss-20b", date("2026-10-31")).unwrap().input_cost_per_million, 0.075);
        assert_eq!(c.price_at("groq", "openai/gpt-oss-20b", date("2026-11-01")).unwrap().input_cost_per_million, 0.05);
        // Usage before the earliest row uses the earliest row.
        assert_eq!(c.price_at("groq", "openai/gpt-oss-20b", date("2026-01-01")).unwrap().input_cost_per_million, 0.075);
    }

    #[test]
    fn user_rows_win_ties() {
        let mut rows = builtin_pricing();
        let mut user = rows[0].clone();
        user.input_cost_per_million = 0.01;
        user.source = PricingSource::User;
        rows.push(user.clone());
        let c = PricingCatalog::new(rows);
        assert_eq!(c.price_at("groq", &user.model, user.effective_date).unwrap().input_cost_per_million, 0.01);
    }

    #[test]
    fn validation_rejects_bad_rows() {
        let mut row = builtin_pricing().remove(0);
        row.input_cost_per_million = -1.0;
        assert!(row.validate().is_err());
        row.input_cost_per_million = f64::NAN;
        assert!(row.validate().is_err());
        row.input_cost_per_million = 0.1;
        row.model = "bad id".into();
        assert!(row.validate().is_err());
    }
}
