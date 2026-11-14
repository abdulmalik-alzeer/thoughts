//! What tokens would cost at the providers' published API prices — the one place the app keeps
//! prices, for the usage page's estimates.
//!
//! - Claude Code reports what each of its runs cost (`total_cost_usd`, `modelUsage.*.costUSD`) and
//!   that figure is used as it is. This table prices only what carries no cost: Codex runs, and the
//!   Claude sessions read from this computer's Claude folder when the user asked for "Everything on
//!   this computer".
//! - Every figure made from this table is an estimate and is shown as one. A model that is not in the
//!   table has no price: its tokens are counted and its cost is "unknown", never a guess.
//! - This file is part of the core, not of the customisation layer, so no customisation can change
//!   it. It is the only list of model names the app keeps (model menus come from the assistant
//!   programs themselves, see the repository's CLAUDE.md).
//!
//! Prices are US dollars per million tokens, as published on 2026-11-14:
//! - Anthropic: <https://platform.claude.com/docs/en/about-claude/pricing> (standard tier, global
//!   routing; a 1-hour cache write is twice the input price).
//! - OpenAI: <https://developers.openai.com/api/docs/pricing> (standard tier). OpenAI charges no
//!   premium for writing the cache, so a cache write is priced as plain input.
//!
//! When a provider changes a price or adds a model, add or change a row here (and its test).

/// One model's prices, in US dollars per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rate {
    pub input: f64,
    pub cache_read: f64,
    /// A write to the cache that lasts 5 minutes (Anthropic); plain input for OpenAI.
    pub cache_write: f64,
    pub output: f64,
}

/// A model's row: its prices, and the higher prices for one request whose prompt is over a size.
#[derive(Clone, Copy, Debug)]
pub struct Price {
    pub base: Rate,
    /// (prompt tokens above which the request is priced at `rate`, `rate`).
    pub long: Option<(u64, Rate)>,
}

const fn r(input: f64, cache_read: f64, cache_write: f64, output: f64) -> Rate { Rate { input, cache_read, cache_write, output } }
const fn p(base: Rate) -> Price { Price { base, long: None } }

