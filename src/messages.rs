//! Shared message types for HTTP/MQTT communication.
//!
//! These types are `no_std` compatible and can be deserialized using either
//! `serde_json` (desktop) or `serde-json-core` (embedded).
//!
//! # Example
//!
//! ```
//! use rs_trainz::messages::SetSpeedRequest;
//!
//! // Desktop: using serde_json
//! #[cfg(feature = "std")]
//! {
//!     let json = r#"{"speed": 0.5, "duration_ms": 1000, "smooth": true}"#;
//!     let req: SetSpeedRequest = serde_json::from_str(json).unwrap();
//! }
//!
//! // Embedded: using serde-json-core
//! #[cfg(not(feature = "std"))]
//! {
//!     let json = br#"{"speed": 0.5}"#;
//!     let (req, _): (SetSpeedRequest, _) = serde_json_core::from_slice(json).unwrap();
//! }
//! ```

use crate::Direction;
use serde::{Deserialize, Serialize};

// ============================================================================
// Request Types
// ============================================================================

/// Request to set the train speed.
///
/// # Fields
///
/// - `speed`: Target speed (0.0 to 1.0)
/// - `duration_ms`: Transition duration in milliseconds (0 = immediate)
/// - `smooth`: Whether to use ease-in-out smoothing (requires `duration_ms` > 0)
///
/// # JSON Examples
///
/// Immediate speed change:
/// ```json
/// {"speed": 0.5}
/// ```
///
/// Linear transition over 2 seconds:
/// ```json
/// {"speed": 0.8, "duration_ms": 2000}
/// ```
///
/// Smooth ease-in-out transition:
/// ```json
/// {"speed": 1.0, "duration_ms": 3000, "smooth": true}
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SetSpeedRequest {
    /// Target speed (0.0 to 1.0)
    pub speed: f32,
    /// Transition duration in milliseconds (0 = immediate)
    #[serde(default)]
    pub duration_ms: u64,
    /// Whether to use ease-in-out smoothing
    #[serde(default)]
    pub smooth: bool,
}

impl SetSpeedRequest {
    /// Create a new immediate speed request.
    pub fn immediate(speed: f32) -> Self {
        Self {
            speed,
            duration_ms: 0,
            smooth: false,
        }
    }

    /// Create a new linear transition request.
    pub fn linear(speed: f32, duration_ms: u64) -> Self {
        Self {
            speed,
            duration_ms,
            smooth: false,
        }
    }

    /// Create a new smooth (ease-in-out) transition request.
    pub fn smooth(speed: f32, duration_ms: u64) -> Self {
        Self {
            speed,
            duration_ms,
            smooth: true,
        }
    }
}

/// Request to set the train direction.
///
/// # JSON Examples
///
/// ```json
/// {"direction": "forward"}
/// {"direction": "reverse"}
/// {"direction": "stopped"}
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SetDirectionRequest {
    /// Target direction
    pub direction: Direction,
}

impl SetDirectionRequest {
    /// Create a new direction request.
    pub fn new(direction: Direction) -> Self {
        Self { direction }
    }
}

/// Request to set maximum allowed speed.
///
/// # JSON Example
///
/// ```json
/// {"max_speed": 0.8}
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SetMaxSpeedRequest {
    /// Maximum speed (0.0 to 1.0)
    pub max_speed: f32,
}

impl SetMaxSpeedRequest {
    /// Create a new max speed request.
    pub fn new(max_speed: f32) -> Self {
        Self { max_speed }
    }
}

// ============================================================================
// Parsing Functions (using serde-json-core for no_std compatibility)
// ============================================================================

/// Parse a speed request from JSON bytes.
///
/// Works in both `std` and `no_std` environments using `serde-json-core`.
///
/// # Example
///
/// ```
/// use rs_trainz::messages::parse_speed_request;
///
/// let json = br#"{"speed": 0.5, "duration_ms": 1000, "smooth": true}"#;
/// let req = parse_speed_request(json).unwrap();
/// assert_eq!(req.speed, 0.5);
/// assert_eq!(req.duration_ms, 1000);
/// assert!(req.smooth);
/// ```
#[cfg(feature = "serde-json-core")]
pub fn parse_speed_request(json: &[u8]) -> Option<SetSpeedRequest> {
    serde_json_core::from_slice(json).ok().map(|(req, _)| req)
}

