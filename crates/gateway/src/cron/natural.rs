use regex::Regex;
use std::sync::OnceLock;

/// Parse a natural language scheduling phrase into a cron expression.
///
/// Supported patterns:
/// - "every day at 9am" -> "0 9 * * *"
/// - "every day at 9:30" -> "0 9 * * *"
/// - "every hour" -> "0 * * * *"
/// - "every 5 minutes" -> "*/5 * * * *"
/// - "every 15 minutes" -> "*/15 * * * *"
/// - "every monday at 9am" -> "0 9 * * 1"
/// - "every tuesday at 9am" -> "0 9 * * 2"
/// - "every weekday at 9am" -> "0 9 * * 1-5"
/// - "every weekend at 9am" -> "0 9 * * 6,0"
/// - "every monday" -> "0 0 * * 1"
/// - "weekly" -> "0 0 * * 1"
/// - "daily" -> "0 0 * * *"
/// - "hourly" -> "0 * * * *"
pub fn parse_natural(input: &str) -> Result<String, String> {
    let lower = input.trim().to_lowercase();

    // Simple keywords
    match lower.as_str() {
        "hourly" => return Ok("0 * * * *".to_string()),
        "daily" | "every day" => return Ok("0 0 * * *".to_string()),
        "weekly" => return Ok("0 0 * * 1".to_string()),
        "monthly" => return Ok("0 0 1 * *".to_string()),
        _ => {}
    }

    // "every N minutes"
    if let Some(caps) = re_every_n_minutes().captures(&lower) {
        let n: u8 = caps.get(1).unwrap().as_str().parse().map_err(|_| "Invalid number")?;
        if n < 1 || n > 59 {
            return Err("Minutes must be between 1 and 59".to_string());
        }
        return Ok(format!("*/{n} * * * *"));
    }

    // "every N hours"
    if let Some(caps) = re_every_n_hours().captures(&lower) {
        let n: u8 = caps.get(1).unwrap().as_str().parse().map_err(|_| "Invalid number")?;
        if n < 1 || n > 23 {
            return Err("Hours must be between 1 and 23".to_string());
        }
        return Ok(format!("0 */{n} * * *"));
    }

    // "every weekday at H:MM" or "every weekday at Ham/pm"
    if let Some(caps) = re_weekday_at().captures(&lower) {
        let time = parse_time_from_caps(&caps)?;
        return Ok(format!("{time} * * 1-5"));
    }

    // "every weekend at H:MM"
    if let Some(caps) = re_weekend_at().captures(&lower) {
        let time = parse_time_from_caps(&caps)?;
        return Ok(format!("{time} * * 6,0"));
    }

    // "every day at H:MM" or "every day at Ham/pm" - check BEFORE generic "every DAY at"
    if let Some(caps) = re_every_day_at().captures(&lower) {
        let time = parse_time_from_caps(&caps)?;
        return Ok(format!("{time} * * *"));
    }

    // "every DAY at H:MM" or "every DAY at Ham/pm"
    if let Some(caps) = re_day_at().captures(&lower) {
        let day = parse_day(caps.get(1).unwrap().as_str())?;
        let time = parse_time_from_caps_with_day(&caps)?;
        return Ok(format!("{time} * * {day}"));
    }

    // "every DAY" (midnight)
    if let Some(caps) = re_just_day().captures(&lower) {
        let day = parse_day(caps.get(1).unwrap().as_str())?;
        return Ok(format!("0 0 * * {day}"));
    }

    Err(format!("Could not parse scheduling phrase: {input}"))
}

fn parse_time_from_caps(caps: &regex::Captures) -> Result<String, String> {
    // For patterns WITHOUT day name: group 1=hour, group 2=minute, group 3=am/pm
    let hour_str = caps.get(1).unwrap().as_str();
    let min = caps.get(2).map(|m| m.as_str());
    let ampm = caps.get(3).map(|m| m.as_str());
    let (hour, minute) = parse_time(hour_str, min, ampm)?;
    Ok(format!("{minute} {hour}"))
}

fn parse_time_from_caps_with_day(caps: &regex::Captures) -> Result<String, String> {
    // For patterns WITH day name: group 2=hour, group 3=minute, group 4=am/pm
    let hour_str = caps.get(2).unwrap().as_str();
    let min = caps.get(3).map(|m| m.as_str());
    let ampm = caps.get(4).map(|m| m.as_str());
    let (hour, minute) = parse_time(hour_str, min, ampm)?;
    Ok(format!("{minute} {hour}"))
}

