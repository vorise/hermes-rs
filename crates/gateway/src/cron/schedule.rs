//! Cron Schedule Parser
//!
//! Parses cron expressions compatible with standard 5-field format.

use std::time::{Duration, SystemTime};
use thiserror::Error;
use chrono::{Datelike, Timelike};

/// Schedule parsing error.
#[derive(Debug, Error)]
pub enum ScheduleError {
    /// Invalid cron expression.
    #[error("Invalid cron expression: {0}")]
    InvalidExpression(String),

    /// Invalid field value.
    #[error("Invalid field value in {field}: {value}")]
    InvalidField {
        field: String,
        value: String,
    },

    /// Unsupported feature.
    #[error("Unsupported cron feature: {0}")]
    Unsupported(String),
}

/// Cron schedule representation.
#[derive(Debug, Clone)]
pub struct CronSchedule {
    /// Minute field (0-59).
    minutes: Vec<u8>,

    /// Hour field (0-23).
    hours: Vec<u8>,

    /// Day of month field (1-31).
    days_of_month: Vec<u8>,

    /// Month field (1-12).
    months: Vec<u8>,

    /// Day of week field (0-6, 0=Sunday).
    days_of_week: Vec<u8>,
}

impl CronSchedule {
    /// Parse a cron expression.
    ///
    /// Standard 5-field format: minute hour day-of-month month day-of-week
    ///
    /// Examples:
    /// - `"* * * * *"` - every minute
    /// - `"0 * * * *"` - every hour
    /// - `"0 9 * * *"` - every day at 9am
    /// - `"0 9 * * 1-5"` - weekdays at 9am
    /// - `"*/5 * * * *"` - every 5 minutes
    /// - `"0 0 1 * *"` - first day of month at midnight
    pub fn parse(expression: &str) -> Result<Self, ScheduleError> {
        let parts: Vec<&str> = expression.trim().split_whitespace().collect();

        if parts.len() != 5 {
            return Err(ScheduleError::InvalidExpression(
                format!("Expected 5 fields, got {}", parts.len())
            ));
        }

        let minutes = parse_field(parts[0], "minute", 0, 59)?;
        let hours = parse_field(parts[1], "hour", 0, 23)?;
        let days_of_month = parse_field(parts[2], "day-of-month", 1, 31)?;
        let months = parse_field(parts[3], "month", 1, 12)?;
        let days_of_week = parse_field(parts[4], "day-of-week", 0, 6)?;

        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
        })
    }

    /// Create a schedule that runs every minute.
    pub fn every_minute() -> Self {
        Self::parse("* * * * *").unwrap()
    }

    /// Create a schedule that runs every hour.
    pub fn every_hour() -> Self {
        Self::parse("0 * * * *").unwrap()
    }

    /// Create a schedule that runs daily at a specific hour.
    pub fn daily_at(hour: u8) -> Result<Self, ScheduleError> {
        if hour > 23 {
            return Err(ScheduleError::InvalidField {
                field: "hour".to_string(),
                value: hour.to_string(),
            });
        }
        Self::parse(&format!("0 {} * * *", hour))
    }

    /// Create a schedule that runs weekdays at a specific hour.
    pub fn weekdays_at(hour: u8) -> Result<Self, ScheduleError> {
        if hour > 23 {
            return Err(ScheduleError::InvalidField {
                field: "hour".to_string(),
                value: hour.to_string(),
            });
        }
        Self::parse(&format!("0 {} * * 1-5", hour))
    }

    /// Create a schedule from natural language description.
    ///
    /// Examples:
    /// - `"every minute"`
    /// - `"every hour"`
    /// - `"every day at 9am"`
    /// - `"weekdays at 9am"`
    /// - `"every monday at 10am"`
    /// - `"every 5 minutes"`
    pub fn from_natural(description: &str) -> Result<Self, ScheduleError> {
        let lower = description.to_lowercase();
        let desc = lower.trim();

        // Simple pattern matching
        if desc == "every minute" {
            return Ok(Self::every_minute());
        }

        if desc == "every hour" {
            return Ok(Self::every_hour());
        }

        // Match "every N minutes"
        if let Some(n) = parse_natural_interval(desc, "minutes") {
            if n > 0 && n <= 59 {
                return Self::parse(&format!("*/{} * * * *", n));
            }
        }

        // Match "every N hours"
        if let Some(n) = parse_natural_interval(desc, "hours") {
            if n > 0 && n <= 23 {
                return Self::parse(&format!("0 */{} * * *", n));
            }
        }

        // Match "every day at Xam/pm"
        if let Some(hour) = parse_natural_time(desc, "every day at") {
            return Self::daily_at(hour);
        }

        // Match "weekdays at Xam/pm"
        if let Some(hour) = parse_natural_time(desc, "weekdays at") {
            return Self::weekdays_at(hour);
        }

        // Match "every monday/tuesday... at Xam/pm"
        if let Some((day, hour)) = parse_natural_day_time(desc) {
            return Self::parse(&format!("0 {} * * {}", hour, day));
        }

        Err(ScheduleError::InvalidExpression(
            format!("Cannot parse natural language schedule: {}", description)
        ))
    }

    /// Get next execution time after a given timestamp.
    pub fn next_after(&self, after: SystemTime) -> SystemTime {
        let after_secs = after
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Convert to datetime components
        let after_dt = chrono::DateTime::from_timestamp(after_secs as i64, 0)
            .unwrap_or_else(|| chrono::Utc::now());

        // Find next matching time (start from next minute)
        let mut candidate = after_dt + chrono::Duration::minutes(1);
        candidate = candidate.with_second(0).unwrap_or(candidate);

        // Search for next match (limit iterations to prevent infinite loop)
        for _ in 0..366 * 24 * 60 {  // Max 1 year of search
            if self.matches_datetime(&candidate) {
                return candidate.into();
            }
            candidate = candidate + chrono::Duration::minutes(1);
        }

        // Fallback: return far future
        SystemTime::UNIX_EPOCH + Duration::from_secs(365 * 24 * 60 * 60)
    }

    /// Check if a datetime matches the schedule.
    fn matches_datetime(&self, dt: &chrono::DateTime<chrono::Utc>) -> bool {
        let minute = dt.minute() as u8;
        let hour = dt.hour() as u8;
        let day = dt.day() as u8;
        let month = dt.month() as u8;
        let weekday = dt.weekday().num_days_from_sunday() as u8;

        self.minutes.contains(&minute)
            && self.hours.contains(&hour)
            && (self.days_of_month.contains(&day) || self.days_of_month.contains(&weekday))
            && self.months.contains(&month)
    }

    /// Get the cron expression string.
    pub fn expression(&self) -> String {
        format!(
            "{} {} {} {} {}",
            field_to_string(&self.minutes),
            field_to_string(&self.hours),
            field_to_string(&self.days_of_month),
            field_to_string(&self.months),
            field_to_string(&self.days_of_week)
        )
    }
}

