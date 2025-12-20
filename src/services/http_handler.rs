//! Shared HTTP API handler logic for both desktop and ESP32.
//!
//! This module provides platform-agnostic HTTP request handling that can be
//! used by both Axum (desktop) and esp-idf-svc (ESP32) HTTP servers.
//!
//! # Design
//!
//! The `HttpApiHandler` struct contains the business logic for all API endpoints.
//! Platform-specific HTTP servers call these methods and translate the results
//! to their native response formats.
//!
//! # Example
//!
//! ```ignore
//! use rs_trainz::services::HttpApiHandler;
//!
//! let handler = HttpApiHandler::new(shared_state);
//!
//! // In Axum handler:
//! let json = handler.handle_get_state();
//!
//! // In ESP-IDF handler:
//! let json = handler.handle_get_state();
//! resp.write_all(json.as_bytes())?;
//! ```

use alloc::format;
use alloc::string::String;

use crate::messages::{parse_direction_request, parse_max_speed_request, parse_speed_request};
use crate::traits::{EaseInOut, Immediate, Linear};
use crate::{CommandOutcome, CommandSource, Direction, ThrottleCommand, ThrottleState};

use super::shared::StateProvider;

extern crate alloc;

// ============================================================================
// API Response Types
// ============================================================================

/// Result of an API operation.
#[derive(Debug)]
pub enum ApiResult {
    /// Success with JSON response body.
    Ok(String),
    /// Error with status code and message.
    Error(u16, String),
}

impl ApiResult {
    /// Create a success response.
    pub fn ok(json: impl Into<String>) -> Self {
        Self::Ok(json.into())
    }

    /// Create an error response.
    pub fn error(status: u16, message: impl Into<String>) -> Self {
        Self::Error(status, message.into())
    }

    /// Create a bad request (400) error.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::Error(400, message.into())
    }

    /// Check if this is a success response.
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok(_))
    }

    /// Get the JSON body (for success) or error message.
    pub fn body(&self) -> &str {
        match self {
            Self::Ok(json) => json,
            Self::Error(_, msg) => msg,
        }
    }

    /// Get the HTTP status code.
    pub fn status(&self) -> u16 {
        match self {
            Self::Ok(_) => 200,
            Self::Error(status, _) => *status,
        }
    }
}

// Axum integration: allow ApiResult to be returned directly from handlers
#[cfg(feature = "web")]
impl axum::response::IntoResponse for ApiResult {
    fn into_response(self) -> axum::response::Response {
        use axum::http::{header, StatusCode};
        use axum::response::Response;

        let status = StatusCode::from_u16(self.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = self.body().to_string();

        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(body))
            .unwrap()
    }
}

// ============================================================================
// HTTP API Handler
// ============================================================================

/// Shared HTTP API handler for both desktop and ESP32.
///
/// Contains the business logic for all REST API endpoints. Platform-specific
/// HTTP servers (Axum, esp-idf-svc) call these methods and adapt the results.
pub struct HttpApiHandler<S: StateProvider> {
    state: S,
}

impl<S: StateProvider> HttpApiHandler<S> {
    /// Create a new handler with the given state provider.
    pub fn new(state: S) -> Self {
        Self { state }
    }

    /// GET /api/state - Get current throttle state.
    ///
    /// Returns JSON with current speed, direction, and transition state.
    pub fn handle_get_state(&self) -> String {
        let state = self.state.state();
        state_to_json(&state)
    }

