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

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // HttpMethod Tests
    // =========================================================================

    #[test]
    fn http_method_clone() {
        let method = HttpMethod::Get;
        let cloned = method.clone();
        assert_eq!(method, cloned);
    }

    #[test]
    fn http_method_copy() {
        let method = HttpMethod::Post;
        let copied = method;
        assert_eq!(method, copied);
    }

    #[test]
    fn http_method_debug() {
        assert_eq!(format!("{:?}", HttpMethod::Get), "Get");
        assert_eq!(format!("{:?}", HttpMethod::Post), "Post");
        assert_eq!(format!("{:?}", HttpMethod::Put), "Put");
        assert_eq!(format!("{:?}", HttpMethod::Delete), "Delete");
    }

    #[test]
    fn http_method_equality() {
        assert_eq!(HttpMethod::Get, HttpMethod::Get);
        assert_eq!(HttpMethod::Post, HttpMethod::Post);
        assert_eq!(HttpMethod::Put, HttpMethod::Put);
        assert_eq!(HttpMethod::Delete, HttpMethod::Delete);
        assert_ne!(HttpMethod::Get, HttpMethod::Post);
        assert_ne!(HttpMethod::Put, HttpMethod::Delete);
    }

    // =========================================================================
    // MqttMessage Tests
    // =========================================================================

    #[test]
    fn mqtt_message_new_with_string() {
        let msg = MqttMessage::new("test/topic", b"payload".to_vec());
        assert_eq!(msg.topic, "test/topic");
        assert_eq!(msg.payload, b"payload");
    }

    #[test]
    fn mqtt_message_new_with_str() {
        let msg = MqttMessage::new("another/topic", "text payload");
        assert_eq!(msg.topic, "another/topic");
        assert_eq!(msg.payload, b"text payload");
    }

    #[test]
    fn mqtt_message_payload_str_valid_utf8() {
        let msg = MqttMessage::new("topic", b"hello world");
        assert_eq!(msg.payload_str(), Some("hello world"));
    }

    #[test]
    fn mqtt_message_payload_str_invalid_utf8() {
        let msg = MqttMessage {
            topic: "topic".into(),
            payload: vec![0xFF, 0xFE, 0xFD], // Invalid UTF-8
        };
        assert_eq!(msg.payload_str(), None);
    }

    #[test]
    fn mqtt_message_clone() {
        let msg = MqttMessage::new("topic", b"data");
        let cloned = msg.clone();
        assert_eq!(msg.topic, cloned.topic);
        assert_eq!(msg.payload, cloned.payload);
    }

    #[test]
    fn mqtt_message_debug() {
        let msg = MqttMessage::new("test", vec![1, 2, 3]);
        let debug_str = format!("{:?}", msg);
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("[1, 2, 3]"));
    }

    // =========================================================================
    // HttpRequest Tests
    // =========================================================================

    #[test]
    fn http_request_body_str_valid_utf8() {
        let req = HttpRequest {
            method: HttpMethod::Post,
            path: "/api/test".into(),
            body: Some(b"hello".to_vec()),
        };
        assert_eq!(req.body_str(), Some("hello"));
    }

    #[test]
    fn http_request_body_str_no_body() {
        let req = HttpRequest {
            method: HttpMethod::Get,
            path: "/api/test".into(),
            body: None,
        };
        assert_eq!(req.body_str(), None);
    }

    #[test]
    fn http_request_body_str_invalid_utf8() {
        let req = HttpRequest {
            method: HttpMethod::Post,
            path: "/api/test".into(),
            body: Some(vec![0xFF, 0xFE, 0xFD]), // Invalid UTF-8
        };
        assert_eq!(req.body_str(), None);
    }

    #[test]
    fn http_request_debug() {
        let req = HttpRequest {
            method: HttpMethod::Get,
            path: "/test".into(),
            body: None,
        };
        let debug_str = format!("{:?}", req);
        assert!(debug_str.contains("Get"));
        assert!(debug_str.contains("/test"));
    }

    // =========================================================================
    // HttpResponse Tests
    // =========================================================================

    #[test]
    fn http_response_ok_json() {
        let response = HttpResponse::ok_json(r#"{"status":"ok"}"#);
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(response.body, br#"{"status":"ok"}"#);
    }

    #[test]
    fn http_response_ok_html() {
        let response = HttpResponse::ok_html("<h1>Hello</h1>");
        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/html");
        assert_eq!(response.body, b"<h1>Hello</h1>");
    }

    #[test]
    fn http_response_error() {
        let response = HttpResponse::error(500, "internal error");
        assert_eq!(response.status, 500);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(response.body, br#"{"error":"internal error"}"#);
    }

    #[test]
    fn http_response_not_found() {
        let response = HttpResponse::not_found();
        assert_eq!(response.status, 404);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(response.body, br#"{"error":"not found"}"#);
    }

    #[test]
    fn http_response_bad_request() {
        let response = HttpResponse::bad_request("invalid input");
        assert_eq!(response.status, 400);
        assert_eq!(response.content_type, "application/json");
        assert_eq!(response.body, br#"{"error":"invalid input"}"#);
    }

    #[test]
    fn http_response_debug() {
        let response = HttpResponse::ok_json("{}");
        let debug_str = format!("{:?}", response);
        assert!(debug_str.contains("200"));
        assert!(debug_str.contains("application/json"));
    }

    #[test]
    fn http_response_empty_json() {
        let response = HttpResponse::ok_json("");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"");
    }

    #[test]
    fn http_response_multiline_html() {
        let html = "<html>\n  <body>Test</body>\n</html>";
        let response = HttpResponse::ok_html(html);
        assert_eq!(response.status, 200);
        assert_eq!(response.body, html.as_bytes());
    }

    #[test]
    fn http_response_error_with_special_chars() {
        let response = HttpResponse::error(403, "access denied: \"admin\" required");
        assert_eq!(response.status, 403);
        // The format! macro doesn't escape quotes in the string
        assert_eq!(
            response.body,
            br#"{"error":"access denied: "admin" required"}"#
        );
    }
}