/// Parse a single cron field.
fn parse_field(field: &str, name: &str, min: u8, max: u8) -> Result<Vec<u8>, ScheduleError> {
    if field == "*" {
        return Ok((min..=max).collect());
    }

    // Handle step values (*/N)
    if field.starts_with("*/") {
        let step: u8 = field[2..].parse().map_err(|_| ScheduleError::InvalidField {
            field: name.to_string(),
            value: field.to_string(),
        })?;
        if step == 0 {
            return Err(ScheduleError::InvalidField {
                field: name.to_string(),
                value: field.to_string(),
            });
        }
        return Ok((min..=max).step_by(step as usize).collect());
    }

    // Handle ranges (N-M)
    if field.contains('-') {
        let parts: Vec<&str> = field.split('-').collect();
        if parts.len() != 2 {
            return Err(ScheduleError::InvalidField {
                field: name.to_string(),
                value: field.to_string(),
            });
        }
        let start: u8 = parts[0].parse().map_err(|_| ScheduleError::InvalidField {
            field: name.to_string(),
            value: field.to_string(),
        })?;
        let end: u8 = parts[1].parse().map_err(|_| ScheduleError::InvalidField {
            field: name.to_string(),
            value: field.to_string(),
        })?;
        if start < min || end > max || start > end {
            return Err(ScheduleError::InvalidField {
                field: name.to_string(),
                value: field.to_string(),
            });
        }
        return Ok((start..=end).collect());
    }

    // Handle lists (N,M,O)
    if field.contains(',') {
        let values: Vec<u8> = field.split(',')
            .map(|v| v.parse::<u8>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ScheduleError::InvalidField {
                field: name.to_string(),
                value: field.to_string(),
            })?;
        for v in &values {
            if *v < min || *v > max {
                return Err(ScheduleError::InvalidField {
                    field: name.to_string(),
                    value: field.to_string(),
                });
            }
        }
        return Ok(values);
    }

    // Single value
    let value: u8 = field.parse().map_err(|_| ScheduleError::InvalidField {
        field: name.to_string(),
        value: field.to_string(),
    })?;
    if value < min || value > max {
        return Err(ScheduleError::InvalidField {
            field: name.to_string(),
            value: field.to_string(),
        });
    }
    Ok(vec![value])
}

