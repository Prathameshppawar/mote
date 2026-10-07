//! Builds the usage dashboard from aggregated usage rows.
//!
//! Storage aggregates raw events into compact hourly buckets (local time) and
//! returns latency samples; this module turns them into summaries, breakdowns,
//! time series and performance statistics. Costs are priced per bucket using
//! the price in effect on that day.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use super::pricing::PricingCatalog;
use super::UsageStatus;
use crate::providers::types::{Feature, RateLimitSnapshot};

/// Usage aggregated by local day, hour, provider, model, feature and status.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageBucket {
    pub day: NaiveDate,
    pub hour: u8,
    pub provider: String,
    pub model: String,
    pub feature: Feature,
    pub status: UsageStatus,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// Sum and count of latencies of requests that reported one.
    pub latency_sum_ms: u64,
    pub latency_count: u64,
    pub rate_limit_hits: u64,
}

/// Latency of one successful request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencySample {
    /// Local date of the request.
    pub day: NaiveDate,
    pub feature: Feature,
    pub latency_ms: u32,
}

/// Inputs to [`build`].
pub struct DashboardInput<'a> {
    pub buckets: &'a [UsageBucket],
    /// Successful-request latencies over the last 30 days.
    pub latencies: &'a [LatencySample],
    pub pricing: &'a PricingCatalog,
    /// Local date and hour of "now".
    pub today: NaiveDate,
    pub current_hour: u8,
    pub provider_limits: Option<RateLimitSnapshot>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PeriodSummary {
    pub requests: u64,
    pub successful: u64,
    pub failed: u64,
    pub rate_limited: u64,
    pub timeouts: u64,
    pub cancelled: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    /// False when some usage could not be priced (unknown model).
    pub cost_complete: bool,
    pub avg_latency_ms: Option<f64>,
    /// Failed / (requests − cancelled).
    pub error_rate: f64,
}

/// The dashboard's feature groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FeatureGroup {
    InlineCompletion,
    PromptEnhancement,
    WritingAssistance,
    Classification,
    ContextAnalysis,
    Other,
}

