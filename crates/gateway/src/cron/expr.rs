use chrono::{Datelike, Local, Timelike};

/// A parsed cron expression.
///
/// Supports standard 5-field cron: minute hour day-of-month month day-of-week.
#[derive(Debug, Clone, Default)]
pub struct CronExpr {
    pub minutes: Vec<u8>,
    pub hours: Vec<u8>,
    pub days_of_month: Vec<u8>,
    pub months: Vec<u8>,
    pub days_of_week: Vec<u8>,
}

impl CronExpr {
    /// Parse a cron expression string into a CronExpr.
    ///
    /// Supports: `*`, ranges (`1-5`), steps (`*/5`, `1-10/2`), lists (`1,3,5`).
    pub fn parse(expr: &str) -> Result<Self, String> {
        let fields: Vec<&str> = expr.trim().split_whitespace().collect();
        if fields.len() != 5 {
            return Err(format!("Expected 5 fields, got {}", fields.len()));
        }

        let minutes = parse_field(fields[0], 0, 59)?;
        let hours = parse_field(fields[1], 0, 23)?;
        let days_of_month = parse_field(fields[2], 1, 31)?;
        let months = parse_field(fields[3], 1, 12)?;
        let days_of_week = parse_field(fields[4], 0, 6)?; // 0=Sunday

        Ok(Self {
            minutes,
            hours,
            days_of_month,
            months,
            days_of_week,
        })
    }

    /// Check if the expression matches the given datetime.
    pub fn matches(&self, dt: &chrono::DateTime<Local>) -> bool {
        let minute = dt.minute() as u8;
        let hour = dt.hour() as u8;
        let day = dt.day() as u8;
        let month = dt.month() as u8;
        let dow = ((dt.weekday().number_from_sunday() as u8) + 6) % 7; // 0=Sunday, 1=Monday, ...

        self.minutes.contains(&minute)
            && self.hours.contains(&hour)
            && self.days_of_month.contains(&day)
            && self.months.contains(&month)
            && self.days_of_week.contains(&dow)
    }
}

/// Parse a single cron field into a list of matching values.
fn parse_field(field: &str, min: u8, max: u8) -> Result<Vec<u8>, String> {
    let mut values = Vec::new();

    for part in field.split(',') {
        if part.contains('/') {
            // Step: */5, 1-10/2
            let parts: Vec<&str> = part.split('/').collect();
            if parts.len() != 2 {
                return Err(format!("Invalid step expression: {part}"));
            }
            let step: u8 = parts[1]
                .parse()
                .map_err(|_| format!("Invalid step value in: {part}"))?;
            if step == 0 {
                return Err(format!("Step cannot be zero: {part}"));
            }

            let (range_start, range_end): (u8, u8) = if parts[0] == "*" {
                (min, max)
            } else if parts[0].contains('-') {
                // Range with step: 1-10/2
                let bounds: Vec<&str> = parts[0].split('-').collect();
                if bounds.len() != 2 {
                    return Err(format!("Invalid range: {part}"));
                }
                let start: u8 = bounds[0].parse().map_err(|_| format!("Invalid range start: {part}"))?;
                let end: u8 = bounds[1].parse().map_err(|_| format!("Invalid range end: {part}"))?;
                (start, end)
            } else {
                let start: u8 = parts[0].parse().map_err(|_| format!("Invalid range start: {part}"))?;
                (start, max)
            };

            let mut v = range_start;
            while v <= range_end {
                values.push(v);
                v += step;
            }
        } else if part == "*" {
            values.extend(min..=max);
        } else if part.contains('-') {
            // Range: 1-5
            let bounds: Vec<&str> = part.split('-').collect();
            if bounds.len() != 2 {
                return Err(format!("Invalid range expression: {part}"));
            }
            let start: u8 = bounds[0]
                .parse()
                .map_err(|_| format!("Invalid range start: {part}"))?;
            let end: u8 = bounds[1]
                .parse()
                .map_err(|_| format!("Invalid range end: {part}"))?;
            if start > end {
                return Err(format!("Range start > end: {part}"));
            }
            values.extend(start..=end);
        } else {
            // Single value
            let val: u8 = part
                .parse()
                .map_err(|_| format!("Invalid value: {part}"))?;
            values.push(val);
        }
    }

    values.sort();
    values.dedup();
    Ok(values)
}