/// Convert field values back to string representation.
fn field_to_string(values: &[u8]) -> String {
    if values.len() == 1 {
        return values[0].to_string();
    }

    // Check if all values in range
    let min = *values.first().unwrap();
    let max = *values.last().unwrap();
    let expected_count = (max - min + 1) as usize;

    if values.len() == expected_count {
        // All values in range - check if it's a full range
        // For different fields, full range means different things:
        // minutes: 0-59 = "*"
        // hours: 0-23 = "*"
        // days of month: 1-31 = "*"
        // months: 1-12 = "*"
        // days of week: 0-6 = "*"
        if min == 0 && max == 59 || min == 0 && max == 23 || min == 1 && max == 31
            || min == 1 && max == 12 || min == 0 && max == 6 {
            return "*".to_string();
        }
        // Range
        return format!("{}-{}", min, max);
    }

    // List
    values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
}

/// Parse natural language interval (e.g., "every 5 minutes").
fn parse_natural_interval(desc: &str, unit: &str) -> Option<u8> {
    // Pattern: "every N unit" (e.g., "every 5 minutes")
    let parts: Vec<&str> = desc.split_whitespace().collect();
    if parts.len() >= 3 && parts[0] == "every" {
        // Try to parse the second part as a number
        if let Ok(n) = parts[1].parse::<u8>() {
            // Check if the third part matches the unit
            if parts[2].starts_with(unit) {
                return Some(n);
            }
        }
    }
    None
}

/// Parse natural language time (e.g., "at 9am").
fn parse_natural_time(desc: &str, prefix: &str) -> Option<u8> {
    if !desc.starts_with(prefix) {
        return None;
    }
    let rest = &desc[prefix.len()..];
    let rest = rest.trim();

    // Parse "9am" or "9 pm" format
    if rest.ends_with("am") {
        let replaced = rest.replace("am", "");
        let num = replaced.trim();
        return num.parse::<u8>().ok();
    }
    if rest.ends_with("pm") {
        let replaced = rest.replace("pm", "");
        let num = replaced.trim();
        let hour: u8 = num.parse().ok()?;
        return Some(if hour == 12 { 12 } else { hour + 12 });
    }
    None
}

