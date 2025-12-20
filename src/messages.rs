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
}
