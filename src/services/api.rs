//! API request and response types for HTTP/MQTT communication.

use serde::{Deserialize, Serialize};

use crate::{CommandSource, Direction, FaultKind, ThrottleState};

// ============================================================================
// Request Types
// ============================================================================

/// Request to set the train speed
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Request to set the train direction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetDirectionRequest {
    /// Target direction
    pub direction: Direction,
}

/// Request to set maximum allowed speed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetMaxSpeedRequest {
    /// Maximum speed (0.0 to 1.0)
    pub max_speed: f32,
}

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
