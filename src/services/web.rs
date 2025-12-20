//! Axum-based HTTP server for the throttle controller API.
//!
//! This module provides a thin Axum wrapper around the shared [`HttpApiHandler`],
//! which contains all the business logic for REST endpoints.
//!
//! # Endpoints
//!
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
    Router,
};
use tower_http::cors::{Any, CorsLayer};

use crate::config::WebConfig;
use crate::traits::MotorController;
use crate::ThrottleController;

use super::api::ApiResponse;
use super::http_handler::{ApiResult, HttpApiHandler};
use super::shared::SharedThrottleState;

// ============================================================================
// Backward Compatibility
// ============================================================================

/// Type alias for backward compatibility.
///
/// New code should use `SharedThrottleState` directly for clarity.
pub type AppState<M> = SharedThrottleState<M>;

// ============================================================================
// Route Handlers (thin wrappers around HttpApiHandler)
// ============================================================================

/// GET /api/state
async fn get_state<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
) -> impl IntoResponse {
    let handler = HttpApiHandler::new(Arc::clone(&state));
    let json = handler.handle_get_state();
    ApiResult::ok(json)
}

/// POST /api/speed
async fn set_speed<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
    body: Bytes,
) -> impl IntoResponse {
    let handler = HttpApiHandler::new(Arc::clone(&state));
    let body_str = std::str::from_utf8(&body).unwrap_or("");
    handler.handle_set_speed(body_str)
}

/// POST /api/direction
async fn set_direction<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
    body: Bytes,
) -> impl IntoResponse {
    let handler = HttpApiHandler::new(Arc::clone(&state));
    let body_str = std::str::from_utf8(&body).unwrap_or("");
    handler.handle_set_direction(body_str)
}

/// POST /api/estop
async fn emergency_stop<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
) -> impl IntoResponse {
    let handler = HttpApiHandler::new(Arc::clone(&state));
    handler.handle_estop()
}

/// POST /api/max-speed
async fn set_max_speed<M: MotorController + Send + 'static>(
    State(state): State<Arc<SharedThrottleState<M>>>,
    body: Bytes,
) -> impl IntoResponse {
    let handler = HttpApiHandler::new(Arc::clone(&state));
    let body_str = std::str::from_utf8(&body).unwrap_or("");
    handler.handle_set_max_speed(body_str)
}

/// GET / - Serve the web UI
async fn index() -> impl IntoResponse {
    Html(include_str!("../../www/index.html"))
}

/// Fallback handler for 404
async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        axum::Json(ApiResponse::<()>::err("Not found")),
    )
}

// ============================================================================
// Server Configuration
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
/// Creates its own `SharedThrottleState` - use `run_server_with_state` to share
/// state with other services.
pub async fn run_server<M: MotorController + Send + 'static>(
    controller: ThrottleController<M>,
    config: WebServerConfig,
) -> Result<(), std::io::Error> {
    let state = Arc::new(SharedThrottleState::new(controller));
    run_server_with_state(state, config).await
}

/// Start the web server with shared state
///
/// Use this when sharing state with other services (MQTT, physical input).
pub async fn run_server_with_state<M: MotorController + Send + 'static>(
    state: Arc<SharedThrottleState<M>>,
    config: WebServerConfig,
) -> Result<(), std::io::Error> {
    let router = build_router(state, &config);
    let listener = tokio::net::TcpListener::bind(config.addr).await?;
    println!("Web server listening on http://{}", config.addr);
    axum::serve(listener, router).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::MockMotor;
    use crate::{CommandSource, Direction, ThrottleCommand};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    // ========================================================================
    // WebServerConfig tests
    // ========================================================================

    #[test]
    fn test_web_server_config_default() {
        let config = WebServerConfig::default();
        assert_eq!(config.addr.to_string(), "0.0.0.0:8080");
        assert!(config.cors_permissive);
    }

    #[test]
    fn test_web_server_config_new() {
        let addr: SocketAddr = "127.0.0.1:3000".parse().unwrap();
        let config = WebServerConfig::new(addr);
        assert_eq!(config.addr.to_string(), "127.0.0.1:3000");
        assert!(config.cors_permissive);
    }

    #[test]
    fn test_web_server_config_cors() {
        let config = WebServerConfig::default().cors(false);
        assert!(!config.cors_permissive);
    }

    #[test]
    fn test_web_server_config_from_config() {
        let web_config = crate::config::WebConfig {
            enabled: true,
            port: 9000,
            cors_permissive: false,
            poll_interval_ms: 20,
        };
        let config = WebServerConfig::from_config(&web_config);
        assert_eq!(config.addr.port(), 9000);
        assert!(!config.cors_permissive);
    }

    // ========================================================================
    // Router tests
    // ========================================================================

    #[test]
    fn test_build_router_with_cors() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default().cors(true);
        let _router = build_router(state, &config);
    }

    #[test]
    fn test_build_router_without_cors() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default().cors(false);
        let _router = build_router(state, &config);
    }

    // ========================================================================
    // Route handler tests
    // ========================================================================

    #[tokio::test]
    async fn test_get_state() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        // Set specific state
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.5).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.update(now);
        });

        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(Request::builder().uri("/api/state").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = std::str::from_utf8(&body).unwrap();

        assert!(body_str.contains("\"speed\""));
        assert!(body_str.contains("0.5"));
    }

    #[tokio::test]
    async fn test_set_speed_valid() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state.clone(), &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"speed": 0.75}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());
        let current = state.state();
        assert!((current.speed - 0.75).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_set_speed_invalid_range_high() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"speed": 1.5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Bad request returns 400
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_set_speed_invalid_range_negative() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"speed": -0.1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_set_speed_invalid_json() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/speed")
                    .header("content-type", "application/json")
                    .body(Body::from("not json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_set_direction_forward() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state.clone(), &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/direction")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"direction": "forward"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.state().direction, Direction::Forward);
    }

    #[tokio::test]
    async fn test_set_direction_reverse() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state.clone(), &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/direction")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"direction": "reverse"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.state().direction, Direction::Reverse);
    }

    #[tokio::test]
    async fn test_set_direction_invalid() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/direction")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"direction": "sideways"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_emergency_stop() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        // Set initial speed
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.8).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.update(now);
        });

        let config = WebServerConfig::default();
        let app = build_router(state.clone(), &config);

        let response = app
            .oneshot(Request::builder().method("POST").uri("/api/estop").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());
        assert!(state.state().speed.abs() < 0.01);
    }

    #[tokio::test]
    async fn test_set_max_speed_valid() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state.clone(), &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/max-speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"max_speed": 0.8}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!((state.state().max_speed - 0.8).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_set_max_speed_invalid_range_high() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/max-speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"max_speed": 1.5}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_set_max_speed_invalid_range_negative() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/max-speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"max_speed": -0.1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_index_returns_html() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(body_str.contains("<!DOCTYPE html>") || body_str.contains("<html"));
    }

    #[tokio::test]
    async fn test_not_found_handler() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();
        let app = build_router(state, &config);

        let response = app
            .oneshot(Request::builder().uri("/api/nonexistent").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_cors_headers_when_enabled() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default().cors(true);
        let app = build_router(state, &config);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header("origin", "http://example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("access-control-allow-origin"));
    }

    #[tokio::test]
    async fn test_multiple_commands_sequential() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = WebServerConfig::default();

        // Set direction
        let app = build_router(state.clone(), &config);
        let _ = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/direction")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"direction": "forward"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        // Set speed
        let app = build_router(state.clone(), &config);
        let _ = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/speed")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"speed": 0.6}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
        assert!((current.speed - 0.6).abs() < 0.01);
    }
}
