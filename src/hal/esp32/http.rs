//! HTTP server for ESP32-C3 web API.
//!
//! Provides a lightweight HTTP server using esp-idf-svc for serving
//! the throttle control API and web UI. Uses shared JSON helpers from
//! the `http_handler` module for consistent response formats.
//!
//! # Endpoints
//!
//! - `GET /api/state` - Get current throttle state (JSON)
//! - `POST /api/speed` - Set speed `{"speed": 0.5}`
//! - `POST /api/direction` - Set direction `{"direction": "forward"|"reverse"}`
//! - `POST /api/estop` - Emergency stop
//! - `GET /` - Web UI (serves embedded HTML)
//!
//! # Example
//!
//! ```ignore
//! use rs_trainz::hal::esp32::{Esp32HttpServer, Esp32SharedState};
//! use rs_trainz::config::WebConfig;
//! use std::sync::{Arc, Mutex};
//!
//! let shared = Arc::new(Mutex::new(Esp32SharedState::default()));
//! let config = WebConfig::default().with_port(80);
//! let server = Esp32HttpServer::new(&config, shared)?;
//! ```

use crate::config::WebConfig;
use crate::parsing::{parse_direction_json, parse_speed_json};
use crate::{ThrottleCommand, ThrottleCommandDyn, ThrottleState};
use esp_idf_hal::io::Write;
use esp_idf_svc::http::server::{Configuration, EspHttpServer};
use esp_idf_svc::io::EspIOError;
use std::sync::{Arc, Mutex};

// Import shared helpers from http_handler (when available)
#[cfg(any(feature = "web", feature = "mqtt"))]
use crate::services::http_handler::{direction_str, state_to_json};

// Fallback for when services aren't available
#[cfg(not(any(feature = "web", feature = "mqtt")))]
fn direction_str(dir: &crate::Direction) -> &'static str {
    match dir {
        crate::Direction::Forward => "forward",
        crate::Direction::Reverse => "reverse",
        crate::Direction::Stopped => "stopped",
    }
}