impl FeatureGroup {
    pub fn of(feature: Feature) -> Self {
        match feature {
            Feature::InlineCompletion => Self::InlineCompletion,
            Feature::PromptEnhancement => Self::PromptEnhancement,
            Feature::WritingAssistance => Self::WritingAssistance,
            Feature::IntentClassification => Self::Classification,
            Feature::ContextAnalysis => Self::ContextAnalysis,
            Feature::Translation | Feature::Rewrite | Feature::CommandInterface => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct FeatureUsage {
    pub feature: Feature,
    pub group: FeatureGroup,
    pub requests: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    /// Share of all requests in the window (0..=1).
    pub share: f64,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub provider: String,
    pub model: String,
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    /// `None` when Mote has no price for this model.
    pub estimated_cost_usd: Option<f64>,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SeriesPoint {
    /// `2026-10-07` for daily points, `14:00` for hourly points.
    pub label: String,
    pub requests: u64,
    pub failed: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_usd: f64,
    pub avg_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PerformanceStats {
    pub avg_latency_ms: Option<f64>,
    pub median_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub completion_avg_latency_ms: Option<f64>,
    pub completion_median_latency_ms: Option<f64>,
    pub classification_avg_latency_ms: Option<f64>,
    pub classification_median_latency_ms: Option<f64>,
    pub failed_requests: u64,
    /// HTTP 429 responses, including ones Mote retried successfully.
    pub rate_limit_events: u64,
    pub timeouts: u64,
    pub cancelled: u64,
    pub error_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct TopFeature {
    pub group: FeatureGroup,
    pub share: f64,
}

/// Feature, model and performance breakdowns for one time range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RangeBreakdown {
    pub summary: PeriodSummary,
    pub by_feature: Vec<FeatureUsage>,
    pub by_model: Vec<ModelUsage>,
    pub performance: PerformanceStats,
    pub top_feature: Option<TopFeature>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct UsageDashboard {
    #[cfg_attr(feature = "ts", ts(type = "string"))]
    pub generated_at: DateTime<Utc>,
    pub today: PeriodSummary,
    pub week: PeriodSummary,
    pub month: PeriodSummary,
    pub last30_days: PeriodSummary,
    /// Breakdowns for today, so one range filter can scope a whole view.
    pub breakdown_today: RangeBreakdown,
    /// Breakdowns for the last 30 days.
    pub breakdown_30d: RangeBreakdown,
    /// One point per day for the last 30 days (oldest first).
    pub daily: Vec<SeriesPoint>,
    /// One point per hour for today.
    pub hourly: Vec<SeriesPoint>,
    /// Rate limits most recently reported by the provider (provider usage).
    pub provider_limits: Option<RateLimitSnapshot>,
    /// Models with usage but no known price.
    pub unpriced_models: Vec<String>,
}

#[derive(Default)]
struct Acc {
    requests: u64,
    successful: u64,
    failed: u64,
    rate_limited: u64,
    timeouts: u64,
    cancelled: u64,
    input: u64,
    output: u64,
    total: u64,
    cost: f64,
    unpriced: bool,
    latency_sum: u64,
    latency_count: u64,
    rate_limit_hits: u64,
}

impl Acc {
    fn add(&mut self, b: &UsageBucket, cost: Option<f64>) {
        self.requests += b.requests;
        match b.status {
            UsageStatus::Success => self.successful += b.requests,
            UsageStatus::Cancelled => self.cancelled += b.requests,
            UsageStatus::RateLimited => {
                self.failed += b.requests;
                self.rate_limited += b.requests;
            }
            UsageStatus::Timeout => {
                self.failed += b.requests;
                self.timeouts += b.requests;
            }
            UsageStatus::Error => self.failed += b.requests,
        }
        self.input += b.input_tokens;
        self.output += b.output_tokens;
        self.total += b.total_tokens;
        match cost {
            Some(c) => self.cost += c,
            None if b.total_tokens > 0 => self.unpriced = true,
            None => {}
        }
        if b.status == UsageStatus::Success {
            self.latency_sum += b.latency_sum_ms;
            self.latency_count += b.latency_count;
        }
        self.rate_limit_hits += b.rate_limit_hits;
    }

    fn avg_latency(&self) -> Option<f64> {
        (self.latency_count > 0).then(|| self.latency_sum as f64 / self.latency_count as f64)
    }

    fn error_rate(&self) -> f64 {
        let attempted = self.requests.saturating_sub(self.cancelled);
        if attempted == 0 {
            0.0
        } else {
            self.failed as f64 / attempted as f64
        }
    }

    fn summary(&self) -> PeriodSummary {
        PeriodSummary {
            requests: self.requests,
            successful: self.successful,
            failed: self.failed,
            rate_limited: self.rate_limited,
            timeouts: self.timeouts,
            cancelled: self.cancelled,
            input_tokens: self.input,
            output_tokens: self.output,
            total_tokens: self.total,
            estimated_cost_usd: self.cost,
            cost_complete: !self.unpriced,
            avg_latency_ms: self.avg_latency(),
            error_rate: self.error_rate(),
        }
    }

    fn point(&self, label: String) -> SeriesPoint {
        SeriesPoint {
            label,
            requests: self.requests,
            failed: self.failed,
            input_tokens: self.input,
            output_tokens: self.output,
            total_tokens: self.total,
            estimated_cost_usd: self.cost,
            avg_latency_ms: self.avg_latency(),
        }
    }
}

/// Value at percentile `p` (0..=100) of sorted `values`, nearest-rank.
fn percentile(sorted: &[u32], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil().max(1.0) as usize;
    Some(f64::from(sorted[rank.min(sorted.len()) - 1]))
}

fn median(sorted: &[u32]) -> Option<f64> {
    match sorted.len() {
        0 => None,
        n if n % 2 == 1 => Some(f64::from(sorted[n / 2])),
        n => Some((f64::from(sorted[n / 2 - 1]) + f64::from(sorted[n / 2])) / 2.0),
    }
}

fn mean(values: &[u32]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().map(|v| f64::from(*v)).sum::<f64>() / values.len() as f64)
}

/// Accumulates one time range.
#[derive(Default)]
struct RangeAcc {
    total: Acc,
    by_feature: HashMap<Feature, Acc>,
    by_model: BTreeMap<(String, String), Acc>,
}

impl RangeAcc {
    fn add(&mut self, b: &UsageBucket, cost: Option<f64>) {
        self.total.add(b, cost);
        self.by_feature.entry(b.feature).or_default().add(b, cost);
        self.by_model.entry((b.provider.clone(), b.model.clone())).or_default().add(b, cost);
    }

    fn finish<'a>(self, latencies: impl Iterator<Item = &'a LatencySample>) -> RangeBreakdown {
        let requests = self.total.requests.max(1) as f64;
        let mut features: Vec<FeatureUsage> = self
            .by_feature
            .iter()
            .map(|(feature, acc)| FeatureUsage {
                feature: *feature,
                group: FeatureGroup::of(*feature),
                requests: acc.requests,
                total_tokens: acc.total,
                estimated_cost_usd: acc.cost,
                share: acc.requests as f64 / requests,
                avg_latency_ms: acc.avg_latency(),
            })
            .collect();
        features.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| a.feature.as_str().cmp(b.feature.as_str())));

        let mut models: Vec<ModelUsage> = self
            .by_model
            .iter()
            .map(|((provider, model), acc)| ModelUsage {
                provider: provider.clone(),
                model: model.clone(),
                requests: acc.requests,
                input_tokens: acc.input,
                output_tokens: acc.output,
                total_tokens: acc.total,
                estimated_cost_usd: (!acc.unpriced).then_some(acc.cost),
                avg_latency_ms: acc.avg_latency(),
            })
            .collect();
        models.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens).then_with(|| a.model.cmp(&b.model)));

        let samples: Vec<&LatencySample> = latencies.collect();
        let sorted = |filter: Option<Feature>| {
            let mut v: Vec<u32> =
                samples.iter().filter(|s| filter.is_none_or(|f| s.feature == f)).map(|s| s.latency_ms).collect();
            v.sort_unstable();
            v
        };
        let all = sorted(None);
        let completion = sorted(Some(Feature::InlineCompletion));
        let classification = sorted(Some(Feature::IntentClassification));
        let performance = PerformanceStats {
            avg_latency_ms: mean(&all).or_else(|| self.total.avg_latency()),
            median_latency_ms: median(&all),
            p95_latency_ms: percentile(&all, 95.0),
            completion_avg_latency_ms: mean(&completion),
            completion_median_latency_ms: median(&completion),
            classification_avg_latency_ms: mean(&classification),
            classification_median_latency_ms: median(&classification),
            failed_requests: self.total.failed,
            rate_limit_events: self.total.rate_limit_hits,
            timeouts: self.total.timeouts,
            cancelled: self.total.cancelled,
            error_rate: self.total.error_rate(),
        };

        let mut groups: HashMap<FeatureGroup, u64> = HashMap::new();
        for f in &features {
            *groups.entry(f.group).or_default() += f.requests;
        }
        let top_feature = groups
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| format!("{:?}", b.0).cmp(&format!("{:?}", a.0))))
            .map(|(group, n)| TopFeature { group, share: n as f64 / requests });

        RangeBreakdown {
            summary: self.total.summary(),
            by_feature: features,
            by_model: models,
            performance,
            top_feature,
        }
    }
}

