//! What the assistant costs, and what to do about it: the monthly spending guard the user sets, and
//! the plan or rate limit the assistant reports.
//!
//! - The guard warns at a share of the limit and, at the limit, stops starting things nobody is
//!   watching (automatic tasks and automatic summaries). A chat the user starts is never stopped:
//!   they are at the keyboard and can see the figure.
//! - A limit on the Claude plan arrives as an event in the assistant's output, or only as text of
//!   an error. Either way it is kept, with the time it resets, so the app can say so in plain words
//!   instead of showing a raw error.

use crate::state::AppState;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// No limit is set.
    Off,
    Ok,
    Warn,
    Reached,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self { Level::Off => "off", Level::Ok => "ok", Level::Warn => "warn", Level::Reached => "reached" }
    }
}

/// Where `spent` stands against `limit` (US dollars) with a warning at `warn_percent`.
pub fn level(spent: f64, limit: Option<f64>, warn_percent: u8) -> Level {
    let Some(limit) = limit.filter(|l| l.is_finite() && *l > 0.0) else { return Level::Off };
    // Half a cent of slack: the figure is a sum of many small amounts.
    if spent + 0.005 >= limit { Level::Reached }
    else if spent >= limit * f64::from(warn_percent.clamp(1, 100)) / 100.0 { Level::Warn }
    else { Level::Ok }
}

/// This month's figures for the status page and the `usage.changed` event.
pub async fn usage_json(state: &AppState) -> Value {
    let spent = state.month_cost().await;
    let (limit, warn) = { let s = state.settings.read().await; (s.spending.monthly_limit_usd, s.spending.warn_percent) };
    let lv = level(spent, limit, warn);
    json!({
        "monthCostUsd": spent, "limitUsd": limit, "warnPercent": warn, "level": lv.name(),
        "percent": limit.filter(|l| *l > 0.0).map(|l| ((spent / l) * 100.0).floor() as u32),
    })
}

/// Has this month's limit been reached? Then nothing may start that nobody is watching.
pub async fn reached(state: &AppState) -> bool {
    let spent = state.month_cost().await;
    let (limit, warn) = { let s = state.settings.read().await; (s.spending.monthly_limit_usd, s.spending.warn_percent) };
    level(spent, limit, warn) == Level::Reached
}

fn money(usd: f64) -> String { format!("${usd:.2}") }

/// After a cost was added: say so (once per level and month) when the warning point or the limit is crossed.
pub async fn after_cost(state: &AppState) {
    let spent = state.month_cost().await;
    let (limit, warn) = { let s = state.settings.read().await; (s.spending.monthly_limit_usd, s.spending.warn_percent) };
    let lv = level(spent, limit, warn);
    state.emit(json!({"type": "usage.changed", "usage": usage_json(state).await}));
    let month = chrono::Local::now().format("%Y-%m").to_string();
    let told = {
        let mut seen = state.spend_told.lock().unwrap_or_else(|e| e.into_inner());
        if seen.0 != month { *seen = (month, Level::Off); }
        // A raised limit starts the counting afresh, and a lower level is never announced.
        if lv < seen.1 { seen.1 = lv; }
        let first = lv > seen.1 && lv >= Level::Warn;
        if first { seen.1 = lv; }
        first
    };
    if !told { return; }
    let Some(limit) = limit else { return };
    let ar = crate::tr::arabic();
    let title = if ar { "حدّ الإنفاق" } else { "Spending limit" };
    let body = if lv == Level::Reached {
        crate::tr::text_in(ar, "spend.reached", &[("limit", &money(limit))])
    } else {
        let pct = ((spent / limit) * 100.0).floor().to_string();
        crate::tr::text_in(ar, "spend.warn", &[("percent", &pct), ("spent", &money(spent)), ("limit", &money(limit))])
    };
    crate::log::info(format!("spending guard: {} ({} of {})", lv.name(), money(spent), money(limit)));
    state.notify(title, &body);
}

// ---------- the plan or rate limit the assistant reports ----------

/// A limit on the user's Claude plan (or the service) that the assistant reported.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanLimit {
    /// `true` once requests are being refused; `false` for "close to the limit".
    pub reached: bool,
    /// Which window: "five_hour", "seven_day"… as the assistant names it, when it says.
    pub window: Option<String>,
    /// When it resets, seconds since 1970, when known.
    pub resets_at: Option<i64>,
}