/// (model id, price). Ids are matched exactly, after `normalise` (no partial or prefix matches, so a
/// new model is "unknown" rather than priced as an older one).
const TABLE: &[(&str, Price)] = &[
    // ---- Anthropic (Claude) ----
    ("claude-fable-5-2", p(r(8.0, 0.20, 10.0, 40.0))),
    ("claude-mythos-5-2", p(r(8.0, 0.20, 10.0, 40.0))),
    ("claude-fable-5-1", p(r(10.0, 0.25, 12.50, 50.0))),
    ("claude-mythos-5-1", p(r(10.0, 0.25, 12.50, 50.0))),
    ("claude-fable-5", p(r(10.0, 1.0, 12.50, 50.0))),
    ("claude-mythos-5", p(r(10.0, 1.0, 12.50, 50.0))),
    ("claude-opus-5-5", p(r(4.0, 0.20, 5.0, 20.0))),
    ("claude-opus-5", p(r(5.0, 0.50, 6.25, 25.0))),
    ("claude-opus-4-8", p(r(5.0, 0.50, 6.25, 25.0))),
    ("claude-opus-4-7", p(r(5.0, 0.50, 6.25, 25.0))),
    ("claude-opus-4-6", p(r(5.0, 0.50, 6.25, 25.0))),
    ("claude-opus-4-5", p(r(5.0, 0.50, 6.25, 25.0))),
    ("claude-opus-4-1", p(r(15.0, 1.50, 18.75, 75.0))),
    ("claude-opus-4", p(r(15.0, 1.50, 18.75, 75.0))),
    ("claude-opus-4-0", p(r(15.0, 1.50, 18.75, 75.0))),
    ("claude-sonnet-5-5", p(r(2.0, 0.10, 2.50, 10.0))),
    ("claude-sonnet-5", p(r(2.0, 0.20, 2.50, 10.0))),
    ("claude-sonnet-4-6", p(r(3.0, 0.30, 3.75, 15.0))),
    ("claude-sonnet-4-5", p(r(3.0, 0.30, 3.75, 15.0))),
    ("claude-sonnet-4", p(r(3.0, 0.30, 3.75, 15.0))),
    ("claude-sonnet-4-0", p(r(3.0, 0.30, 3.75, 15.0))),
    ("claude-3-7-sonnet", p(r(3.0, 0.30, 3.75, 15.0))),
    ("claude-haiku-5-5", Price { base: r(0.10, 0.01, 0.125, 0.50), long: Some((100_000, r(0.50, 0.05, 0.625, 2.50))) }),
    ("claude-haiku-4-5", p(r(1.0, 0.10, 1.25, 5.0))),
    ("claude-3-5-haiku", p(r(0.80, 0.08, 1.0, 4.0))),
    // ---- OpenAI (Codex) ----
    ("gpt-6-astra", Price { base: r(10.0, 1.0, 10.0, 50.0), long: Some((272_000, r(20.0, 2.0, 20.0, 75.0))) }),
    ("gpt-6.1-sol", Price { base: r(2.0, 0.10, 2.0, 10.0), long: Some((272_000, r(4.0, 0.20, 4.0, 15.0))) }),
    ("gpt-6.1-terra", Price { base: r(1.0, 0.05, 1.0, 5.0), long: Some((272_000, r(2.0, 0.10, 2.0, 7.50))) }),
    ("gpt-6-sol", Price { base: r(2.0, 0.20, 2.0, 10.0), long: Some((272_000, r(4.0, 0.40, 4.0, 15.0))) }),
    ("gpt-6-luna", Price { base: r(0.10, 0.01, 0.10, 0.50), long: Some((272_000, r(0.20, 0.02, 0.20, 0.75))) }),
    ("gpt-5.6-sol", Price { base: r(4.0, 0.40, 4.0, 20.0), long: Some((272_000, r(8.0, 0.80, 8.0, 30.0))) }),
    ("gpt-5.6-terra", Price { base: r(2.0, 0.20, 2.0, 12.0), long: Some((272_000, r(4.0, 0.40, 4.0, 18.0))) }),
    ("gpt-5.6-luna", Price { base: r(0.20, 0.02, 0.20, 1.20), long: Some((272_000, r(0.40, 0.04, 0.40, 1.80))) }),
    ("gpt-5.5", Price { base: r(5.0, 0.50, 5.0, 30.0), long: Some((272_000, r(10.0, 1.0, 10.0, 45.0))) }),
    ("gpt-5.4", Price { base: r(2.50, 0.25, 2.50, 15.0), long: Some((272_000, r(5.0, 0.50, 5.0, 22.50))) }),
    ("gpt-5.4-mini", p(r(0.75, 0.075, 0.75, 4.50))),
    ("gpt-5.4-nano", p(r(0.20, 0.02, 0.20, 1.25))),
    ("gpt-5.3-codex", p(r(1.75, 0.175, 1.75, 14.0))),
    ("gpt-5.2", p(r(1.75, 0.175, 1.75, 14.0))),
    ("gpt-5.2-codex", p(r(1.75, 0.175, 1.75, 14.0))),
    ("gpt-5.1", p(r(1.25, 0.125, 1.25, 10.0))),
    ("gpt-5.1-codex", p(r(1.25, 0.125, 1.25, 10.0))),
    ("gpt-5.1-codex-max", p(r(1.25, 0.125, 1.25, 10.0))),
    ("gpt-5.1-codex-mini", p(r(0.25, 0.025, 0.25, 2.0))),
    ("gpt-5", p(r(1.25, 0.125, 1.25, 10.0))),
    ("gpt-5-codex", p(r(1.25, 0.125, 1.25, 10.0))),
    ("gpt-5-mini", p(r(0.25, 0.025, 0.25, 2.0))),
    ("gpt-5-nano", p(r(0.05, 0.005, 0.05, 0.40))),
    ("codex-mini-latest", p(r(1.50, 0.375, 1.50, 6.0))),
    ("gpt-4.1", p(r(2.0, 0.50, 2.0, 8.0))),
    ("o3", p(r(2.0, 0.50, 2.0, 8.0))),
    ("o4-mini", p(r(1.10, 0.275, 1.10, 4.40))),
];

/// A model name as the programs report it, reduced to the id in the table: lower case, without a
/// provider prefix (`anthropic.`, `us.anthropic.`, `openai/`), a context-size tag (`[1m]`) or a
/// release date (`-20251001`, `-2025-08-07`).
pub fn normalise(model: &str) -> String {
    let mut m = model.trim().to_lowercase();
    if let Some(i) = m.find('[') { m.truncate(i); }
    for pre in ["openai/", "anthropic/"] { if let Some(rest) = m.strip_prefix(pre) { m = rest.to_string(); } }
    if let Some(i) = m.find("anthropic.") { m = m[i + "anthropic.".len()..].to_string(); }
    // A Bedrock version tag (`…-v1:0`).
    if let Some(i) = m.rfind("-v") { if m[i + 2..].chars().all(|c| c.is_ascii_digit() || c == ':') && m.len() > i + 2 { m.truncate(i); } }
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // -YYYYMMDD
    if m.len() > 9 && m.as_bytes()[m.len() - 9] == b'-' && digits(&m[m.len() - 8..]) { m.truncate(m.len() - 9); }
    // -YYYY-MM-DD
    if m.len() > 11 {
        let tail = &m[m.len() - 11..];
        let b = tail.as_bytes();
        if b[0] == b'-' && b[5] == b'-' && b[8] == b'-' && digits(&tail[1..5]) && digits(&tail[6..8]) && digits(&tail[9..]) { m.truncate(m.len() - 11); }
    }
    m.trim_end_matches('-').to_string()
}