#[cfg(not(any(feature = "web", feature = "mqtt")))]
fn state_to_json(state: &ThrottleState) -> String {
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

/// HTTP server for throttle control API.
///
/// Runs an embedded HTTP server that exposes REST endpoints for
/// controlling the throttle and retrieving state.
pub struct Esp32HttpServer {
    _server: EspHttpServer<'static>,
}

/// Shared state between HTTP handlers and main loop (ESP32-specific).
///
/// This struct uses a command queue pattern suitable for ESP32's
/// callback-based HTTP server. The main loop should:
/// 1. Update `state` and `now_ms` regularly
/// 2. Check and consume `pending_command` when present
///
/// Note: This is different from `services::SharedThrottleState` which
/// wraps a full `ThrottleController`. This ESP32 variant is designed
/// for the callback-based esp-idf-svc HTTP handlers.
pub struct Esp32SharedState {
    /// Current throttle state snapshot
    pub state: ThrottleState,
    /// Pending command from HTTP (consumed by main loop)
    pub pending_command: Option<ThrottleCommandDyn>,
    /// Current timestamp in milliseconds
    pub now_ms: u64,
}

impl Default for Esp32SharedState {
    fn default() -> Self {
        Self {
            state: ThrottleState::default(),
            pending_command: None,
            now_ms: 0,
        }
    }
}

/// Type alias for backward compatibility.
#[deprecated(since = "0.2.0", note = "Use Esp32SharedState instead")]
pub type SharedThrottleState = Esp32SharedState;

impl Esp32HttpServer {
    /// Create a new HTTP server.
    ///
    /// The server shares state via the provided `Arc<Mutex<Esp32SharedState>>`.
    /// The main loop should:
    /// 1. Update `state` and `now_ms` regularly
    /// 2. Check and consume `pending_command` when present
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP server fails to start.
    pub fn new(
        config: &WebConfig,
        shared_state: Arc<Mutex<Esp32SharedState>>,
    ) -> anyhow::Result<Self> {
        let server_config = Configuration {
            http_port: config.port,
            ..Default::default()
        };

        let mut server = EspHttpServer::new(&server_config)?;

        // Clone Arc for each handler
        let state_for_get = shared_state.clone();
        let state_for_speed = shared_state.clone();
        let state_for_dir = shared_state.clone();
        let state_for_estop = shared_state.clone();

        // GET /api/state - Return current throttle state
        server.fn_handler("/api/state", esp_idf_svc::http::Method::Get, move |req| {
            let state = state_for_get.lock().unwrap();
            let json = state_to_json(&state.state);
            let mut resp = req.into_ok_response()?;
            resp.write_all(json.as_bytes())?;
            Ok::<_, EspIOError>(())
        })?;

        // POST /api/speed - Set speed
        server.fn_handler(
            "/api/speed",
            esp_idf_svc::http::Method::Post,
            move |mut req| {
                let mut buf = [0u8; 128];
                let len = req.read(&mut buf).unwrap_or(0);
                let body = core::str::from_utf8(&buf[..len]).unwrap_or("");

                if let Some(speed) = parse_speed_json(body) {
                    if (0.0..=1.0).contains(&speed) {
                        let mut state = state_for_speed.lock().unwrap();
                        state.pending_command =
                            Some(ThrottleCommand::speed_immediate(speed).into());
                        let mut resp = req.into_ok_response()?;
                        resp.write_all(b"{\"ok\":true,\"result\":\"applied\"}")?;
                    } else {
                        let mut resp =
                            req.into_response(400, None, &[("Content-Type", "application/json")])?;
                        resp.write_all(b"{\"error\":\"speed must be between 0.0 and 1.0\"}")?;
                    }
                } else {
                    let mut resp =
                        req.into_response(400, None, &[("Content-Type", "application/json")])?;
                    resp.write_all(b"{\"error\":\"invalid speed\"}")?;
                }
                Ok::<_, EspIOError>(())
            },
        )?;

        // POST /api/direction - Set direction
        server.fn_handler(
            "/api/direction",
            esp_idf_svc::http::Method::Post,
            move |mut req| {
                let mut buf = [0u8; 128];
                let len = req.read(&mut buf).unwrap_or(0);
                let body = core::str::from_utf8(&buf[..len]).unwrap_or("");

                if let Some(dir) = parse_direction_json(body) {
                    let mut state = state_for_dir.lock().unwrap();
                    state.pending_command = Some(ThrottleCommandDyn::SetDirection(dir));
                    let mut resp = req.into_ok_response()?;
                    resp.write_all(b"{\"ok\":true,\"result\":\"direction_set\"}")?;
                } else {
                    let mut resp =
                        req.into_response(400, None, &[("Content-Type", "application/json")])?;
                    resp.write_all(b"{\"error\":\"invalid direction\"}")?;
                }
                Ok::<_, EspIOError>(())
            },
        )?;

        // POST /api/estop - Emergency stop
        server.fn_handler("/api/estop", esp_idf_svc::http::Method::Post, move |req| {
            let mut state = state_for_estop.lock().unwrap();
            state.pending_command = Some(ThrottleCommandDyn::EmergencyStop);
            let mut resp = req.into_ok_response()?;
            resp.write_all(b"{\"ok\":true,\"result\":\"emergency_stop\"}")?;
            Ok::<_, EspIOError>(())
        })?;

        // GET / - Serve web UI (shared with desktop)
        server.fn_handler("/", esp_idf_svc::http::Method::Get, move |req| {
            let html = include_str!("../../../www/index.html");
            let mut resp = req.into_response(200, None, &[("Content-Type", "text/html")])?;
            resp.write_all(html.as_bytes())?;
            Ok::<_, EspIOError>(())
        })?;

        println!("[HTTP] Server started on port {}", config.port);

        Ok(Self { _server: server })
    }
}