/// Calculate the next datetime that matches the cron expression after the given time.
///
/// Returns the next matching time, or None if none found within 1 year.
pub fn next_after(expr: &CronExpr, after: chrono::DateTime<Local>) -> Option<chrono::DateTime<Local>> {
    let mut candidate = after + chrono::Duration::minutes(1);
    candidate = candidate.with_second(0)?.with_nanosecond(0)?;

    // Search for up to 1 year
    let max_iterations = 366 * 24 * 60; // minutes in a year
    for _ in 0..max_iterations {
        if expr.matches(&candidate) {
            return Some(candidate);
        }
        candidate = candidate + chrono::Duration::minutes(1);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_parse_every_minute() {
        let expr = CronExpr::parse("* * * * *").unwrap();
        assert_eq!(expr.minutes.len(), 60);
        assert_eq!(expr.hours.len(), 24);
    }

    #[test]
    fn test_parse_specific_values() {
        let expr = CronExpr::parse("0 9 * * *").unwrap();
        assert_eq!(expr.minutes, vec![0]);
        assert_eq!(expr.hours, vec![9]);
    }

    #[test]
    fn test_parse_range() {
        let expr = CronExpr::parse("0 9-17 * * *").unwrap();
        assert_eq!(expr.hours, vec![9, 10, 11, 12, 13, 14, 15, 16, 17]);
    }

    #[test]
    fn test_parse_step() {
        let expr = CronExpr::parse("*/15 * * * *").unwrap();
        assert_eq!(expr.minutes, vec![0, 15, 30, 45]);
    }

    #[test]
    fn test_parse_step_with_range() {
        let expr = CronExpr::parse("1-10/2 * * * *").unwrap();
        assert_eq!(expr.minutes, vec![1, 3, 5, 7, 9]);
    }

    #[test]
    fn test_parse_list() {
        let expr = CronExpr::parse("0,15,30,45 * * * *").unwrap();
        assert_eq!(expr.minutes, vec![0, 15, 30, 45]);
    }

    #[test]
    fn test_parse_invalid_field_count() {
        assert!(CronExpr::parse("* * *").is_err());
        assert!(CronExpr::parse("").is_err());
    }

    #[test]
    fn test_parse_invalid_value() {
        assert!(CronExpr::parse("abc * * * *").is_err());
    }

    #[test]
    fn test_parse_step_zero() {
        assert!(CronExpr::parse("*/0 * * * *").is_err());
    }

    #[test]
    fn test_matches() {
        let expr = CronExpr::parse("30 9 15 6 *").unwrap();
        // June 15, 2026 at 09:30 - Monday (dow=1)
        let dt = Local.with_ymd_and_hms(2026, 6, 15, 9, 30, 0).unwrap();
        assert!(expr.matches(&dt));

        // Wrong minute
        let dt = Local.with_ymd_and_hms(2026, 6, 15, 9, 31, 0).unwrap();
        assert!(!expr.matches(&dt));

        // Wrong month
        let dt = Local.with_ymd_and_hms(2026, 7, 15, 9, 30, 0).unwrap();
        assert!(!expr.matches(&dt));
    }

    #[test]
    fn test_matches_day_of_week() {
        let expr = CronExpr::parse("0 0 * * 1").unwrap(); // Monday midnight
        // January 5, 2026 is a Monday
        let dt = Local.with_ymd_and_hms(2026, 1, 5, 0, 0, 0).unwrap();
        assert!(expr.matches(&dt));

        // January 6, 2026 is a Tuesday
        let dt = Local.with_ymd_and_hms(2026, 1, 6, 0, 0, 0).unwrap();
        assert!(!expr.matches(&dt));
    }

    #[test]
    fn test_next_after() {
        let expr = CronExpr::parse("0 9 * * *").unwrap();
        // Start from Jan 1, 2026 at 08:00
        let start = Local.with_ymd_and_hms(2026, 1, 1, 8, 0, 0).unwrap();
        let next = next_after(&expr, start).unwrap();
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
    }
}
