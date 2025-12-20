//! Network abstraction traits for MQTT and HTTP.
//!
//! This module defines traits for network connectivity, enabling the throttle
//! controller to be accessed remotely via MQTT pub/sub and HTTP REST APIs.
//!
//! # Traits
//!
//! | Trait | Purpose |
//! |-------|---------|
//! | [`MqttClient`] | Pub/sub messaging for home automation |
//! | [`HttpServer`] | REST API for web UI and programmatic control |
//!
//! # MQTT Integration
//!
//! MQTT is ideal for integration with home automation systems like
//! Home Assistant or Node-RED:
//!
//! ```text
//! train/speed      - Current speed (0-100)
//! train/speed/set  - Set speed command
//! train/direction  - Current direction (forward/reverse/stopped)
//! train/estop      - Emergency stop trigger
//! ```
//!
//! # HTTP API
//!
//! The HTTP server provides a REST API for web-based control:
//!
//! ```text
//! GET  /api/state     - Get current throttle state
//! POST /api/speed     - Set speed: {"speed": 0.5}
//! POST /api/direction - Set direction: {"direction": "forward"}
//! POST /api/estop     - Trigger emergency stop
//! ```

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================================
// MQTT Client Trait (Sync-First Design)
// ============================================================================

/// MQTT client trait for pub/sub messaging.
///
/// This trait uses a **sync-first design** that works on both ESP32 (blocking I/O)
/// and desktop (can be wrapped in async). The design prioritizes embedded compatibility
/// while still allowing async wrappers for desktop use.
///
/// # Implementation Notes
///
/// - `publish` and `subscribe` are synchronous (blocking on ESP32)
/// - `try_recv` is non-blocking for polling patterns
/// - Implement `MqttClientAsync` for async desktop usage
/// - The client should handle reconnection internally
///
/// # Example
///
/// ```rust,ignore
/// use rs_trainz::traits::MqttClient;
///
/// fn publish_state<M: MqttClient>(client: &mut M, speed: f32) {
///     let payload = format!("{:.0}", speed * 100.0);
///     client.publish("train/speed", payload.as_bytes(), true).unwrap();
/// }
/// ```
pub trait MqttClient {
    /// Error type for MQTT operations.
    type Error;

    /// Publish a message to a topic (blocking).
    ///
    /// # Arguments
    /// - `topic`: MQTT topic path
    /// - `payload`: Message bytes
    /// - `retain`: If true, broker keeps message for new subscribers
    fn publish(&mut self, topic: &str, payload: &[u8], retain: bool) -> Result<(), Self::Error>;

    /// Subscribe to a topic (blocking).
    ///
    /// Supports wildcards: `train/#` or `train/+/set`
    fn subscribe(&mut self, topic: &str) -> Result<(), Self::Error>;

    /// Try to receive the next message (non-blocking).
    ///
    /// Returns `None` if no message is available. This should never block.
    fn try_recv(&mut self) -> Option<MqttMessage>;

    /// Check if connected to broker.
    fn is_connected(&self) -> bool;
}

/// Async extension trait for MQTT clients (desktop/tokio usage).
///
/// This trait extends `MqttClient` with async methods for use with
/// async runtimes like tokio. Desktop implementations can implement
/// both traits while ESP32 only needs the sync `MqttClient`.
#[cfg(feature = "std")]
pub trait MqttClientAsync: MqttClient {
    /// Publish a message asynchronously.
    fn publish_async(
        &mut self,
        topic: &str,
        payload: &[u8],
        retain: bool,
    ) -> impl core::future::Future<Output = Result<(), Self::Error>>;

    /// Subscribe to a topic asynchronously.
    fn subscribe_async(
        &mut self,
        topic: &str,
    ) -> impl core::future::Future<Output = Result<(), Self::Error>>;

    /// Receive the next message asynchronously (waits for message).
    fn recv_async(&mut self) -> impl core::future::Future<Output = Option<MqttMessage>>;
}

/// An MQTT message received from a subscription.
///
/// Contains the topic and payload of a published message.
#[derive(Clone, Debug)]
pub struct MqttMessage {
    /// Topic the message was published to.
    pub topic: String,
    /// Message payload as raw bytes.
    pub payload: Vec<u8>,
}

impl MqttMessage {
    /// Create a new MQTT message.
    pub fn new(topic: impl Into<String>, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            topic: topic.into(),
            payload: payload.into(),
        }
    }

    /// Returns the payload as a UTF-8 string, if valid.
    pub fn payload_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.payload).ok()
    }
}

// ============================================================================
// HTTP Server Trait
// ============================================================================

/// HTTP server trait for web UI and REST API.
///
/// Provides a simple async HTTP server interface for serving the web UI
/// and handling API requests.
///
/// # Implementation Notes
///
/// - `recv_request` should block until a request arrives
/// - `send_response` must complete the HTTP transaction
/// - For production, consider using axum or similar frameworks
///
/// Note: ESP32 uses a callback-based HTTP server that doesn't fit this model.
/// For ESP32, use `HttpApiHandler` directly with esp-idf-svc callbacks instead.
pub trait HttpServer {
    /// Error type for HTTP operations.
    type Error;

    /// Wait for and receive the next HTTP request.
    ///
    /// Returns `None` if the server is shutting down.
    fn recv_request(&mut self) -> impl core::future::Future<Output = Option<HttpRequest>>;

    /// Send an HTTP response for the current request.
    fn send_response(
        &mut self,
        response: HttpResponse,
    ) -> impl core::future::Future<Output = Result<(), Self::Error>>;
}

/// HTTP request methods.
///
/// Standard HTTP methods used by the REST API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    /// HTTP GET request (retrieve state).
    Get,
    /// HTTP POST request (create/action).
    Post,
    /// HTTP PUT request (update).
    Put,
    /// HTTP DELETE request (remove).
    Delete,
}

/// An HTTP request received by the server.
///
/// Contains the method, path, and optional body for processing.
#[derive(Debug)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, etc.).
    pub method: HttpMethod,
    /// Request path (e.g., "/api/state").
    pub path: String,
    /// Request body, if present (for POST/PUT).
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    /// Returns the body as a UTF-8 string, if valid.
    pub fn body_str(&self) -> Option<&str> {
        self.body
            .as_ref()
            .and_then(|b| core::str::from_utf8(b).ok())
    }
}

/// An HTTP response to send to the client.
///
/// Helper methods are provided for common response types.
#[derive(Debug)]
pub struct HttpResponse {
    /// HTTP status code (e.g., 200, 404, 500).
    pub status: u16,
    /// Content-Type header value.
    pub content_type: &'static str,
    /// Response body as bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Creates a 200 OK response with JSON content.
    pub fn ok_json(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: body.as_bytes().to_vec(),
        }
    }

    /// Creates a 200 OK response with HTML content.
    pub fn ok_html(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "text/html",
            body: body.as_bytes().to_vec(),
        }
    }

    /// Creates an error response with the given status code.
    pub fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: alloc::format!(r#"{{"error":"{}"}}"#, message).into_bytes(),
        }
    }

    /// Creates a 404 Not Found response.
    pub fn not_found() -> Self {
        Self::error(404, "not found")
    }

    /// Creates a 400 Bad Request response.
    pub fn bad_request(message: &str) -> Self {
        Self::error(400, message)
    }
}