/// A `rate_limit_event` of the assistant's output. `Some(None)` = "no limit now" (clear it);
/// `None` = not such an event.
pub fn limit_from_event(ev: &Value) -> Option<Option<PlanLimit>> {
    if ev["type"] != "rate_limit_event" { return None; }
    let info = &ev["rate_limit_info"];
    let status = info["status"].as_str().unwrap_or("");
    let resets_at = info["resetsAt"].as_i64().or_else(|| info["resetsAt"].as_f64().map(|f| f as i64)).map(normalise_epoch);
    let window = info["rateLimitType"].as_str().map(String::from);
    Some(match status {
        "rejected" => Some(PlanLimit { reached: true, window, resets_at }),
        "allowed_warning" => Some(PlanLimit { reached: false, window, resets_at }),
        _ => None,
    })
}

/// Some versions give milliseconds, some seconds.
fn normalise_epoch(t: i64) -> i64 { if t > 100_000_000_000 { t / 1000 } else { t } }

/// A run's error text that says the plan's limit was reached (older and newer wordings), with the
/// reset time when the text carries one (`…limit reached|1760000000`).
pub fn limit_from_text(text: &str) -> Option<PlanLimit> {
    let low = text.to_lowercase();
    let says = ["usage limit reached", "hit your limit", "reached your usage limit", "usage limit has been reached", "you've reached your", "5-hour limit reached", "weekly limit reached"]
        .iter().any(|k| low.contains(k));
    if !says { return None; }
    let resets_at = text.rsplit('|').next().filter(|_| text.contains('|')).and_then(|t| t.trim().parse::<i64>().ok()).map(normalise_epoch);
    Some(PlanLimit { reached: true, window: None, resets_at })
}

impl PlanLimit {
    /// Is it still in force at `now`? One that said when it resets is over by then; one that did not
    /// is kept for a few hours (the shortest window), then forgotten.
    pub fn in_force(&self, seen_at: i64, now: i64) -> bool {
        match self.resets_at { Some(r) => now < r, None => now < seen_at + 5 * 3600 }
    }
}

/// What the status page and `limit.changed` carry: `null` when there is nothing to say.
pub fn plan_limit_json(state: &AppState) -> Value {
    let now = chrono::Utc::now().timestamp();
    let mut g = state.plan_limit.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((l, at)) = g.as_ref() {
        if l.in_force(*at, now) {
            return json!({"reached": l.reached, "window": l.window, "resetsAt": l.resets_at});
        }
        *g = None;
    }
    Value::Null
}

