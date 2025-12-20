//! Integration tests for the web API.
//!
//! These tests verify the HTTP API endpoints work correctly.

#![cfg(feature = "web")]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use rs_trainz::hal::MockMotor;
use rs_trainz::services::{build_router, ApiResponse, AppState, StateResponse, WebServerConfig};
use rs_trainz::ThrottleController;

fn create_test_app() -> (axum::Router, Arc<AppState<MockMotor>>) {
    let motor = MockMotor::new();
    let controller = ThrottleController::new(motor);
    let state = Arc::new(AppState::new(controller));
    let config = WebServerConfig::default();
    let router = build_router(Arc::clone(&state), &config);
    (router, state)
}

#[tokio::test]
async fn test_get_state() {
    let (app, _state) = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: ApiResponse<StateResponse> = serde_json::from_slice(&body).unwrap();

    assert!(json.success);
    assert!(json.data.is_some());

    let data = json.data.unwrap();
    assert_eq!(data.speed, 0.0);
    assert_eq!(data.direction, rs_trainz::Direction::Stopped);
    assert_eq!(data.max_speed, 1.0);
    assert!(!data.transitioning);
}

#[tokio::test]
async fn test_set_speed_immediate() {
    let (app, state) = create_test_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"speed": 0.5, "duration_ms": 0}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Update controller to apply the command
    let now_ms = state.now_ms();
    state.with_controller(|controller| {
        controller.update(now_ms).unwrap();
    });

    // Check state
    let current_speed = state.with_controller(|controller| controller.current_speed());
    assert!((current_speed - 0.5).abs() < 0.01);
}

#[tokio::test]
async fn test_set_speed_with_transition() {
    let (app, state) = create_test_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("Content-Type", "application/json")
                .body(Body::from(
                    r#"{"speed": 1.0, "duration_ms": 1000, "smooth": false}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Check that transition started
    let is_transitioning = state.with_controller(|controller| controller.is_transitioning());
    assert!(is_transitioning);
}

#[tokio::test]
async fn test_set_speed_validation() {
    let (app, _state) = create_test_app();

    // Test invalid speed > 1.0
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/speed")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"speed": 1.5, "duration_ms": 0}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: ApiResponse<()> = serde_json::from_slice(&body).unwrap();
    assert!(!json.success);
    assert!(json.error.is_some());
}

#[tokio::test]
async fn test_set_direction() {
    let (app, state) = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/direction")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"direction": "forward"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let direction = state.with_controller(|controller| controller.current_direction());
    assert_eq!(direction, rs_trainz::Direction::Forward);
}

#[tokio::test]
async fn test_emergency_stop() {
    let (app, state) = create_test_app();

    // First set some speed
    let now_ms = state.now_ms();
    state.with_controller(|controller| {
        let cmd = rs_trainz::ThrottleCommand::speed_immediate(0.8).into();
        controller
            .apply_command(cmd, rs_trainz::CommandSource::WebApi, now_ms)
            .unwrap();
        controller.update(now_ms).unwrap();
    });

    // Then e-stop
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/estop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let (speed, direction) = state
        .with_controller(|controller| (controller.current_speed(), controller.current_direction()));
    assert_eq!(speed, 0.0);
    assert_eq!(direction, rs_trainz::Direction::Stopped);
}

#[tokio::test]
async fn test_set_max_speed() {
    let (app, state) = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/max-speed")
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"max_speed": 0.7}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let throttle_state = state.state();
    assert!((throttle_state.max_speed - 0.7).abs() < 0.01);
}

#[tokio::test]
async fn test_index_serves_html() {
    let (app, _state) = create_test_app();

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();

    assert!(html.contains("rs-trainz"));
    assert!(html.contains("alpine"));
}

#[tokio::test]
async fn test_not_found() {
    let (app, _state) = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
