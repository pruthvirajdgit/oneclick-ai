//! Cron expression normalization utilities.
//!
//! The [`cron`] crate expects 7-field expressions
//! (`sec min hour dom month dow year`), but users typically supply the
//! standard 5-field format (`min hour dom month dow`).
//!
//! This module bridges the two by normalising user input before parsing.

use chrono::{DateTime, Utc};
use cron::Schedule;
use std::str::FromStr;

/// Convert a user-facing cron expression into the 7-field format expected by
/// the `cron` crate.
///
/// - 5 fields → prepend `0` (seconds) and append `*` (year)
/// - 6 fields → assume seconds are included, append `*` (year)
/// - 7 fields → pass through unchanged
pub fn normalise_cron(expr: &str) -> Result<String, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {} *", expr)),
        6 => Ok(format!("{} *", expr)),
        7 => Ok(expr.to_string()),
        n => Err(format!(
            "Invalid cron expression: expected 5, 6, or 7 fields, got {n}"
        )),
    }
}

/// Parse a cron expression and return the next upcoming run time (UTC).
///
/// Accepts 5-, 6-, or 7-field expressions; conversion is handled
/// automatically via [`normalise_cron`].
pub fn next_run_at(cron_expr: &str) -> Result<DateTime<Utc>, String> {
    let normalised = normalise_cron(cron_expr)?;
    let schedule = Schedule::from_str(&normalised)
        .map_err(|e| format!("Invalid cron expression: {e}"))?;
    schedule
        .upcoming(Utc)
        .next()
        .ok_or_else(|| format!("No upcoming time for cron expression: {cron_expr}"))
}
