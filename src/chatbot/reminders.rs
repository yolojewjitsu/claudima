//! Reminder system for scheduled messages.

use chrono::{DateTime, Duration, Utc};
use cron::Schedule;
use std::str::FromStr;

/// A reminder stored in the database.
#[derive(Debug, Clone)]
pub struct Reminder {
    pub id: i64,
    pub chat_id: i64,
    pub user_id: i64,
    pub message: String,
    pub trigger_at: DateTime<Utc>,
    pub repeat_cron: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_triggered_at: Option<DateTime<Utc>>,
    pub active: bool,
}

/// Parse trigger time: "+30m", "+2h", "+1d" or absolute "2026-01-25 15:00"
pub fn parse_trigger_time(input: &str) -> Result<DateTime<Utc>, String> {
    let input = input.trim();

    // Relative time: +30m, +2h, +1d
    if let Some(rest) = input.strip_prefix('+') {
        if rest.len() < 2 {
            return Err(format!("Invalid relative time: '{}'", input));
        }

        // Find where the number ends and unit begins
        let unit_start = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        if unit_start == 0 {
            return Err(format!("Invalid number in '{}'", input));
        }

        let num: i64 = rest[..unit_start]
            .parse()
            .map_err(|_| format!("Invalid number in '{}'", input))?;

        let unit = &rest[unit_start..];
        let duration = match unit {
            "m" | "min" | "mins" | "minute" | "minutes" => Duration::minutes(num),
            "h" | "hr" | "hrs" | "hour" | "hours" => Duration::hours(num),
            "d" | "day" | "days" => Duration::days(num),
            "w" | "week" | "weeks" => Duration::weeks(num),
            _ => return Err(format!("Unknown unit '{}'. Use m/h/d/w", unit)),
        };
        return Ok(Utc::now() + duration);
    }

    // Absolute time: "2026-01-25 15:00"
    DateTime::parse_from_str(&format!("{} +0000", input), "%Y-%m-%d %H:%M %z")
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| format!("Invalid date format: {}. Use YYYY-MM-DD HH:MM", e))
}

/// Validate cron expression.
pub fn validate_cron(expr: &str) -> Result<(), String> {
    Schedule::from_str(expr)
        .map(|_| ())
        .map_err(|e| format!("Invalid cron: {}", e))
}

/// Get next trigger time from cron expression.
pub fn next_cron_trigger(expr: &str, after: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
    let schedule = Schedule::from_str(expr).map_err(|e| format!("Invalid cron: {}", e))?;
    schedule
        .after(&after)
        .next()
        .ok_or_else(|| "No future occurrence for cron".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relative_minutes() {
        let now = Utc::now();
        let result = parse_trigger_time("+30m").unwrap();
        let diff = (result - now).num_minutes();
        assert!((29..=31).contains(&diff));
    }

    #[test]
    fn test_parse_relative_hours() {
        let now = Utc::now();
        let result = parse_trigger_time("+2h").unwrap();
        let diff = (result - now).num_hours();
        assert!((1..=2).contains(&diff));
    }

    #[test]
    fn test_parse_relative_days() {
        let now = Utc::now();
        let result = parse_trigger_time("+1d").unwrap();
        let diff = (result - now).num_days();
        assert!((0..=1).contains(&diff));
    }

    #[test]
    fn test_parse_absolute() {
        let result = parse_trigger_time("2030-06-15 14:30").unwrap();
        assert_eq!(result.format("%Y-%m-%d %H:%M").to_string(), "2030-06-15 14:30");
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_trigger_time("invalid").is_err());
        assert!(parse_trigger_time("+").is_err());
        assert!(parse_trigger_time("+30x").is_err());
    }

    #[test]
    fn test_validate_cron() {
        // cron crate uses 7-field format: sec min hour day month dow year
        assert!(validate_cron("0 0 9 * * * *").is_ok()); // Daily at 9am
        assert!(validate_cron("0 0 0 * * 1 *").is_ok()); // Mondays at midnight
        assert!(validate_cron("0 */5 * * * * *").is_ok()); // Every 5 minutes
        assert!(validate_cron("invalid").is_err());
    }

    #[test]
    fn test_next_cron_trigger() {
        let now = Utc::now();
        // cron crate uses 7-field format: sec min hour day month dow year
        let next = next_cron_trigger("0 0 * * * * *", now).unwrap(); // Every hour
        assert!(next > now);
    }
}
