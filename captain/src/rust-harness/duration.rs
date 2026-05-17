/// Parses a human‑readable duration string into seconds.
///
/// Accepts plain seconds (`"3600"`) or a number followed by a unit:
/// * `s` – seconds
/// * `m` – minutes (multiplied by 60)
/// * `h` – hours (multiplied by 3600)
///
/// Returns an error if the string is empty, the number is zero, or the unit is
/// not one of the supported suffixes.
pub fn parse_duration_seconds(input: &str) -> std::result::Result<u64, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    if let Ok(seconds) = input.parse::<u64>() {
        if seconds == 0 {
            return Err("duration must be > 0".to_string());
        }
        return Ok(seconds);
    }

    let split_at = input
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(idx, _)| idx)
        .ok_or_else(|| format!("invalid duration: {input}"))?;

    let (num_part, unit_part) = input.split_at(split_at);
    if num_part.is_empty() {
        return Err(format!("invalid duration: {input}"));
    }

    let quantity = num_part
        .parse::<u64>()
        .map_err(|_| format!("invalid duration number: {num_part}"))?;

    if quantity == 0 {
        return Err("duration must be > 0".to_string());
    }

    let unit = unit_part.trim().to_ascii_lowercase();
    match unit.as_str() {
        "s" => Ok(quantity),
        "m" => Ok(quantity.saturating_mul(60)),
        "h" => Ok(quantity.saturating_mul(3600)),
        _ => Err(format!(
            "invalid duration unit '{unit_part}' (use seconds or suffix s/m/h)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_supports_seconds_minutes_and_hours() {
        assert_eq!(parse_duration_seconds("3600").unwrap(), 3600);
        assert_eq!(parse_duration_seconds("45s").unwrap(), 45);
        assert_eq!(parse_duration_seconds("5m").unwrap(), 300);
        assert_eq!(parse_duration_seconds("2h").unwrap(), 7200);
    }

    #[test]
    fn parse_duration_rejects_invalid_values() {
        assert!(parse_duration_seconds("0").is_err());
        assert!(parse_duration_seconds("10d").is_err());
        assert!(parse_duration_seconds("abc").is_err());
    }
}
