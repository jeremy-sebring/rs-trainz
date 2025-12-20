//! API request and response types for HTTP/MQTT communication.

use serde::{Deserialize, Serialize};

use crate::{CommandSource, Direction, FaultKind, ThrottleState};

// Re-export shared request types from messages module
pub use crate::messages::{SetDirectionRequest, SetMaxSpeedRequest, SetSpeedRequest};

// ============================================================================
// Response Types
// ============================================================================

/// API response wrapper for consistent JSON structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    /// Whether the request was successful
    pub success: bool,
    /// Response data (present when success=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    /// Error message (present when success=false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Create a successful response with data
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    /// Create an error response
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

/// Current throttle state response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateResponse {
    /// Current speed (0.0 to 1.0)
    pub speed: f32,
    /// Target speed if transitioning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_speed: Option<f32>,
    /// Current direction
    pub direction: Direction,
    /// Maximum allowed speed
    pub max_speed: f32,
    /// Active fault condition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<FaultKind>,
    /// Whether a transition is in progress
    pub transitioning: bool,
    /// Transition lock status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lock_status: Option<LockStatusResponse>,
    /// Transition progress information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<ProgressResponse>,
}

/// Lock status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockStatusResponse {
    /// Lock source
    pub source: CommandSource,
    /// Target speed of locked transition
    pub target: f32,
    /// Whether there's a queued command
    pub has_queued: bool,
}

/// Transition progress response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressResponse {
    /// Starting speed
    pub from: f32,
    /// Target speed
    pub to: f32,
    /// Current speed
    pub current: f32,
    /// Elapsed time in milliseconds
    pub elapsed_ms: u64,
    /// Estimated total duration (if known)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<u64>,
    /// Progress percentage (0.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<f32>,
}

impl From<&ThrottleState> for StateResponse {
    fn from(state: &ThrottleState) -> Self {
        Self {
            speed: state.speed,
            target_speed: state.target_speed,
            direction: state.direction,
            max_speed: state.max_speed,
            fault: state.fault,
            transitioning: state.target_speed.is_some(),
            lock_status: state.lock_status.as_ref().map(|l| LockStatusResponse {
                source: l.source,
                target: l.target,
                has_queued: l.has_queued,
            }),
            progress: state
                .transition_progress
                .as_ref()
                .map(|p| ProgressResponse {
                    from: p.from,
                    to: p.to,
                    current: p.current,
                    elapsed_ms: p.elapsed_ms,
                    total_ms: p.estimated_total_ms,
                    percent: p.percent(),
                }),
        }
    }
}

