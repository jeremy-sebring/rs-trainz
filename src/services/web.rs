//! Axum-based HTTP server for the throttle controller API.
//!
//! Provides REST endpoints for:
//! - GET `/api/state` - Current throttle state
//! - POST `/api/speed` - Set speed with optional transition
//! - POST `/api/direction` - Set direction
//! - POST `/api/estop` - Emergency stop
//! - POST `/api/max-speed` - Set maximum speed limit
//! - GET `/` - Web UI (serves index.html)

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::config::WebConfig;
use crate::parsing::{parse_direction_json, parse_max_speed_json, parse_speed_json};
use crate::traits::{Immediate, MotorController};
use crate::{CommandOutcome, CommandSource, ThrottleCommand, ThrottleController};

use super::api::{ApiResponse, CommandResponse, StateResponse};
use super::shared::SharedThrottleState;

// ============================================================================
// Backward Compatibility
// ============================================================================

/// Type alias for backward compatibility.
///
/// New code should use `SharedThrottleState` directly for clarity.
/// This alias allows existing code using `AppState` to continue working.
pub type AppState<M> = SharedThrottleState<M>;

// ============================================================================
// Route Handlers
// ============================================================================

/// GET /api/state - Returns current throttle state
async fn get_state<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
) -> Json<ApiResponse<StateResponse>> {
    let throttle_state = state.state();
    Json(ApiResponse::ok(StateResponse::from(&throttle_state)))
}

/// POST /api/speed - Set speed (immediate)
///
/// Accepts JSON: `{"speed": 0.5}`
/// Uses the same simple parser as ESP32 for consistency.
async fn set_speed<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
    body: Bytes,
) -> Json<ApiResponse<CommandResponse>> {
    let body_str = std::str::from_utf8(&body).unwrap_or("");

    let Some(speed) = parse_speed_json(body_str) else {
        return Json(ApiResponse::err("Invalid speed request"));
    };

    // Validate speed
    if speed < 0.0 || speed > 1.0 {
        return Json(ApiResponse::err("Speed must be between 0.0 and 1.0"));
    }

    let now_ms = state.now_ms();
    let cmd = ThrottleCommand::speed_immediate(speed).into();

    let result = state
        .with_controller(|controller| controller.apply_command(cmd, CommandSource::WebApi, now_ms));

    match result {
        Ok(outcome) => {
            let result = match outcome {
                CommandOutcome::Applied => "applied",
                CommandOutcome::SpeedTransition(r) => match r {
                    crate::TransitionResult::Started => "transition_started",
                    crate::TransitionResult::Queued => "queued",
                    crate::TransitionResult::Interrupted { .. } => "interrupted_previous",
                    crate::TransitionResult::Rejected { reason } => {
                        return Json(ApiResponse::ok(CommandResponse::rejected(format!(
                            "{:?}",
                            reason
                        ))));
                    }
                },
            };
            Json(ApiResponse::ok(CommandResponse::accepted(result)))
        }
        Err(_) => Json(ApiResponse::err("Motor controller error")),
    }
}

/// POST /api/direction - Set direction
///
/// Accepts JSON: `{"direction": "forward"}` or `{"direction": "reverse"}`
/// Uses the same simple parser as ESP32 for consistency.
async fn set_direction<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
    body: Bytes,
) -> Json<ApiResponse<CommandResponse>> {
    let body_str = std::str::from_utf8(&body).unwrap_or("");

    let Some(direction) = parse_direction_json(body_str) else {
        return Json(ApiResponse::err("Invalid direction request"));
    };

    let now_ms = state.now_ms();
    let cmd = ThrottleCommand::<Immediate>::SetDirection(direction).into();

    let result = state
        .with_controller(|controller| controller.apply_command(cmd, CommandSource::WebApi, now_ms));

    match result {
        Ok(_) => Json(ApiResponse::ok(CommandResponse::accepted("direction_set"))),
        Err(_) => Json(ApiResponse::err("Motor controller error")),
    }
}

/// POST /api/estop - Emergency stop
async fn emergency_stop<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
) -> Json<ApiResponse<CommandResponse>> {
    let now_ms = state.now_ms();
    let cmd = ThrottleCommand::estop().into();

    let result = state
        .with_controller(|controller| controller.apply_command(cmd, CommandSource::WebApi, now_ms));

    match result {
        Ok(_) => Json(ApiResponse::ok(CommandResponse::accepted("emergency_stop"))),
        Err(_) => Json(ApiResponse::err("Motor controller error")),
    }
}

