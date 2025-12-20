//! Network services for HTTP API and MQTT integration.
//!
//! This module provides optional network connectivity for the throttle controller:
//! - `web` feature: Axum-based HTTP API server with JSON endpoints
//! - `mqtt` feature: MQTT client for pub/sub messaging
//!
//! Both services integrate with the core `ThrottleController` through a unified
//! shared state pattern using `SharedThrottleState<M>` wrapped in `Arc` for
//! thread-safe access across all services.
//!
//! # Shared State Pattern
//!
//! For real-time model train control, all services should share a single
//! `ThrottleController` instance via `SharedThrottleState`:
//!
//! ```ignore
//! use std::sync::Arc;
//! use rs_trainz::services::SharedThrottleState;
//!
//! // Create single shared state
//! let state = Arc::new(SharedThrottleState::new(controller));
//!
//! // Web and MQTT both use the same state
//! let web_router = build_router(Arc::clone(&state), &web_config);
//! let mqtt_handler = MqttHandler::with_shared_state(Arc::clone(&state), mqtt_config);
//! ```

// Shared state (available when either web or mqtt is enabled)
#[cfg(any(feature = "web", feature = "mqtt"))]
pub mod shared;

// API types are shared between web and mqtt
#[cfg(any(feature = "web", feature = "mqtt"))]
pub mod api;

// HTTP handler logic (shared between desktop and ESP32)
#[cfg(any(feature = "web", feature = "mqtt"))]
pub mod http_handler;

#[cfg(feature = "web")]
pub mod web;

#[cfg(feature = "mqtt")]
pub mod mqtt;

// MQTT service runner (platform-agnostic)
#[cfg(any(feature = "web", feature = "mqtt"))]
pub mod mqtt_runner;

// Physical input handler (requires std for Arc)
#[cfg(any(feature = "web", feature = "mqtt"))]
pub mod physical;

// Re-exports
#[cfg(any(feature = "web", feature = "mqtt"))]
pub use shared::*;

#[cfg(any(feature = "web", feature = "mqtt"))]
pub use api::*;

#[cfg(any(feature = "web", feature = "mqtt"))]
pub use http_handler::*;

#[cfg(feature = "web")]
pub use web::*;

#[cfg(feature = "mqtt")]
pub use mqtt::*;

#[cfg(any(feature = "web", feature = "mqtt"))]
pub use mqtt_runner::*;

#[cfg(any(feature = "web", feature = "mqtt"))]
pub use physical::*;