    /// POST /api/speed - Set speed with optional transition.
    ///
    /// Accepts JSON:
    /// - Immediate: `{"speed": 0.5}`
    /// - Linear transition: `{"speed": 0.5, "duration_ms": 1000}`
    /// - Smooth transition: `{"speed": 0.5, "duration_ms": 1000, "smooth": true}`
    pub fn handle_set_speed(&self, body: &str) -> ApiResult {
        let Some(req) = parse_speed_request(body.as_bytes()) else {
            return ApiResult::bad_request(r#"{"error":"invalid speed request"}"#);
        };

        if !(0.0..=1.0).contains(&req.speed) {
            return ApiResult::bad_request(r#"{"error":"speed must be between 0.0 and 1.0"}"#);
        }

        let cmd = if req.duration_ms > 0 {
            if req.smooth {
                ThrottleCommand::SetSpeed {
                    target: req.speed,
                    strategy: EaseInOut::new(req.duration_ms),
                }
                .into()
            } else {
                ThrottleCommand::SetSpeed {
                    target: req.speed,
                    strategy: Linear::new(req.duration_ms),
                }
                .into()
            }
        } else {
            ThrottleCommand::speed_immediate(req.speed).into()
        };

        match self.state.apply_command(cmd, CommandSource::WebApi) {
            Ok(outcome) => ApiResult::ok(command_outcome_to_json(outcome)),
            Err(_) => ApiResult::error(500, r#"{"error":"controller error"}"#),
        }
    }

    /// POST /api/direction - Set direction.
    ///
    /// Accepts JSON: `{"direction": "forward"}`, `{"direction": "reverse"}`, or `{"direction": "stopped"}`
    pub fn handle_set_direction(&self, body: &str) -> ApiResult {
        let Some(req) = parse_direction_request(body.as_bytes()) else {
            return ApiResult::bad_request(r#"{"error":"invalid direction"}"#);
        };

        let cmd = ThrottleCommand::<Immediate>::SetDirection(req.direction).into();
        match self.state.apply_command(cmd, CommandSource::WebApi) {
            Ok(_) => ApiResult::ok(r#"{"ok":true,"result":"direction_set"}"#),
            Err(_) => ApiResult::error(500, r#"{"error":"controller error"}"#),
        }
    }

    /// POST /api/estop - Emergency stop.
    pub fn handle_estop(&self) -> ApiResult {
        let cmd = ThrottleCommand::estop().into();
        match self.state.apply_command(cmd, CommandSource::WebApi) {
            Ok(_) => ApiResult::ok(r#"{"ok":true,"result":"emergency_stop"}"#),
            Err(_) => ApiResult::error(500, r#"{"error":"controller error"}"#),
        }
    }

    /// POST /api/max-speed - Set maximum speed limit.
    ///
    /// Accepts JSON: `{"max_speed": 0.8}`
    pub fn handle_set_max_speed(&self, body: &str) -> ApiResult {
        let Some(req) = parse_max_speed_request(body.as_bytes()) else {
            return ApiResult::bad_request(r#"{"error":"invalid max_speed"}"#);
        };

        if !(0.0..=1.0).contains(&req.max_speed) {
            return ApiResult::bad_request(r#"{"error":"max_speed must be between 0.0 and 1.0"}"#);
        }

        let cmd = ThrottleCommand::<Immediate>::SetMaxSpeed(req.max_speed).into();
        match self.state.apply_command(cmd, CommandSource::WebApi) {
            Ok(_) => ApiResult::ok(r#"{"ok":true,"result":"max_speed_set"}"#),
            Err(_) => ApiResult::error(500, r#"{"error":"controller error"}"#),
        }
    }

    /// GET / - Get web UI HTML.
    pub fn handle_index(&self) -> &'static str {
        include_str!("../../www/index.html")
    }
}

// ============================================================================
// JSON Helpers
// ============================================================================

/// Convert throttle state to JSON string.
pub fn state_to_json(state: &ThrottleState) -> String {
    let target = state.target_speed.unwrap_or(state.speed);
    let is_transitioning = state.transition_progress.is_some();

    format!(
        r#"{{"speed":{:.2},"target_speed":{:.2},"direction":"{}","max_speed":{:.2},"is_transitioning":{}}}"#,
        state.speed,
        target,
        direction_str(&state.direction),
        state.max_speed,
        is_transitioning
    )
}

/// Convert direction to string.
pub fn direction_str(dir: &Direction) -> &'static str {
    match dir {
        Direction::Forward => "forward",
        Direction::Reverse => "reverse",
        Direction::Stopped => "stopped",
    }
}

/// Convert command outcome to JSON.
fn command_outcome_to_json(outcome: CommandOutcome) -> String {
    let result = match outcome {
        CommandOutcome::Applied => "applied",
        CommandOutcome::SpeedTransition(r) => match r {
            crate::TransitionResult::Started => "transition_started",
            crate::TransitionResult::Queued => "queued",
            crate::TransitionResult::Interrupted { .. } => "interrupted_previous",
            crate::TransitionResult::Rejected { .. } => "rejected",
        },
    };
    format!(r#"{{"ok":true,"result":"{}"}}"#, result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RejectReason, TransitionResult};
    use alloc::sync::Arc;
    use std::sync::Mutex;

    // ========================================================================
    // MockStateProvider for testing HttpApiHandler
    // ========================================================================

    /// Mock state provider that allows controlled responses for testing.
    struct MockStateProvider {
        state: Mutex<ThrottleState>,
        command_result: Mutex<Result<CommandOutcome, ()>>,
        last_command: Mutex<Option<(crate::ThrottleCommandDyn, CommandSource)>>,
    }

    impl MockStateProvider {
        fn new() -> Self {
            Self {
                state: Mutex::new(ThrottleState {
                    speed: 0.0,
                    direction: Direction::Stopped,
                    max_speed: 1.0,
                    target_speed: None,
                    transition_progress: None,
                    fault: None,
                    lock_status: None,
                }),
                command_result: Mutex::new(Ok(CommandOutcome::Applied)),
                last_command: Mutex::new(None),
            }
        }

        fn with_state(self, speed: f32, direction: Direction) -> Self {
            let mut state = self.state.lock().unwrap();
            state.speed = speed;
            state.direction = direction;
            drop(state);
            self
        }

        fn with_transition(self, target: f32, progress: f32) -> Self {
            let mut state = self.state.lock().unwrap();
            state.target_speed = Some(target);
            state.transition_progress = Some(crate::TransitionProgress {
                from: 0.0,
                to: target,
                current: progress * target,
                elapsed_ms: (progress * 1000.0) as u64,
                estimated_total_ms: Some(1000),
            });
            drop(state);
            self
        }

        fn with_command_result(self, result: Result<CommandOutcome, ()>) -> Self {
            *self.command_result.lock().unwrap() = result;
            self
        }

        fn last_command(&self) -> Option<(crate::ThrottleCommandDyn, CommandSource)> {
            self.last_command.lock().unwrap().clone()
        }
    }

    impl StateProvider for MockStateProvider {
        fn state(&self) -> ThrottleState {
            self.state.lock().unwrap().clone()
        }

        fn now_ms(&self) -> u64 {
            1000 // Fixed time for testing
        }

        fn apply_command(
            &self,
            cmd: crate::ThrottleCommandDyn,
            source: CommandSource,
        ) -> Result<CommandOutcome, ()> {
            *self.last_command.lock().unwrap() = Some((cmd, source));
            self.command_result.lock().unwrap().clone()
        }
    }

    impl StateProvider for Arc<MockStateProvider> {
        fn state(&self) -> ThrottleState {
            (**self).state()
        }

        fn now_ms(&self) -> u64 {
            (**self).now_ms()
        }

        fn apply_command(
            &self,
            cmd: crate::ThrottleCommandDyn,
            source: CommandSource,
        ) -> Result<CommandOutcome, ()> {
            (**self).apply_command(cmd, source)
        }
    }

    // ========================================================================
    // Helper function tests
    // ========================================================================

    #[test]
    fn test_direction_str() {
        assert_eq!(direction_str(&Direction::Forward), "forward");
        assert_eq!(direction_str(&Direction::Reverse), "reverse");
        assert_eq!(direction_str(&Direction::Stopped), "stopped");
    }

    #[test]
    fn test_api_result() {
        let ok = ApiResult::ok(r#"{"status":"ok"}"#);
        assert!(ok.is_ok());
        assert_eq!(ok.status(), 200);

        let err = ApiResult::bad_request("bad input");
        assert!(!err.is_ok());
        assert_eq!(err.status(), 400);
    }

    #[test]
    fn test_api_result_error() {
        let err = ApiResult::error(500, "internal error");
        assert!(!err.is_ok());
        assert_eq!(err.status(), 500);
        assert_eq!(err.body(), "internal error");
    }

    #[test]
    fn test_state_to_json_basic() {
        let state = ThrottleState {
            speed: 0.5,
            direction: Direction::Forward,
            max_speed: 1.0,
            target_speed: None,
            transition_progress: None,
            fault: None,
            lock_status: None,
        };

        let json = state_to_json(&state);
        assert!(json.contains("\"speed\":0.50"));
        assert!(json.contains("\"direction\":\"forward\""));
        assert!(json.contains("\"max_speed\":1.00"));
        assert!(json.contains("\"is_transitioning\":false"));
    }

    #[test]
    fn test_state_to_json_with_transition() {
        let state = ThrottleState {
            speed: 0.3,
            direction: Direction::Reverse,
            max_speed: 0.8,
            target_speed: Some(0.7),
            transition_progress: Some(crate::TransitionProgress {
                from: 0.0,
                to: 0.7,
                current: 0.3,
                elapsed_ms: 500,
                estimated_total_ms: Some(1000),
            }),
            fault: None,
            lock_status: None,
        };

        let json = state_to_json(&state);
        assert!(json.contains("\"speed\":0.30"));
        assert!(json.contains("\"target_speed\":0.70"));
        assert!(json.contains("\"direction\":\"reverse\""));
        assert!(json.contains("\"is_transitioning\":true"));
    }

    #[test]
    fn test_command_outcome_to_json_applied() {
        let json = command_outcome_to_json(CommandOutcome::Applied);
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"result\":\"applied\""));
    }

    #[test]
    fn test_command_outcome_to_json_transition_started() {
        let json =
            command_outcome_to_json(CommandOutcome::SpeedTransition(TransitionResult::Started));
        assert!(json.contains("\"result\":\"transition_started\""));
    }

    #[test]
    fn test_command_outcome_to_json_queued() {
        let json =
            command_outcome_to_json(CommandOutcome::SpeedTransition(TransitionResult::Queued));
        assert!(json.contains("\"result\":\"queued\""));
    }

    #[test]
    fn test_command_outcome_to_json_interrupted() {
        let json = command_outcome_to_json(CommandOutcome::SpeedTransition(
            TransitionResult::Interrupted {
                previous_target: 0.5,
            },
        ));
        assert!(json.contains("\"result\":\"interrupted_previous\""));
    }

    #[test]
    fn test_command_outcome_to_json_rejected() {
        let json = command_outcome_to_json(CommandOutcome::SpeedTransition(
            TransitionResult::Rejected {
                reason: RejectReason::TransitionLocked,
            },
        ));
        assert!(json.contains("\"result\":\"rejected\""));
    }

    // ========================================================================
    // HttpApiHandler tests
    // ========================================================================

    #[test]
    fn test_handle_get_state() {
        let provider = Arc::new(MockStateProvider::new().with_state(0.75, Direction::Forward));
        let handler = HttpApiHandler::new(provider);

        let json = handler.handle_get_state();
        assert!(json.contains("\"speed\":0.75"));
        assert!(json.contains("\"direction\":\"forward\""));
    }

    #[test]
    fn test_handle_get_state_with_transition() {
        let provider = Arc::new(
            MockStateProvider::new()
                .with_state(0.3, Direction::Reverse)
                .with_transition(0.8, 0.5),
        );
        let handler = HttpApiHandler::new(provider);

        let json = handler.handle_get_state();
        assert!(json.contains("\"is_transitioning\":true"));
        assert!(json.contains("\"target_speed\":0.80"));
    }

    #[test]
    fn test_handle_set_speed_valid() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider.clone());

        let result = handler.handle_set_speed(r#"{"speed": 0.5}"#);
        assert!(result.is_ok());

        let (cmd, source) = provider.last_command().expect("command should be captured");
        assert!(matches!(source, CommandSource::WebApi));
        match cmd {
            crate::ThrottleCommandDyn::SetSpeed { target, .. } => {
                assert!((target - 0.5).abs() < 0.001);
            }
            _ => panic!("expected SetSpeed command"),
        }
    }

    #[test]
    fn test_handle_set_speed_invalid_json() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider);

        let result = handler.handle_set_speed("not valid json");
        assert!(!result.is_ok());
        assert_eq!(result.status(), 400);
        assert!(result.body().contains("invalid speed"));
    }

    #[test]
    fn test_handle_set_speed_missing_field() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider);

        let result = handler.handle_set_speed(r#"{"other": 0.5}"#);
        assert!(!result.is_ok());
        assert_eq!(result.status(), 400);
    }

    #[test]
    fn test_handle_set_speed_controller_error() {
        let provider = Arc::new(MockStateProvider::new().with_command_result(Err(())));
        let handler = HttpApiHandler::new(provider);

        let result = handler.handle_set_speed(r#"{"speed": 0.5}"#);
        assert!(!result.is_ok());
        assert_eq!(result.status(), 500);
        assert!(result.body().contains("controller error"));
    }

    #[test]
    fn test_handle_set_direction_forward() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider.clone());

        let result = handler.handle_set_direction(r#"{"direction": "forward"}"#);
        assert!(result.is_ok());

        let (cmd, _) = provider.last_command().expect("command should be captured");
        assert!(matches!(
            cmd,
            crate::ThrottleCommandDyn::SetDirection(Direction::Forward)
        ));
    }

    #[test]
    fn test_handle_set_direction_reverse() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider.clone());

        let result = handler.handle_set_direction(r#"{"direction": "reverse"}"#);
        assert!(result.is_ok());

        let (cmd, _) = provider.last_command().expect("command should be captured");
        assert!(matches!(
            cmd,
            crate::ThrottleCommandDyn::SetDirection(Direction::Reverse)
        ));
    }

    #[test]
    fn test_handle_set_direction_invalid() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider);

        let result = handler.handle_set_direction(r#"{"direction": "sideways"}"#);
        assert!(!result.is_ok());
        assert_eq!(result.status(), 400);
    }

    #[test]
    fn test_handle_estop() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider.clone());

        let result = handler.handle_estop();
        assert!(result.is_ok());
        assert!(result.body().contains("emergency_stop"));

        let (cmd, _) = provider.last_command().expect("command should be captured");
        assert!(matches!(cmd, crate::ThrottleCommandDyn::EmergencyStop));
    }

    #[test]
    fn test_handle_set_max_speed_valid() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider.clone());

        let result = handler.handle_set_max_speed(r#"{"max_speed": 0.8}"#);
        assert!(result.is_ok());

        let (cmd, _) = provider.last_command().expect("command should be captured");
        assert!(
            matches!(cmd, crate::ThrottleCommandDyn::SetMaxSpeed(s) if (s - 0.8).abs() < 0.001)
        );
    }

    #[test]
    fn test_handle_set_max_speed_out_of_range_high() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider);

        let result = handler.handle_set_max_speed(r#"{"max_speed": 1.5}"#);
        assert!(!result.is_ok());
        assert_eq!(result.status(), 400);
        assert!(result.body().contains("between 0.0 and 1.0"));
    }

    #[test]
    fn test_handle_set_max_speed_out_of_range_negative() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider);

        let result = handler.handle_set_max_speed(r#"{"max_speed": -0.1}"#);
        assert!(!result.is_ok());
        assert_eq!(result.status(), 400);
    }

    #[test]
    fn test_handle_set_max_speed_invalid_json() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider);

        let result = handler.handle_set_max_speed("invalid");
        assert!(!result.is_ok());
        assert_eq!(result.status(), 400);
    }

    #[test]
    fn test_handle_index_returns_html() {
        let provider = Arc::new(MockStateProvider::new());
        let handler = HttpApiHandler::new(provider);

        let html = handler.handle_index();
        assert!(html.contains("<!DOCTYPE html>") || html.contains("<html"));
    }
}