/// POST /api/max-speed - Set maximum allowed speed
///
/// Accepts JSON: `{"max_speed": 0.8}`
/// Uses the same simple parser as other endpoints for consistency.
async fn set_max_speed<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
    body: Bytes,
) -> Json<ApiResponse<CommandResponse>> {
    let body_str = std::str::from_utf8(&body).unwrap_or("");

    let Some(max_speed) = parse_max_speed_json(body_str) else {
        return Json(ApiResponse::err("Invalid max_speed request"));
    };

    // Validate max speed
    if max_speed < 0.0 || max_speed > 1.0 {
        return Json(ApiResponse::err("Max speed must be between 0.0 and 1.0"));
    }

    let now_ms = state.now_ms();
    let cmd = ThrottleCommand::<Immediate>::SetMaxSpeed(max_speed).into();

    let result = state
        .with_controller(|controller| controller.apply_command(cmd, CommandSource::WebApi, now_ms));

    match result {
        Ok(_) => Json(ApiResponse::ok(CommandResponse::accepted("max_speed_set"))),
        Err(_) => Json(ApiResponse::err("Motor controller error")),
    }
}

/// GET / - Serve the web UI
async fn index() -> impl IntoResponse {
    Html(include_str!("../../www/index.html"))
}

/// Fallback handler for 404
async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::<()>::err("Not found")),
    )
}

// ============================================================================
// Server Builder
// ============================================================================

/// Configuration for the web server
#[derive(Debug, Clone)]
pub struct WebServerConfig {
    /// Address to bind to
    pub addr: SocketAddr,
    /// Whether to enable CORS for all origins
    pub cors_permissive: bool,
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:8080".parse().unwrap(),
            cors_permissive: true,
        }
    }
}

impl WebServerConfig {
    /// Create a new config with the given address
    pub fn new(addr: impl Into<SocketAddr>) -> Self {
        Self {
            addr: addr.into(),
            ..Default::default()
        }
    }

    /// Set whether CORS should be permissive
    pub fn cors(mut self, permissive: bool) -> Self {
        self.cors_permissive = permissive;
        self
    }

    /// Create from shared WebConfig
    pub fn from_config(config: &WebConfig) -> Self {
        Self {
            addr: ([0, 0, 0, 0], config.port).into(),
            cors_permissive: config.cors_permissive,
        }
    }
}

/// Build the Axum router with all routes
pub fn build_router<M: MotorController + Send + 'static>(
    state: Arc<SharedThrottleState<M>>,
    config: &WebServerConfig,
) -> Router {
    let mut router = Router::new()
        // API routes
        .route("/api/state", get(get_state::<M>))
        .route("/api/speed", post(set_speed::<M>))
        .route("/api/direction", post(set_direction::<M>))
        .route("/api/estop", post(emergency_stop::<M>))
        .route("/api/max-speed", post(set_max_speed::<M>))
        // Web UI
        .route("/", get(index))
        // Fallback
        .fallback(not_found)
        .with_state(state);

    // Add CORS if requested
    if config.cors_permissive {
        router = router.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );
    }

    router
}

/// Start the web server
///
/// This function blocks until the server is shut down.
/// Creates its own `SharedThrottleState` - use `run_server_with_state` to share
/// state with other services.
pub async fn run_server<M: MotorController + Send + 'static>(
    controller: ThrottleController<M>,
    config: WebServerConfig,
) -> Result<(), std::io::Error> {
    let state = Arc::new(SharedThrottleState::new(controller));
    let router = build_router(state, &config);

    let listener = tokio::net::TcpListener::bind(config.addr).await?;
    println!("Web server listening on http://{}", config.addr);

    axum::serve(listener, router).await
}

/// Start the web server with shared state
///
/// Use this when you need to share state with other services (MQTT, physical input).
/// This is the recommended way to run the web server in a multi-service setup.
///
/// # Example
///
/// ```ignore
/// let state = Arc::new(SharedThrottleState::new(controller));
///
/// // Share state with MQTT
/// let mqtt_handler = MqttHandler::with_shared_state(Arc::clone(&state), mqtt_config);
///
/// // Run web server with same state
/// run_server_with_state(state, web_config).await?;
/// ```
pub async fn run_server_with_state<M: MotorController + Send + 'static>(
    state: Arc<SharedThrottleState<M>>,
    config: WebServerConfig,
) -> Result<(), std::io::Error> {
    let router = build_router(state, &config);

    let listener = tokio::net::TcpListener::bind(config.addr).await?;
    println!("Web server listening on http://{}", config.addr);

    axum::serve(listener, router).await
}