/// Parse natural language day and time (e.g., "every monday at 10am").
fn parse_natural_day_time(desc: &str) -> Option<(u8, u8)> {
    let days = [
        ("sunday", 0), ("monday", 1), ("tuesday", 2),
        ("wednesday", 3), ("thursday", 4), ("friday", 5), ("saturday", 6),
    ];

    for (day_name, day_num) in days {
        if desc.starts_with(&format!("every {}", day_name)) {
            let rest = &desc[format!("every {}", day_name).len()..];
            if rest.starts_with(" at") {
                let time_part = rest.trim_start_matches(" at").trim();
                if time_part.ends_with("am") {
                    let replaced = time_part.replace("am", "");
                    let num = replaced.trim();
                    if let Ok(h) = num.parse::<u8>() {
                        return Some((day_num, h));
                    }
                }
                if time_part.ends_with("pm") {
                    let replaced = time_part.replace("pm", "");
                    let num = replaced.trim();
                    if let Ok(h) = num.parse::<u8>() {
                        let hour = if h == 12 { 12 } else { h + 12 };
                        return Some((day_num, hour));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let schedule = CronSchedule::parse("* * * * *").unwrap();
        assert_eq!(schedule.minutes.len(), 60);
        assert_eq!(schedule.hours.len(), 24);
    }

    #[test]
    fn test_parse_specific_time() {
        let schedule = CronSchedule::parse("0 9 * * *").unwrap();
        assert_eq!(schedule.minutes, vec![0]);
        assert_eq!(schedule.hours, vec![9]);
    }

    #[test]
    fn test_parse_range() {
        let schedule = CronSchedule::parse("0 9 * * 1-5").unwrap();
        assert_eq!(schedule.days_of_week, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_parse_step() {
        let schedule = CronSchedule::parse("*/5 * * * *").unwrap();
        assert_eq!(schedule.minutes, vec![0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55]);
    }

    #[test]
    fn test_parse_list() {
        let schedule = CronSchedule::parse("0,30 9,17 * * *").unwrap();
        assert_eq!(schedule.minutes, vec![0, 30]);
        assert_eq!(schedule.hours, vec![9, 17]);
    }

    #[test]
    fn test_parse_invalid_fields() {
        assert!(CronSchedule::parse("* * * *").is_err());  // Too few fields
        assert!(CronSchedule::parse("60 * * * *").is_err());  // Invalid minute
        assert!(CronSchedule::parse("* 24 * * *").is_err());  // Invalid hour
    }

    #[test]
    fn test_daily_at() {
        let schedule = CronSchedule::daily_at(9).unwrap();
        assert_eq!(schedule.hours, vec![9]);
    }

    #[test]
    fn test_weekdays_at() {
        let schedule = CronSchedule::weekdays_at(9).unwrap();
        assert_eq!(schedule.days_of_week, vec![1, 2, 3, 4, 5]);
        assert_eq!(schedule.hours, vec![9]);
    }

    #[test]
    fn test_natural_every_minute() {
        let schedule = CronSchedule::from_natural("every minute").unwrap();
        assert_eq!(schedule.expression(), "* * * * *");
    }

    #[test]
    fn test_natural_every_hour() {
        let schedule = CronSchedule::from_natural("every hour").unwrap();
        // "every hour" means run at minute 0 of every hour
        assert_eq!(schedule.minutes, vec![0]);
        assert_eq!(schedule.hours.len(), 24);
    }

    #[test]
    fn test_natural_every_day() {
        let schedule = CronSchedule::from_natural("every day at 9am").unwrap();
        assert_eq!(schedule.hours, vec![9]);
    }

    #[test]
    fn test_natural_weekdays() {
        let schedule = CronSchedule::from_natural("weekdays at 9am").unwrap();
        assert_eq!(schedule.days_of_week, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_natural_every_5_minutes() {
        let schedule = CronSchedule::from_natural("every 5 minutes").unwrap();
        assert_eq!(schedule.minutes.len(), 12);
    }

    #[test]
    fn test_next_after() {
        let schedule = CronSchedule::parse("0 9 * * *").unwrap();
        let now = SystemTime::now();
        let next = schedule.next_after(now);

        // Should be at 9:00
        let next_dt = chrono::DateTime::from_timestamp(
            next.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i64,
            0
        ).unwrap();
        assert_eq!(next_dt.minute(), 0);
        assert_eq!(next_dt.hour(), 9);
    }

    #[test]
    fn test_expression() {
        let schedule = CronSchedule::parse("0 9 * * 1-5").unwrap();
        assert_eq!(schedule.expression(), "0 9 * * 1-5");
    }
}