/// Remember (or, with `None`, forget) the limit, and tell the window when that changed what it shows.
pub fn set_plan_limit(state: &AppState, l: Option<PlanLimit>) {
    let before = plan_limit_json(state);
    *state.plan_limit.lock().unwrap_or_else(|e| e.into_inner()) = l.map(|l| (l, chrono::Utc::now().timestamp()));
    let after = plan_limit_json(state);
    if before != after { state.emit(json!({"type": "limit.changed", "planLimit": after})); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guard_warns_at_its_share_and_stops_at_the_limit() {
        assert_eq!(level(50.0, None, 80), Level::Off);
        assert_eq!(level(0.0, Some(20.0), 80), Level::Ok);
        assert_eq!(level(15.99, Some(20.0), 80), Level::Ok);
        assert_eq!(level(16.0, Some(20.0), 80), Level::Warn);
        assert_eq!(level(19.996, Some(20.0), 80), Level::Reached, "under half a cent short is the limit");
        assert_eq!(level(20.0, Some(20.0), 80), Level::Reached);
        assert_eq!(level(35.0, Some(20.0), 80), Level::Reached);
        assert_eq!(level(1.0, Some(0.0), 80), Level::Off, "a zero limit means none, not 'always reached'");
    }

    #[tokio::test]
    async fn the_month_total_follows_the_limit_and_says_so_once() {
        let st = AppState::for_test();
        let mut rx = st.events.subscribe();
        st.settings.write().await.spending.monthly_limit_usd = Some(10.0);
        st.add_usage(5.0).await;
        assert!(!reached(&st).await);
        st.add_usage(3.5).await; // 8.5: past 80 %
        st.add_usage(0.5).await; // 9.0: still the same level, no second warning
        st.add_usage(1.0).await; // 10.0: the limit
        assert!(reached(&st).await);
        st.add_usage(1.0).await; // beyond: no second notice
        let mut notices = vec![];
        let mut last_level = String::new();
        while let Ok(e) = rx.try_recv() {
            if e["type"] == "notify" { notices.push(e["body"].as_str().unwrap().to_string()); }
            if e["type"] == "usage.changed" { last_level = e["usage"]["level"].as_str().unwrap().to_string(); }
        }
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert!(notices[0].contains("85%") && notices[0].contains("$10.00"), "{}", notices[0]);
        assert!(notices[1].contains("is reached"), "{}", notices[1]);
        assert_eq!(last_level, "reached");
        // Raising the limit lifts the stop at once.
        st.settings.write().await.spending.monthly_limit_usd = Some(50.0);
        assert!(!reached(&st).await);
        let u = usage_json(&st).await;
        assert_eq!(u["level"], "ok");
        assert_eq!(u["percent"], 22);
    }

    #[test]
    fn a_plan_limit_is_read_from_the_event_and_from_error_text() {
        let ev = json!({"type": "rate_limit_event", "rate_limit_info": {"status": "rejected", "resetsAt": 1_900_000_000, "rateLimitType": "five_hour"}});
        let l = limit_from_event(&ev).unwrap().unwrap();
        assert!(l.reached && l.window.as_deref() == Some("five_hour") && l.resets_at == Some(1_900_000_000));
        let warn = json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed_warning", "resetsAt": 1_900_000_000_000_i64}});
        let l = limit_from_event(&warn).unwrap().unwrap();
        assert!(!l.reached && l.resets_at == Some(1_900_000_000), "milliseconds are turned into seconds");
        assert_eq!(limit_from_event(&json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed"}})), Some(None));
        assert_eq!(limit_from_event(&json!({"type": "result"})), None);

        let l = limit_from_text("Claude AI usage limit reached|1900000000").unwrap();
        assert!(l.reached && l.resets_at == Some(1_900_000_000));
        assert!(limit_from_text("You've hit your limit · resets 3pm").unwrap().resets_at.is_none());
        assert!(limit_from_text("invalid x-api-key").is_none());
    }

    #[test]
    fn a_limit_ends_when_it_resets_or_after_a_few_hours_when_it_does_not_say() {
        let with = PlanLimit { reached: true, window: None, resets_at: Some(1000) };
        assert!(with.in_force(0, 999) && !with.in_force(0, 1000));
        let without = PlanLimit { reached: true, window: None, resets_at: None };
        assert!(without.in_force(0, 5 * 3600 - 1) && !without.in_force(0, 5 * 3600));
    }

    #[test]
    fn the_state_keeps_and_forgets_the_limit_and_tells_the_window() {
        let st = AppState::for_test();
        let mut rx = st.events.subscribe();
        assert!(plan_limit_json(&st).is_null());
        let future = chrono::Utc::now().timestamp() + 3600;
        set_plan_limit(&st, Some(PlanLimit { reached: true, window: Some("five_hour".into()), resets_at: Some(future) }));
        assert_eq!(plan_limit_json(&st)["resetsAt"], future);
        assert_eq!(rx.try_recv().unwrap()["type"], "limit.changed");
        set_plan_limit(&st, Some(PlanLimit { reached: true, window: Some("five_hour".into()), resets_at: Some(future) }));
        assert!(rx.try_recv().is_err(), "nothing new to say");
        set_plan_limit(&st, None);
        assert!(plan_limit_json(&st).is_null());
        assert_eq!(rx.try_recv().unwrap()["type"], "limit.changed");
        // One that has already reset is never shown.
        set_plan_limit(&st, Some(PlanLimit { reached: true, window: None, resets_at: Some(chrono::Utc::now().timestamp() - 5) }));
        assert!(plan_limit_json(&st).is_null());
    }

    #[test]
    fn limit_settings_are_checked() {
        let mut s = crate::settings::SpendingSettings::default();
        assert!(s.sanitize().is_ok());
        s.monthly_limit_usd = Some(25.456);
        s.sanitize().unwrap();
        assert_eq!(s.monthly_limit_usd, Some(25.46));
        for bad in [0.0, -3.0, 0.5, 1e9, f64::NAN, f64::INFINITY] {
            s.monthly_limit_usd = Some(bad);
            assert!(s.sanitize().is_err(), "{bad}");
        }
        s.monthly_limit_usd = Some(10.0);
        s.warn_percent = 20;
        assert!(s.sanitize().is_err());
        s.warn_percent = 99;
        assert!(s.sanitize().is_err());
    }
}
