//! JSON parsing helpers for HTTP/MQTT API.
//!
//! Provides simple, allocation-free JSON parsing for throttle commands.
//! These parsers handle the common request formats without requiring
//! a full JSON parser.
//!
//! # Example
//!
//! ```
//! use rs_trainz::parsing::{parse_speed_json, parse_direction_json};
//! use rs_trainz::Direction;
//!
//! assert_eq!(parse_speed_json(r#"{"speed": 0.5}"#), Some(0.5));
//! assert_eq!(parse_direction_json(r#"{"direction": "forward"}"#), Some(Direction::Forward));
//! ```

use crate::Direction;

/// Parse a speed value from JSON like `{"speed": 0.5}`.
///
/// Returns `None` if the JSON is malformed or the speed value is invalid.
///
/// # Examples
///
/// ```
/// use rs_trainz::parsing::parse_speed_json;
///
/// assert_eq!(parse_speed_json(r#"{"speed": 0.5}"#), Some(0.5));
/// assert_eq!(parse_speed_json(r#"{"speed": 1.0}"#), Some(1.0));
/// assert_eq!(parse_speed_json(r#"{"speed": 0}"#), Some(0.0));
/// assert_eq!(parse_speed_json(r#"{"other": 0.5}"#), None);
/// assert_eq!(parse_speed_json("invalid"), None);
/// ```
pub fn parse_speed_json(json: &str) -> Option<f32> {
    // Find "speed" key
    let speed_start = json.find("\"speed\"")?;
    let colon = json[speed_start..].find(':')?;
    let value_start = speed_start + colon + 1;
    let rest = json[value_start..].trim_start();

    // Find end of number (digits, decimal, negative sign)
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(rest.len());

    rest[..end].parse().ok()
}

/// Parse a max_speed value from JSON like `{"max_speed": 0.8}`.
///
/// Returns `None` if the JSON is malformed or the value is invalid.
///
/// # Examples
///
/// ```
/// use rs_trainz::parsing::parse_max_speed_json;
///
/// assert_eq!(parse_max_speed_json(r#"{"max_speed": 0.8}"#), Some(0.8));
/// assert_eq!(parse_max_speed_json(r#"{"max_speed": 1.0}"#), Some(1.0));
/// assert_eq!(parse_max_speed_json(r#"{"other": 0.5}"#), None);
/// ```
pub fn parse_max_speed_json(json: &str) -> Option<f32> {
    // Find "max_speed" key
    let start = json.find("\"max_speed\"")?;
    let colon = json[start..].find(':')?;
    let value_start = start + colon + 1;
    let rest = json[value_start..].trim_start();

    // Find end of number (digits, decimal, negative sign)
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(rest.len());

    rest[..end].parse().ok()
}