/// The price of `model`, when the table has it.
pub fn price(model: &str) -> Option<Price> {
    let m = normalise(model);
    if m.is_empty() { return None; }
    TABLE.iter().find(|(id, _)| *id == m).map(|(_, p)| *p)
}

/// Tokens in the four kinds that are priced differently. `input` is the input that was neither read
/// from nor written to the cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    pub fn total(&self) -> u64 { self.input + self.output + self.cache_read + self.cache_write }
    pub fn add(&mut self, o: &Tokens) {
        self.input += o.input; self.output += o.output; self.cache_read += o.cache_read; self.cache_write += o.cache_write;
    }
    pub fn is_empty(&self) -> bool { self.total() == 0 }
}

/// What `t` costs at `rate` (US dollars), `cache_write_long` of the cache writes lasting an hour.
fn at(rate: &Rate, t: &Tokens, cache_write_long: u64) -> f64 {
    let long = cache_write_long.min(t.cache_write);
    let m = |n: u64, per: f64| n as f64 * per / 1_000_000.0;
    m(t.input, rate.input) + m(t.cache_read, rate.cache_read) + m(t.cache_write - long, rate.cache_write)
        + m(long, rate.input * 2.0) + m(t.output, rate.output)
}

/// What one request (`single`) or a sum of requests would cost on `model`; `None` for a model the
/// table does not have. Only a single request can be priced at the higher rate for a long prompt
/// (a sum says nothing about the size of each prompt in it).
pub fn cost(model: &str, t: &Tokens, cache_write_long: u64, single: bool) -> Option<f64> {
    let p = price(model)?;
    let prompt = t.input + t.cache_read + t.cache_write;
    let rate = match p.long { Some((over, ref l)) if single && prompt > over => l, _ => &p.base };
    Some(at(rate, t, cache_write_long))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_reduced_to_the_table_id_and_never_guessed() {
        assert_eq!(normalise("claude-opus-4-5-20251101"), "claude-opus-4-5");
        assert_eq!(normalise("claude-sonnet-4-5-20250929[1m]"), "claude-sonnet-4-5");
        assert_eq!(normalise("us.anthropic.claude-haiku-4-5-20251001-v1:0"), "claude-haiku-4-5");
        assert_eq!(normalise("gpt-5-2025-08-07"), "gpt-5");
        assert_eq!(normalise(" GPT-5.1-Codex "), "gpt-5.1-codex");
        assert_eq!(normalise("openai/gpt-5-mini"), "gpt-5-mini");
        assert!(price("claude-opus-5-5").is_some() && price("gpt-5.2-codex").is_some());
        // A model the table does not know has no price, however close its name is to one it does.
        for unknown in ["", "gpt-5.9", "gpt-5-turbo", "claude-opus-9", "claude-opus-5-5-fast", "<synthetic>", "llama3"] {
            assert!(price(unknown).is_none(), "{unknown}");
        }
    }

    #[test]
    fn every_row_is_a_sane_price() {
        let mut seen = std::collections::HashSet::new();
        for (id, p) in TABLE {
            assert!(seen.insert(*id), "{id} twice");
            assert_eq!(normalise(id), *id, "{id} must be written as `normalise` leaves it");
            for rt in std::iter::once(&p.base).chain(p.long.as_ref().map(|l| &l.1)) {
                assert!(rt.input > 0.0 && rt.output >= rt.input && rt.cache_read < rt.input && rt.cache_write >= rt.input, "{id}");
            }
        }
    }

    #[test]
    fn costs_follow_the_published_prices() {
        let t = Tokens { input: 1_000_000, output: 1_000_000, cache_read: 1_000_000, cache_write: 1_000_000 };
        // Opus 4.5: 5 + 25 + 0.50 + 6.25
        assert!((cost("claude-opus-4-5-20251101", &t, 0, false).unwrap() - 36.75).abs() < 1e-9);
        // An hour-long cache write costs twice the input price: 5 + 25 + 0.50 + 10
        assert!((cost("claude-opus-4-5", &t, 1_000_000, false).unwrap() - 40.50).abs() < 1e-9);
        // gpt-5: 1.25 + 10 + 0.125 + 1.25 (a write is plain input)
        assert!((cost("gpt-5-codex", &t, 0, true).unwrap() - 12.625).abs() < 1e-9);
        // A long prompt in one request is priced at the higher rate; a sum never is.
        let long = Tokens { input: 300_000, output: 0, cache_read: 0, cache_write: 0 };
        assert!((cost("gpt-5.4", &long, 0, true).unwrap() - 1.5).abs() < 1e-9);
        assert!((cost("gpt-5.4", &long, 0, false).unwrap() - 0.75).abs() < 1e-9);
        assert!(cost("gpt-5.9", &t, 0, true).is_none());
        assert_eq!(cost("gpt-5", &Tokens::default(), 0, true), Some(0.0));
    }
}