fn parse_time(hour_str: &str, min: Option<&str>, ampm: Option<&str>) -> Result<(u8, u8), String> {
    let mut hour: u8 = hour_str.parse().map_err(|_| "Invalid hour")?;

    if let Some(ampm) = ampm {
        match ampm {
            "am" if hour == 12 => hour = 0,
            "pm" if hour != 12 => hour += 12,
            _ => {}
        }
    }

    if hour > 23 {
        return Err("Hour must be between 0 and 23".to_string());
    }

    let minute = if let Some(m) = min {
        let m: u8 = m.parse().map_err(|_| "Invalid minute")?;
        if m > 59 {
            return Err("Minute must be between 0 and 59".to_string());
        }
        m
    } else {
        0
    };

    Ok((hour, minute))
}

fn parse_day(day: &str) -> Result<u8, String> {
    match day {
        "sunday" | "sun" => Ok(0),
        "monday" | "mon" => Ok(1),
        "tuesday" | "tue" | "tues" => Ok(2),
        "wednesday" | "wed" => Ok(3),
        "thursday" | "thu" | "thur" | "thurs" => Ok(4),
        "friday" | "fri" => Ok(5),
        "saturday" | "sat" => Ok(6),
        _ => Err(format!("Unknown day: {day}")),
    }
}

// Lazy regex patterns
fn re_every_n_minutes() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"every\s+(\d+)\s+minute").unwrap())
}

fn re_every_n_hours() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"every\s+(\d+)\s+hour").unwrap())
}

fn re_weekday_at() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"every\s+weekday\s+at\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?").unwrap())
}

fn re_weekend_at() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"every\s+weekend\s+at\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?").unwrap())
}

fn re_day_at() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"every\s+(\w+)\s+at\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?").unwrap())
}

fn re_just_day() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"every\s+(\w+)$").unwrap())
}

fn re_every_day_at() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"every\s+day\s+at\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)?").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keywords() {
        assert_eq!(parse_natural("hourly").unwrap(), "0 * * * *");
        assert_eq!(parse_natural("daily").unwrap(), "0 0 * * *");
        assert_eq!(parse_natural("weekly").unwrap(), "0 0 * * 1");
        assert_eq!(parse_natural("monthly").unwrap(), "0 0 1 * *");
    }

    #[test]
    fn test_every_n_minutes() {
        assert_eq!(parse_natural("every 5 minutes").unwrap(), "*/5 * * * *");
        assert_eq!(parse_natural("every 15 minutes").unwrap(), "*/15 * * * *");
        assert_eq!(parse_natural("Every 30 Minutes").unwrap(), "*/30 * * * *");
    }

    #[test]
    fn test_every_n_hours() {
        assert_eq!(parse_natural("every 2 hours").unwrap(), "0 */2 * * *");
        assert_eq!(parse_natural("every 6 hours").unwrap(), "0 */6 * * *");
    }

    #[test]
    fn test_every_day_at_time() {
        assert_eq!(parse_natural("every day at 9am").unwrap(), "0 9 * * *");
        assert_eq!(parse_natural("every day at 2:30pm").unwrap(), "30 14 * * *");
        assert_eq!(parse_natural("Every Day at 12am").unwrap(), "0 0 * * *");
    }

    #[test]
    fn test_every_day_of_week() {
        assert_eq!(parse_natural("every monday at 9am").unwrap(), "0 9 * * 1");
        assert_eq!(parse_natural("every friday at 5pm").unwrap(), "0 17 * * 5");
    }

    #[test]
    fn test_weekday_weekend() {
        assert_eq!(parse_natural("every weekday at 9am").unwrap(), "0 9 * * 1-5");
        assert_eq!(parse_natural("every weekend at 10am").unwrap(), "0 10 * * 6,0");
    }

    #[test]
    fn test_invalid_input() {
        assert!(parse_natural("once a year").is_err());
        assert!(parse_natural("tomorrow").is_err());
    }

    #[test]
    fn test_invalid_minutes() {
        assert!(parse_natural("every 0 minutes").is_err());
        assert!(parse_natural("every 60 minutes").is_err());
    }
}