/// Parse a direction from JSON like `{"direction": "forward"}`.
///
/// Returns `None` if the JSON is malformed or direction is invalid.
///
/// # Examples
///
/// ```
/// use rs_trainz::parsing::parse_direction_json;
/// use rs_trainz::Direction;
///
/// assert_eq!(parse_direction_json(r#"{"direction": "forward"}"#), Some(Direction::Forward));
/// assert_eq!(parse_direction_json(r#"{"direction": "reverse"}"#), Some(Direction::Reverse));
/// assert_eq!(parse_direction_json(r#"{"direction": "stopped"}"#), Some(Direction::Stopped));
/// assert_eq!(parse_direction_json(r#"{"other": "forward"}"#), None);
/// ```
pub fn parse_direction_json(json: &str) -> Option<Direction> {
    // Look for "direction" key followed by a value
    if !json.contains("\"direction\"") {
        return None;
    }

    if json.contains("\"forward\"") {
        Some(Direction::Forward)
    } else if json.contains("\"reverse\"") {
        Some(Direction::Reverse)
    } else if json.contains("\"stopped\"") {
        Some(Direction::Stopped)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // parse_speed_json tests
    // =========================================================================

    #[test]
    fn parse_speed_valid_decimal() {
        assert_eq!(parse_speed_json(r#"{"speed": 0.5}"#), Some(0.5));
    }

    #[test]
    fn parse_speed_valid_integer() {
        assert_eq!(parse_speed_json(r#"{"speed": 1}"#), Some(1.0));
    }

    #[test]
    fn parse_speed_valid_zero() {
        assert_eq!(parse_speed_json(r#"{"speed": 0}"#), Some(0.0));
    }

    #[test]
    fn parse_speed_valid_one() {
        assert_eq!(parse_speed_json(r#"{"speed": 1.0}"#), Some(1.0));
    }

    #[test]
    fn parse_speed_valid_small() {
        assert_eq!(parse_speed_json(r#"{"speed": 0.01}"#), Some(0.01));
    }

    #[test]
    fn parse_speed_with_whitespace() {
        assert_eq!(parse_speed_json(r#"{ "speed" : 0.75 }"#), Some(0.75));
    }

    #[test]
    fn parse_speed_with_other_fields() {
        assert_eq!(
            parse_speed_json(r#"{"id": 1, "speed": 0.5, "name": "test"}"#),
            Some(0.5)
        );
    }

    #[test]
    fn parse_speed_negative() {
        // Negative should parse (though it may be invalid for the API)
        assert_eq!(parse_speed_json(r#"{"speed": -0.5}"#), Some(-0.5));
    }

    #[test]
    fn parse_speed_missing_key() {
        assert_eq!(parse_speed_json(r#"{"velocity": 0.5}"#), None);
    }

    #[test]
    fn parse_speed_invalid_value() {
        assert_eq!(parse_speed_json(r#"{"speed": "fast"}"#), None);
    }

    #[test]
    fn parse_speed_empty_json() {
        assert_eq!(parse_speed_json(r#"{}"#), None);
    }

    #[test]
    fn parse_speed_not_json() {
        assert_eq!(parse_speed_json("speed=0.5"), None);
    }

    #[test]
    fn parse_speed_trailing_content() {
        assert_eq!(parse_speed_json(r#"{"speed": 0.5, "other": 1}"#), Some(0.5));
    }

    // =========================================================================
    // parse_direction_json tests
    // =========================================================================

    #[test]
    fn parse_direction_forward() {
        assert_eq!(
            parse_direction_json(r#"{"direction": "forward"}"#),
            Some(Direction::Forward)
        );
    }

    #[test]
    fn parse_direction_reverse() {
        assert_eq!(
            parse_direction_json(r#"{"direction": "reverse"}"#),
            Some(Direction::Reverse)
        );
    }

    #[test]
    fn parse_direction_stopped() {
        assert_eq!(
            parse_direction_json(r#"{"direction": "stopped"}"#),
            Some(Direction::Stopped)
        );
    }

    #[test]
    fn parse_direction_with_whitespace() {
        assert_eq!(
            parse_direction_json(r#"{ "direction" : "forward" }"#),
            Some(Direction::Forward)
        );
    }

    #[test]
    fn parse_direction_missing_key() {
        assert_eq!(parse_direction_json(r#"{"dir": "forward"}"#), None);
    }

    #[test]
    fn parse_direction_invalid_value() {
        // Has "direction" key but invalid value
        assert_eq!(parse_direction_json(r#"{"direction": "left"}"#), None);
    }

    #[test]
    fn parse_direction_empty_json() {
        assert_eq!(parse_direction_json(r#"{}"#), None);
    }

    #[test]
    fn parse_direction_not_json() {
        assert_eq!(parse_direction_json("direction=forward"), None);
    }

    #[test]
    fn parse_direction_with_other_fields() {
        assert_eq!(
            parse_direction_json(r#"{"id": 1, "direction": "reverse", "speed": 0.5}"#),
            Some(Direction::Reverse)
        );
    }

    // =========================================================================
    // parse_max_speed_json tests
    // =========================================================================

    #[test]
    fn parse_max_speed_valid() {
        assert_eq!(parse_max_speed_json(r#"{"max_speed": 0.8}"#), Some(0.8));
    }

    #[test]
    fn parse_max_speed_one() {
        assert_eq!(parse_max_speed_json(r#"{"max_speed": 1.0}"#), Some(1.0));
    }

    #[test]
    fn parse_max_speed_zero() {
        assert_eq!(parse_max_speed_json(r#"{"max_speed": 0}"#), Some(0.0));
    }

    #[test]
    fn parse_max_speed_with_whitespace() {
        assert_eq!(parse_max_speed_json(r#"{ "max_speed" : 0.5 }"#), Some(0.5));
    }

    #[test]
    fn parse_max_speed_missing_key() {
        assert_eq!(parse_max_speed_json(r#"{"speed": 0.5}"#), None);
    }

    #[test]
    fn parse_max_speed_invalid_value() {
        assert_eq!(parse_max_speed_json(r#"{"max_speed": "high"}"#), None);
    }

    #[test]
    fn parse_max_speed_empty_json() {
        assert_eq!(parse_max_speed_json(r#"{}"#), None);
    }
}
