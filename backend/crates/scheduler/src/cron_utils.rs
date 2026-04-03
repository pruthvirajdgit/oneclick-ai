//! Cron expression helpers.
//!
//! The [`cron`] crate expects 7-field expressions
//! (`sec min hour dom month dow year`), but users typically supply the
//! standard 5-field format (`min hour dom month dow`).
//!
//! This module bridges the two by normalising user input before parsing.

use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;

/// Convert a user-facing 5-field cron expression into the 7-field format
/// expected by the `cron` crate.
///
/// - 5 fields → prepend `0` (seconds) and append `*` (year)
/// - 6 fields → assume seconds are included, append `*` (year)
/// - 7 fields → pass through unchanged
///
/// Returns an error if the field count is unexpected.
pub fn normalise_cron(expr: &str) -> anyhow::Result<String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {} *", expr)),
        6 => Ok(format!("{} *", expr)),
        7 => Ok(expr.to_string()),
        n => anyhow::bail!(
            "Invalid cron expression: expected 5, 6, or 7 fields, got {n}"
        ),
    }
}

/// Parse a cron expression and return the next upcoming run time (UTC).
///
/// Accepts 5-, 6-, or 7-field expressions; conversion is handled
/// automatically via [`normalise_cron`].
pub fn next_run_at(cron_expr: &str) -> anyhow::Result<DateTime<Utc>> {
    let normalised = normalise_cron(cron_expr)?;
    let schedule = Schedule::from_str(&normalised)?;
    schedule
        .upcoming(Utc)
        .next()
        .ok_or_else(|| anyhow::anyhow!("No upcoming time for cron expression: {cron_expr}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_field_normalises_to_seven() {
        let out = normalise_cron("*/5 * * * *").unwrap();
        assert_eq!(out, "0 */5 * * * * *");
    }

    #[test]
    fn six_field_normalises_to_seven() {
        let out = normalise_cron("0 30 * * * *").unwrap();
        assert_eq!(out, "0 30 * * * * *");
    }

    #[test]
    fn seven_field_passes_through() {
        let out = normalise_cron("0 0 12 * * Mon *").unwrap();
        assert_eq!(out, "0 0 12 * * Mon *");
    }

    #[test]
    fn bad_field_count_errors() {
        assert!(normalise_cron("* *").is_err());
    }

    #[test]
    fn next_run_at_returns_future_time() {
        let next = next_run_at("* * * * *").unwrap();
        assert!(next > Utc::now());
    }
}