/// Builds the dashboard.
pub fn build(input: DashboardInput<'_>) -> UsageDashboard {
    let today = input.today;
    let week_start = today - Duration::days(i64::from(today.weekday().num_days_from_monday()));
    let month_start = today.with_day(1).unwrap_or(today);
    let window_start = today - Duration::days(29);

    let (mut week_acc, mut month_acc) = (Acc::default(), Acc::default());
    let (mut today_range, mut window_range) = (RangeAcc::default(), RangeAcc::default());
    let mut daily: BTreeMap<NaiveDate, Acc> = BTreeMap::new();
    let mut hourly: BTreeMap<u8, Acc> = BTreeMap::new();
    let mut unpriced: Vec<String> = Vec::new();

    for b in input.buckets {
        let cost = input.pricing.estimate(&b.provider, &b.model, b.day, b.input_tokens, b.output_tokens);
        if cost.is_none() && b.total_tokens > 0 && !unpriced.contains(&b.model) {
            unpriced.push(b.model.clone());
        }
        if b.day == today {
            today_range.add(b, cost);
            hourly.entry(b.hour).or_default().add(b, cost);
        }
        if b.day >= week_start && b.day <= today {
            week_acc.add(b, cost);
        }
        if b.day >= month_start && b.day <= today {
            month_acc.add(b, cost);
        }
        if b.day >= window_start && b.day <= today {
            window_range.add(b, cost);
            daily.entry(b.day).or_default().add(b, cost);
        }
    }

    let daily_points: Vec<SeriesPoint> = (0..30)
        .map(|offset| {
            let day = window_start + Duration::days(offset);
            daily.get(&day).map_or_else(
                || SeriesPoint { label: day.to_string(), ..SeriesPoint::default() },
                |acc| acc.point(day.to_string()),
            )
        })
        .collect();
    let hourly_points: Vec<SeriesPoint> = (0..=input.current_hour.min(23))
        .map(|hour| {
            let label = format!("{hour:02}:00");
            match hourly.get(&hour) {
                Some(acc) => acc.point(label),
                None => SeriesPoint { label, ..SeriesPoint::default() },
            }
        })
        .collect();
    unpriced.sort();

    let breakdown_today = today_range.finish(input.latencies.iter().filter(|s| s.day == today));
    let breakdown_30d = window_range.finish(input.latencies.iter().filter(|s| s.day >= window_start && s.day <= today));
    UsageDashboard {
        generated_at: input.generated_at,
        today: breakdown_today.summary.clone(),
        week: week_acc.summary(),
        month: month_acc.summary(),
        last30_days: breakdown_30d.summary.clone(),
        breakdown_today,
        breakdown_30d,
        daily: daily_points,
        hourly: hourly_points,
        provider_limits: input.provider_limits,
        unpriced_models: unpriced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::pricing::builtin_pricing;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn bucket(
        day: &str,
        hour: u8,
        model: &str,
        feature: Feature,
        status: UsageStatus,
        requests: u64,
        input: u64,
        output: u64,
        latency_sum: u64,
    ) -> UsageBucket {
        UsageBucket {
            day: d(day),
            hour,
            provider: "groq".into(),
            model: model.into(),
            feature,
            status,
            requests,
            input_tokens: input,
            output_tokens: output,
            total_tokens: input + output,
            latency_sum_ms: latency_sum,
            latency_count: if latency_sum > 0 { requests } else { 0 },
            rate_limit_hits: if status == UsageStatus::RateLimited { requests } else { 0 },
        }
    }

    const QWEN: &str = "qwen/qwen3.8-27b";
    const OSS: &str = "openai/gpt-oss-120b";

    fn sample() -> Vec<UsageBucket> {
        vec![
            // Today: Wednesday 2026-10-07
            bucket("2026-10-07", 9, QWEN, Feature::InlineCompletion, UsageStatus::Success, 10, 1_000, 200, 4_000),
            bucket("2026-10-07", 9, QWEN, Feature::InlineCompletion, UsageStatus::Cancelled, 4, 0, 0, 0),
            bucket("2026-10-07", 14, OSS, Feature::PromptEnhancement, UsageStatus::Success, 2, 600, 400, 3_000),
            bucket("2026-10-07", 14, QWEN, Feature::InlineCompletion, UsageStatus::RateLimited, 1, 0, 0, 0),
            // Monday this week
            bucket("2026-10-05", 10, QWEN, Feature::IntentClassification, UsageStatus::Success, 5, 500, 50, 1_500),
            // Earlier this month, last week
            bucket("2026-10-01", 10, QWEN, Feature::WritingAssistance, UsageStatus::Error, 2, 0, 0, 0),
            // Last month, inside the 30-day window
            bucket("2026-09-20", 10, "mystery-model", Feature::Rewrite, UsageStatus::Success, 1, 100, 100, 900),
            // Outside the window
            bucket("2026-08-01", 10, QWEN, Feature::InlineCompletion, UsageStatus::Success, 99, 9_999, 9_999, 1),
        ]
    }

    fn build_sample(latencies: &[LatencySample]) -> UsageDashboard {
        let pricing = PricingCatalog::new(builtin_pricing());
        let buckets = sample();
        build(DashboardInput {
            buckets: &buckets,
            latencies,
            pricing: &pricing,
            today: d("2026-10-07"),
            current_hour: 15,
            provider_limits: None,
            generated_at: Utc::now(),
        })
    }

    #[test]
    fn daily_weekly_monthly_aggregation() {
        let dash = build_sample(&[]);
        assert_eq!(dash.today.requests, 17);
        assert_eq!(dash.today.successful, 12);
        assert_eq!(dash.today.cancelled, 4);
        assert_eq!(dash.today.failed, 1);
        assert_eq!(dash.today.rate_limited, 1);
        assert_eq!(dash.today.input_tokens, 1_600);
        assert_eq!(dash.today.output_tokens, 600);
        assert_eq!(dash.today.total_tokens, 2_200);
        assert_eq!(dash.week.requests, 22, "today + Monday");
        assert_eq!(dash.month.requests, 24, "October only");
        assert_eq!(dash.last30_days.requests, 25, "includes Sep 20, excludes Aug 1");
    }

    #[test]
    fn error_rate_excludes_cancelled_requests() {
        let dash = build_sample(&[]);
        // Today: 1 failure out of 17 - 4 cancelled = 13 attempted.
        assert!((dash.today.error_rate - 1.0 / 13.0).abs() < 1e-9);
    }

    #[test]
    fn estimated_cost_uses_pricing() {
        let dash = build_sample(&[]);
        let expected = (1_000.0 * 0.80 + 200.0 * 4.0) / 1e6 + (600.0 * 0.15 + 400.0 * 0.60) / 1e6;
        assert!((dash.today.estimated_cost_usd - expected).abs() < 1e-12);
        assert!(dash.today.cost_complete);
        assert!(!dash.last30_days.cost_complete, "mystery-model has no price");
        assert_eq!(dash.unpriced_models, vec!["mystery-model".to_string()]);
        let mystery = dash.breakdown_30d.by_model.iter().find(|m| m.model == "mystery-model").unwrap();
        assert_eq!(mystery.estimated_cost_usd, None);
    }

    #[test]
    fn feature_and_model_breakdowns() {
        let dash = build_sample(&[]);
        let completion = dash.breakdown_30d.by_feature.iter().find(|f| f.feature == Feature::InlineCompletion).unwrap();
        assert_eq!(completion.requests, 15);
        assert_eq!(completion.group, FeatureGroup::InlineCompletion);
        let rewrite = dash.breakdown_30d.by_feature.iter().find(|f| f.feature == Feature::Rewrite).unwrap();
        assert_eq!(rewrite.group, FeatureGroup::Other);
        let shares: f64 = dash.breakdown_30d.by_feature.iter().map(|f| f.share).sum();
        assert!((shares - 1.0).abs() < 1e-9);
        let qwen = dash.breakdown_30d.by_model.iter().find(|m| m.model == QWEN).unwrap();
        assert_eq!((qwen.requests, qwen.input_tokens, qwen.output_tokens), (22, 1_500, 250));
        assert_eq!(dash.breakdown_30d.top_feature.as_ref().unwrap().group, FeatureGroup::InlineCompletion);
    }

    #[test]
    fn time_series_are_zero_filled() {
        let dash = build_sample(&[]);
        assert_eq!(dash.daily.len(), 30);
        assert_eq!(dash.daily.first().unwrap().label, "2026-09-08");
        assert_eq!(dash.daily.last().unwrap().label, "2026-10-07");
        assert_eq!(dash.daily.last().unwrap().requests, 17);
        assert_eq!(dash.daily.iter().find(|p| p.label == "2026-10-02").unwrap().requests, 0);
        assert_eq!(dash.hourly.len(), 16, "00:00 through 15:00");
        assert_eq!(dash.hourly[9].requests, 14);
        assert_eq!(dash.hourly[14].requests, 3);
        assert_eq!(dash.hourly[14].avg_latency_ms, Some(1_500.0));
    }

    #[test]
    fn performance_statistics() {
        let latencies: Vec<LatencySample> = [300, 400, 500, 600, 2_000]
            .into_iter()
            .map(|ms| LatencySample { day: d("2026-10-06"), feature: Feature::InlineCompletion, latency_ms: ms })
            .chain([LatencySample { day: d("2026-10-07"), feature: Feature::IntentClassification, latency_ms: 250 }])
            .collect();
        let dash = build_sample(&latencies);
        let p = &dash.breakdown_30d.performance;
        assert_eq!(p.completion_median_latency_ms, Some(500.0));
        assert_eq!(p.completion_avg_latency_ms, Some(760.0));
        assert_eq!(p.classification_median_latency_ms, Some(250.0));
        assert_eq!(p.median_latency_ms, Some(450.0));
        assert_eq!(p.p95_latency_ms, Some(2_000.0));
        assert_eq!(p.rate_limit_events, 1);
        assert_eq!(p.failed_requests, 3);
        assert_eq!(p.cancelled, 4);
    }

    #[test]
    fn today_breakdown_is_scoped_to_today() {
        let latencies = [
            LatencySample { day: d("2026-10-07"), feature: Feature::InlineCompletion, latency_ms: 300 },
            LatencySample { day: d("2026-10-01"), feature: Feature::InlineCompletion, latency_ms: 900 },
        ];
        let dash = build_sample(&latencies);
        let t = &dash.breakdown_today;
        assert_eq!(t.summary, dash.today);
        let features: Vec<Feature> = t.by_feature.iter().map(|f| f.feature).collect();
        assert_eq!(features, vec![Feature::InlineCompletion, Feature::PromptEnhancement]);
        assert_eq!(t.by_feature[0].requests, 15);
        assert_eq!(t.performance.completion_median_latency_ms, Some(300.0), "only today's samples");
        assert_eq!(t.performance.rate_limit_events, 1);
        assert_eq!(dash.breakdown_30d.performance.completion_median_latency_ms, Some(600.0));
        assert_eq!(t.top_feature.as_ref().unwrap().group, FeatureGroup::InlineCompletion);
    }

    #[test]
    fn empty_input_produces_an_empty_dashboard() {
        let pricing = PricingCatalog::new(builtin_pricing());
        let dash = build(DashboardInput {
            buckets: &[],
            latencies: &[],
            pricing: &pricing,
            today: d("2026-10-07"),
            current_hour: 0,
            provider_limits: None,
            generated_at: Utc::now(),
        });
        assert_eq!(dash.today, PeriodSummary { cost_complete: true, ..PeriodSummary::default() });
        assert_eq!(dash.breakdown_30d.performance.median_latency_ms, None);
        assert!(dash.breakdown_30d.top_feature.is_none());
        assert_eq!(dash.hourly.len(), 1);
    }
}