/// Parse a direction request from JSON bytes.
///
/// # Example
///
/// ```
/// use rs_trainz::messages::parse_direction_request;
/// use rs_trainz::Direction;
///
/// let json = br#"{"direction": "forward"}"#;
/// let req = parse_direction_request(json).unwrap();
/// assert_eq!(req.direction, Direction::Forward);
/// ```
#[cfg(feature = "serde-json-core")]
pub fn parse_direction_request(json: &[u8]) -> Option<SetDirectionRequest> {
    serde_json_core::from_slice(json).ok().map(|(req, _)| req)
}

/// Parse a max speed request from JSON bytes.
///
/// # Example
///
/// ```
/// use rs_trainz::messages::parse_max_speed_request;
///
/// let json = br#"{"max_speed": 0.8}"#;
/// let req = parse_max_speed_request(json).unwrap();
/// assert_eq!(req.max_speed, 0.8);
/// ```
#[cfg(feature = "serde-json-core")]
pub fn parse_max_speed_request(json: &[u8]) -> Option<SetMaxSpeedRequest> {
    serde_json_core::from_slice(json).ok().map(|(req, _)| req)
}

// ============================================================================
// MQTT Command Parsing (Unified for ESP32 and Desktop)
// ============================================================================

use crate::commands::{ThrottleCommand, ThrottleCommandDyn};
use crate::traits::{EaseInOut, Linear};

/// Parse an MQTT payload into a throttle command.
///
/// This unified function handles both JSON and plain-text payloads for
/// MQTT messages, supporting the same formats across ESP32 and desktop platforms.
///
/// # Topic Suffixes
///
/// - `"speed/set"` - Set speed (JSON or plain float)
/// - `"direction/set"` - Set direction (JSON or plain text)
/// - `"estop"` - Emergency stop (any payload)
/// - `"max-speed/set"` - Set max speed (JSON or plain float)
///
/// # Examples
///
/// ```
/// use rs_trainz::messages::parse_mqtt_command;
/// use rs_trainz::ThrottleCommandDyn;
///
/// // Speed from plain text
/// let cmd = parse_mqtt_command("speed/set", b"0.5");
/// assert!(matches!(cmd, Some(ThrottleCommandDyn::SetSpeed { .. })));
///
/// // Direction from text
/// let cmd = parse_mqtt_command("direction/set", b"forward");
/// assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(_))));
///
/// // Emergency stop
/// let cmd = parse_mqtt_command("estop", b"");
/// assert!(matches!(cmd, Some(ThrottleCommandDyn::EmergencyStop)));
/// ```
#[cfg(feature = "serde-json-core")]
pub fn parse_mqtt_command(topic_suffix: &str, payload: &[u8]) -> Option<ThrottleCommandDyn> {
    match topic_suffix {
        "speed/set" => parse_speed_payload(payload),
        "direction/set" => parse_direction_payload(payload),
        "estop" => Some(ThrottleCommandDyn::EmergencyStop),
        "max-speed/set" => parse_max_speed_payload(payload),
        _ => None,
    }
}

/// Parse speed payload from JSON or plain float.
///
/// Supports:
/// - Plain float: `"0.5"`
/// - JSON: `{"speed": 0.5, "duration_ms": 1000, "smooth": true}`
#[cfg(feature = "serde-json-core")]
pub fn parse_speed_payload(payload: &[u8]) -> Option<ThrottleCommandDyn> {
    // Try JSON first
    if let Some(req) = parse_speed_request(payload) {
        let speed = req.speed.clamp(0.0, 1.0);
        return Some(if req.duration_ms > 0 {
            if req.smooth {
                ThrottleCommand::SetSpeed {
                    target: speed,
                    strategy: EaseInOut::new(req.duration_ms),
                }
                .into()
            } else {
                ThrottleCommand::SetSpeed {
                    target: speed,
                    strategy: Linear::new(req.duration_ms),
                }
                .into()
            }
        } else {
            ThrottleCommand::speed_immediate(speed).into()
        });
    }

    // Fall back to plain float for backward compatibility
    let payload_str = core::str::from_utf8(payload).ok()?;
    let speed: f32 = payload_str.trim().parse().ok()?;
    Some(ThrottleCommand::speed_immediate(speed.clamp(0.0, 1.0)).into())
}