/// Command result response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    /// Whether the command was accepted
    pub accepted: bool,
    /// Result details
    pub result: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::TransitionLock;
    use crate::transition::{LockStatus, TransitionProgress};

    // ========================================================================
    // Request Types Tests
    // ========================================================================

    #[test]
    fn test_set_speed_request_serde() {
        // Test full serialization/deserialization
        let req = SetSpeedRequest {
            speed: 0.75,
            duration_ms: 2000,
            smooth: true,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: SetSpeedRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.speed, 0.75);
        assert_eq!(deserialized.duration_ms, 2000);
        assert!(deserialized.smooth);
    }

    #[test]
    fn test_set_speed_request_defaults() {
        // Test that duration_ms and smooth default to 0 and false
        let json = r#"{"speed": 0.5}"#;
        let req: SetSpeedRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.speed, 0.5);
        assert_eq!(req.duration_ms, 0);
        assert!(!req.smooth);
    }

    #[test]
    fn test_set_direction_request_serde() {
        let req = SetDirectionRequest {
            direction: Direction::Forward,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: SetDirectionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.direction, Direction::Forward);

        let req = SetDirectionRequest {
            direction: Direction::Reverse,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: SetDirectionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.direction, Direction::Reverse);
    }

    #[test]
    fn test_set_max_speed_request_serde() {
        let req = SetMaxSpeedRequest { max_speed: 0.8 };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: SetMaxSpeedRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_speed, 0.8);
    }

    // ========================================================================
    // ApiResponse Tests
    // ========================================================================

    #[test]
    fn test_api_response_ok() {
        let response = ApiResponse::ok("test data");
        assert!(response.success);
        assert_eq!(response.data, Some("test data"));
        assert_eq!(response.error, None);
    }

    #[test]
    fn test_api_response_ok_with_struct() {
        let response = ApiResponse::ok(CommandResponse::accepted("executed"));
        assert!(response.success);
        assert!(response.data.is_some());
        assert_eq!(response.error, None);
        let data = response.data.unwrap();
        assert!(data.accepted);
        assert_eq!(data.result, "executed");
    }

    #[test]
    fn test_api_response_err_with_string() {
        let response: ApiResponse<String> = ApiResponse::err("something went wrong");
        assert!(!response.success);
        assert_eq!(response.data, None);
        assert_eq!(response.error, Some("something went wrong".to_string()));
    }

    #[test]
    fn test_api_response_err_with_string_ref() {
        let error_msg = String::from("error message");
        let response: ApiResponse<String> = ApiResponse::err(&error_msg);
        assert!(!response.success);
        assert_eq!(response.data, None);
        assert_eq!(response.error, Some("error message".to_string()));
    }

    #[test]
    fn test_api_response_serde_ok() {
        let response = ApiResponse::ok(42);
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: ApiResponse<i32> = serde_json::from_str(&json).unwrap();
        assert!(deserialized.success);
        assert_eq!(deserialized.data, Some(42));
        assert_eq!(deserialized.error, None);
    }

    #[test]
    fn test_api_response_serde_err() {
        let response: ApiResponse<i32> = ApiResponse::err("failed");
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: ApiResponse<i32> = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.success);
        assert_eq!(deserialized.data, None);
        assert_eq!(deserialized.error, Some("failed".to_string()));
    }

    #[test]
    fn test_api_response_skip_serializing_none() {
        // Verify that None fields are omitted from JSON
        let response = ApiResponse::ok(42);
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("error"));

        let response: ApiResponse<i32> = ApiResponse::err("failed");
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("data"));
    }

    // ========================================================================
    // CommandResponse Tests
    // ========================================================================

    #[test]
    fn test_command_response_accepted() {
        let response = CommandResponse::accepted("command executed");
        assert!(response.accepted);
        assert_eq!(response.result, "command executed");
    }

    #[test]
    fn test_command_response_accepted_with_string() {
        let msg = String::from("done");
        let response = CommandResponse::accepted(msg);
        assert!(response.accepted);
        assert_eq!(response.result, "done");
    }

    #[test]
    fn test_command_response_rejected() {
        let response = CommandResponse::rejected("insufficient priority");
        assert!(!response.accepted);
        assert_eq!(response.result, "insufficient priority");
    }

    #[test]
    fn test_command_response_rejected_with_string() {
        let msg = String::from("locked");
        let response = CommandResponse::rejected(msg);
        assert!(!response.accepted);
        assert_eq!(response.result, "locked");
    }

    #[test]
    fn test_command_response_serde() {
        let response = CommandResponse::accepted("success");
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: CommandResponse = serde_json::from_str(&json).unwrap();
        assert!(deserialized.accepted);
        assert_eq!(deserialized.result, "success");

        let response = CommandResponse::rejected("failed");
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: CommandResponse = serde_json::from_str(&json).unwrap();
        assert!(!deserialized.accepted);
        assert_eq!(deserialized.result, "failed");
    }

    // ========================================================================
    // StateResponse From ThrottleState Tests
    // ========================================================================

    #[test]
    fn test_state_response_from_throttle_state_minimal() {
        // Test minimal state with no transition or fault
        let state = ThrottleState {
            speed: 0.5,
            target_speed: None,
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: None,
        };

        let response = StateResponse::from(&state);
        assert_eq!(response.speed, 0.5);
        assert_eq!(response.target_speed, None);
        assert_eq!(response.direction, Direction::Forward);
        assert_eq!(response.max_speed, 1.0);
        assert_eq!(response.fault, None);
        assert!(!response.transitioning);
        assert!(response.lock_status.is_none());
        assert!(response.progress.is_none());
    }

    #[test]
    fn test_state_response_from_throttle_state_with_target() {
        // Test state with target speed (transitioning)
        let state = ThrottleState {
            speed: 0.3,
            target_speed: Some(0.8),
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: None,
        };

        let response = StateResponse::from(&state);
        assert_eq!(response.speed, 0.3);
        assert_eq!(response.target_speed, Some(0.8));
        assert!(response.transitioning);
    }

    #[test]
    fn test_state_response_from_throttle_state_with_fault() {
        // Test state with fault
        let state = ThrottleState {
            speed: 0.0,
            target_speed: None,
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: Some(FaultKind::Overcurrent),
            lock_status: None,
            transition_progress: None,
        };

        let response = StateResponse::from(&state);
        assert_eq!(response.fault, Some(FaultKind::Overcurrent));
    }

    #[test]
    fn test_state_response_from_throttle_state_with_lock_status() {
        // Test state with lock status
        let lock = LockStatus {
            lock: TransitionLock::Hard,
            source: CommandSource::Physical,
            target: 0.7,
            has_queued: true,
        };

        let state = ThrottleState {
            speed: 0.5,
            target_speed: Some(0.7),
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: Some(lock),
            transition_progress: None,
        };

        let response = StateResponse::from(&state);
        assert!(response.lock_status.is_some());
        let lock_resp = response.lock_status.unwrap();
        assert_eq!(lock_resp.source, CommandSource::Physical);
        assert_eq!(lock_resp.target, 0.7);
        assert!(lock_resp.has_queued);
    }

    #[test]
    fn test_state_response_from_throttle_state_with_progress() {
        // Test state with transition progress
        let progress = TransitionProgress {
            from: 0.2,
            to: 0.8,
            current: 0.5,
            elapsed_ms: 1000,
            estimated_total_ms: Some(2000),
        };

        let state = ThrottleState {
            speed: 0.5,
            target_speed: Some(0.8),
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: Some(progress),
        };

        let response = StateResponse::from(&state);
        assert!(response.progress.is_some());
        let prog_resp = response.progress.unwrap();
        assert_eq!(prog_resp.from, 0.2);
        assert_eq!(prog_resp.to, 0.8);
        assert_eq!(prog_resp.current, 0.5);
        assert_eq!(prog_resp.elapsed_ms, 1000);
        assert_eq!(prog_resp.total_ms, Some(2000));
        assert_eq!(prog_resp.percent, Some(0.5));
    }

    #[test]
    fn test_state_response_from_throttle_state_with_progress_no_total() {
        // Test state with transition progress but no estimated total (e.g., Momentum)
        let progress = TransitionProgress {
            from: 0.0,
            to: 1.0,
            current: 0.3,
            elapsed_ms: 500,
            estimated_total_ms: None,
        };

        let state = ThrottleState {
            speed: 0.3,
            target_speed: Some(1.0),
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: Some(progress),
        };

        let response = StateResponse::from(&state);
        assert!(response.progress.is_some());
        let prog_resp = response.progress.unwrap();
        assert_eq!(prog_resp.total_ms, None);
        assert_eq!(prog_resp.percent, None);
    }

    #[test]
    fn test_state_response_from_throttle_state_complete() {
        // Test state with all fields populated
        let lock = LockStatus {
            lock: TransitionLock::Source,
            source: CommandSource::WebLocal,
            target: 0.9,
            has_queued: false,
        };

        let progress = TransitionProgress {
            from: 0.1,
            to: 0.9,
            current: 0.6,
            elapsed_ms: 1500,
            estimated_total_ms: Some(3000),
        };

        let state = ThrottleState {
            speed: 0.6,
            target_speed: Some(0.9),
            direction: Direction::Reverse,
            max_speed: 0.95,
            fault: Some(FaultKind::ShortCircuit),
            lock_status: Some(lock),
            transition_progress: Some(progress),
        };

        let response = StateResponse::from(&state);
        assert_eq!(response.speed, 0.6);
        assert_eq!(response.target_speed, Some(0.9));
        assert_eq!(response.direction, Direction::Reverse);
        assert_eq!(response.max_speed, 0.95);
        assert_eq!(response.fault, Some(FaultKind::ShortCircuit));
        assert!(response.transitioning);

        let lock_resp = response.lock_status.unwrap();
        assert_eq!(lock_resp.source, CommandSource::WebLocal);
        assert_eq!(lock_resp.target, 0.9);
        assert!(!lock_resp.has_queued);

        let prog_resp = response.progress.unwrap();
        assert_eq!(prog_resp.from, 0.1);
        assert_eq!(prog_resp.to, 0.9);
        assert_eq!(prog_resp.current, 0.6);
        assert_eq!(prog_resp.elapsed_ms, 1500);
        assert_eq!(prog_resp.total_ms, Some(3000));
        assert_eq!(prog_resp.percent, Some(0.5));
    }

    #[test]
    fn test_state_response_serde() {
        let state = ThrottleState {
            speed: 0.5,
            target_speed: Some(0.8),
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: None,
        };

        let response = StateResponse::from(&state);
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: StateResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.speed, 0.5);
        assert_eq!(deserialized.target_speed, Some(0.8));
        assert_eq!(deserialized.direction, Direction::Forward);
        assert!(deserialized.transitioning);
    }

    #[test]
    fn test_state_response_skip_serializing_none() {
        // Verify that None fields are omitted from JSON
        let state = ThrottleState {
            speed: 0.5,
            target_speed: None,
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: None,
        };

        let response = StateResponse::from(&state);
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("target_speed"));
        assert!(!json.contains("fault"));
        assert!(!json.contains("lock_status"));
        assert!(!json.contains("progress"));
    }

    // ========================================================================
    // LockStatusResponse Tests
    // ========================================================================

    #[test]
    fn test_lock_status_response_serde() {
        let lock = LockStatusResponse {
            source: CommandSource::Physical,
            target: 0.75,
            has_queued: true,
        };

        let json = serde_json::to_string(&lock).unwrap();
        let deserialized: LockStatusResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.source, CommandSource::Physical);
        assert_eq!(deserialized.target, 0.75);
        assert!(deserialized.has_queued);
    }

    // ========================================================================
    // ProgressResponse Tests
    // ========================================================================

    #[test]
    fn test_progress_response_serde_with_total() {
        let progress = ProgressResponse {
            from: 0.0,
            to: 1.0,
            current: 0.5,
            elapsed_ms: 1000,
            total_ms: Some(2000),
            percent: Some(0.5),
        };

        let json = serde_json::to_string(&progress).unwrap();
        let deserialized: ProgressResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.from, 0.0);
        assert_eq!(deserialized.to, 1.0);
        assert_eq!(deserialized.current, 0.5);
        assert_eq!(deserialized.elapsed_ms, 1000);
        assert_eq!(deserialized.total_ms, Some(2000));
        assert_eq!(deserialized.percent, Some(0.5));
    }

    #[test]
    fn test_progress_response_serde_without_total() {
        let progress = ProgressResponse {
            from: 0.0,
            to: 1.0,
            current: 0.3,
            elapsed_ms: 500,
            total_ms: None,
            percent: None,
        };

        let json = serde_json::to_string(&progress).unwrap();
        let deserialized: ProgressResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.from, 0.0);
        assert_eq!(deserialized.to, 1.0);
        assert_eq!(deserialized.current, 0.3);
        assert_eq!(deserialized.elapsed_ms, 500);
        assert_eq!(deserialized.total_ms, None);
        assert_eq!(deserialized.percent, None);
    }

    #[test]
    fn test_progress_response_skip_serializing_none() {
        let progress = ProgressResponse {
            from: 0.0,
            to: 1.0,
            current: 0.3,
            elapsed_ms: 500,
            total_ms: None,
            percent: None,
        };

        let json = serde_json::to_string(&progress).unwrap();
        assert!(!json.contains("total_ms"));
        assert!(!json.contains("percent"));
    }
}

impl CommandResponse {
    /// Create a response for an accepted command
    pub fn accepted(result: impl Into<String>) -> Self {
        Self {
            accepted: true,
            result: result.into(),
        }
    }

    /// Create a response for a rejected command
    pub fn rejected(reason: impl Into<String>) -> Self {
        Self {
            accepted: false,
            result: reason.into(),
        }
    }
}