/// Parse direction payload from JSON or plain text.
///
/// Supports:
/// - Plain text: `"forward"`, `"fwd"`, `"1"`, `"reverse"`, `"rev"`, `"-1"`, `"stop"`, `"stopped"`
/// - JSON: `{"direction": "forward"}`
#[cfg(feature = "serde-json-core")]
pub fn parse_direction_payload(payload: &[u8]) -> Option<ThrottleCommandDyn> {
    // Try JSON first
    if let Some(req) = parse_direction_request(payload) {
        return Some(ThrottleCommandDyn::SetDirection(req.direction));
    }

    // Fall back to plain text using Direction::from_text()
    let payload_str = core::str::from_utf8(payload).ok()?;
    let dir = Direction::from_text(payload_str)?;
    Some(ThrottleCommandDyn::SetDirection(dir))
}

/// Parse max speed payload from JSON or plain float.
///
/// Supports:
/// - Plain float: `"0.8"`
/// - JSON: `{"max_speed": 0.8}`
#[cfg(feature = "serde-json-core")]
pub fn parse_max_speed_payload(payload: &[u8]) -> Option<ThrottleCommandDyn> {
    // Try JSON first
    if let Some(req) = parse_max_speed_request(payload) {
        return Some(ThrottleCommandDyn::SetMaxSpeed(req.max_speed.clamp(0.0, 1.0)));
    }

    // Fall back to plain float
    let payload_str = core::str::from_utf8(payload).ok()?;
    let max_speed: f32 = payload_str.trim().parse().ok()?;
    Some(ThrottleCommandDyn::SetMaxSpeed(max_speed.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // SetSpeedRequest tests
    // =========================================================================

    #[test]
    fn test_set_speed_request_immediate() {
        let req = SetSpeedRequest::immediate(0.5);
        assert_eq!(req.speed, 0.5);
        assert_eq!(req.duration_ms, 0);
        assert!(!req.smooth);
    }

    #[test]
    fn test_set_speed_request_linear() {
        let req = SetSpeedRequest::linear(0.8, 2000);
        assert_eq!(req.speed, 0.8);
        assert_eq!(req.duration_ms, 2000);
        assert!(!req.smooth);
    }

    #[test]
    fn test_set_speed_request_smooth() {
        let req = SetSpeedRequest::smooth(1.0, 3000);
        assert_eq!(req.speed, 1.0);
        assert_eq!(req.duration_ms, 3000);
        assert!(req.smooth);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_speed_request_serde_full() {
        let json = r#"{"speed": 0.75, "duration_ms": 2000, "smooth": true}"#;
        let req: SetSpeedRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.speed, 0.75);
        assert_eq!(req.duration_ms, 2000);
        assert!(req.smooth);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_speed_request_serde_defaults() {
        // duration_ms and smooth should default to 0 and false
        let json = r#"{"speed": 0.5}"#;
        let req: SetSpeedRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.speed, 0.5);
        assert_eq!(req.duration_ms, 0);
        assert!(!req.smooth);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_speed_request_serialize() {
        let req = SetSpeedRequest::smooth(0.5, 1000);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"speed\":0.5"));
        assert!(json.contains("\"duration_ms\":1000"));
        assert!(json.contains("\"smooth\":true"));
    }

    // =========================================================================
    // SetDirectionRequest tests
    // =========================================================================

    #[test]
    fn test_set_direction_request_new() {
        let req = SetDirectionRequest::new(Direction::Forward);
        assert_eq!(req.direction, Direction::Forward);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_direction_request_serde_forward() {
        let json = r#"{"direction": "forward"}"#;
        let req: SetDirectionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.direction, Direction::Forward);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_direction_request_serde_reverse() {
        let json = r#"{"direction": "reverse"}"#;
        let req: SetDirectionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.direction, Direction::Reverse);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_direction_request_serde_stopped() {
        let json = r#"{"direction": "stopped"}"#;
        let req: SetDirectionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.direction, Direction::Stopped);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_direction_request_serialize() {
        let req = SetDirectionRequest::new(Direction::Reverse);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"direction\":\"reverse\""));
    }

    // =========================================================================
    // SetMaxSpeedRequest tests
    // =========================================================================

    #[test]
    fn test_set_max_speed_request_new() {
        let req = SetMaxSpeedRequest::new(0.8);
        assert_eq!(req.max_speed, 0.8);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_max_speed_request_serde() {
        let json = r#"{"max_speed": 0.75}"#;
        let req: SetMaxSpeedRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.max_speed, 0.75);
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_set_max_speed_request_serialize() {
        let req = SetMaxSpeedRequest::new(0.9);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"max_speed\":0.9"));
    }

    // =========================================================================
    // MQTT Command Parsing tests
    // =========================================================================

    #[cfg(feature = "serde-json-core")]
    mod mqtt_parsing {
        use super::*;
        use crate::ThrottleCommandDyn;

        #[test]
        fn test_parse_mqtt_command_speed_plain() {
            let cmd = super::super::parse_mqtt_command("speed/set", b"0.5");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetSpeed { target, .. }) if (target - 0.5).abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_speed_json_immediate() {
            let cmd = super::super::parse_mqtt_command("speed/set", br#"{"speed": 0.75}"#);
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetSpeed { target, .. }) if (target - 0.75).abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_speed_json_linear() {
            let cmd = super::super::parse_mqtt_command("speed/set", br#"{"speed": 0.5, "duration_ms": 1000}"#);
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetSpeed { target, .. }) if (target - 0.5).abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_speed_json_smooth() {
            let cmd = super::super::parse_mqtt_command("speed/set", br#"{"speed": 0.5, "duration_ms": 2000, "smooth": true}"#);
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetSpeed { target, .. }) if (target - 0.5).abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_speed_clamped() {
            let cmd = super::super::parse_mqtt_command("speed/set", b"1.5");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetSpeed { target, .. }) if (target - 1.0).abs() < 0.001));

            let cmd = super::super::parse_mqtt_command("speed/set", b"-0.5");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetSpeed { target, .. }) if target.abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_direction_text() {
            let cmd = super::super::parse_mqtt_command("direction/set", b"forward");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Forward))));

            let cmd = super::super::parse_mqtt_command("direction/set", b"reverse");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Reverse))));

            let cmd = super::super::parse_mqtt_command("direction/set", b"stopped");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Stopped))));
        }

        #[test]
        fn test_parse_mqtt_command_direction_abbreviations() {
            let cmd = super::super::parse_mqtt_command("direction/set", b"fwd");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Forward))));

            let cmd = super::super::parse_mqtt_command("direction/set", b"rev");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Reverse))));

            let cmd = super::super::parse_mqtt_command("direction/set", b"stop");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Stopped))));
        }

        #[test]
        fn test_parse_mqtt_command_direction_numeric() {
            let cmd = super::super::parse_mqtt_command("direction/set", b"1");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Forward))));

            let cmd = super::super::parse_mqtt_command("direction/set", b"-1");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Reverse))));

            let cmd = super::super::parse_mqtt_command("direction/set", b"0");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Stopped))));
        }

        #[test]
        fn test_parse_mqtt_command_direction_json() {
            let cmd = super::super::parse_mqtt_command("direction/set", br#"{"direction": "forward"}"#);
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetDirection(Direction::Forward))));
        }

        #[test]
        fn test_parse_mqtt_command_estop() {
            let cmd = super::super::parse_mqtt_command("estop", b"");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::EmergencyStop)));

            let cmd = super::super::parse_mqtt_command("estop", b"any payload");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::EmergencyStop)));
        }

        #[test]
        fn test_parse_mqtt_command_max_speed_plain() {
            let cmd = super::super::parse_mqtt_command("max-speed/set", b"0.8");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetMaxSpeed(max)) if (max - 0.8).abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_max_speed_json() {
            let cmd = super::super::parse_mqtt_command("max-speed/set", br#"{"max_speed": 0.75}"#);
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetMaxSpeed(max)) if (max - 0.75).abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_max_speed_clamped() {
            let cmd = super::super::parse_mqtt_command("max-speed/set", b"1.5");
            assert!(matches!(cmd, Some(ThrottleCommandDyn::SetMaxSpeed(max)) if (max - 1.0).abs() < 0.001));
        }

        #[test]
        fn test_parse_mqtt_command_unknown_topic() {
            let cmd = super::super::parse_mqtt_command("unknown/topic", b"payload");
            assert!(cmd.is_none());
        }

        #[test]
        fn test_parse_mqtt_command_invalid_payload() {
            let cmd = super::super::parse_mqtt_command("speed/set", b"not a number");
            assert!(cmd.is_none());

            let cmd = super::super::parse_mqtt_command("direction/set", b"invalid");
            assert!(cmd.is_none());
        }
    }
}
