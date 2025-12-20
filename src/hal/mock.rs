//! Mock implementations for testing without hardware.
//!
//! This module provides test doubles for all hardware and network traits,
//! enabling development and testing on desktop without physical hardware.
//!
//! # Available Mocks
//!
//! | Mock | Trait | Purpose |
//! |------|-------|---------|
//! | [`MockMotor`] | [`MotorController`] | Tracks speed/direction calls |
//! | [`MockEncoder`] | [`EncoderInput`] | Queued delta values and button state |
//! | [`MockFault`] | [`FaultDetector`] | Simulates fault conditions |
//! | [`MockClock`] | [`Clock`] | Controllable time source |
//! | [`MockDisplay`] | [`ThrottleDisplay`] | Tracks render calls |
//! | [`MockMqtt`] | [`MqttClient`] | Captures pub/sub operations |
//! | [`MockHttp`] | [`HttpServer`] | Queued request/response |
//!
//! # Example
//!
//! ```rust
//! use rs_trainz::{ThrottleController, ThrottleCommand, CommandSource};
//! use rs_trainz::hal::MockMotor;
//! use rs_trainz::traits::MotorController;
//!
//! // Create controller with mock motor
//! let motor = MockMotor::new();
//! let mut controller = ThrottleController::new(motor);
//!
//! // Apply command
//! let cmd = ThrottleCommand::speed_immediate(0.5);
//! controller.apply_command(cmd.into(), CommandSource::Physical, 0).unwrap();
//! controller.update(0).unwrap();
//!
//! // Verify via state
//! let state = controller.state(0);
//! assert!((state.speed - 0.5).abs() < 0.01);
//! ```
//!
//! [`MotorController`]: crate::traits::MotorController
//! [`EncoderInput`]: crate::traits::EncoderInput
//! [`FaultDetector`]: crate::traits::FaultDetector
//! [`Clock`]: crate::traits::Clock
//! [`ThrottleDisplay`]: crate::traits::ThrottleDisplay
//! [`MqttClient`]: crate::traits::MqttClient
//! [`HttpServer`]: crate::traits::HttpServer

use crate::traits::{
    Clock, Direction, EncoderInput, FaultDetector, HttpRequest, HttpResponse, HttpServer,
    MotorController, MqttClient, MqttMessage,
};

#[cfg(feature = "std")]
use crate::traits::MqttClientAsync;

// ============================================================================
// Hardware Mocks
// ============================================================================

/// Mock motor controller for testing.
///
/// Records all speed and direction changes for verification. Use the
/// public fields to inspect state after test operations.
///
/// # Example
///
/// ```rust
/// use rs_trainz::hal::MockMotor;
/// use rs_trainz::traits::{MotorController, Direction};
///
/// let mut motor = MockMotor::new();
/// motor.set_speed(0.75).unwrap();
/// motor.set_direction(Direction::Forward).unwrap();
///
/// assert_eq!(motor.speed, 0.75);
/// assert_eq!(motor.direction, Direction::Forward);
/// assert_eq!(motor.call_count, 1); // set_speed increments
/// ```
#[derive(Debug, Default)]
pub struct MockMotor {
    /// Current speed setting (0.0 to 1.0).
    pub speed: f32,
    /// Current direction.
    pub direction: Direction,
    /// Simulated motor current in milliamps.
    pub current_ma: u32,
    /// Number of times `set_speed` was called.
    pub call_count: usize,
}

impl MockMotor {
    /// Creates a new mock motor with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a mock motor with the specified current reading.
    pub fn with_current(mut self, current_ma: u32) -> Self {
        self.current_ma = current_ma;
        self
    }
}

impl MotorController for MockMotor {
    type Error = ();

    fn set_speed(&mut self, speed: f32) -> Result<(), ()> {
        self.speed = speed;
        self.call_count += 1;
        Ok(())
    }

    fn set_direction(&mut self, dir: Direction) -> Result<(), ()> {
        self.direction = dir;
        Ok(())
    }

    fn read_current_ma(&self) -> Result<Option<u32>, ()> {
        Ok(Some(self.current_ma))
    }
}

/// Mock encoder for testing.
///
/// Simulates a rotary encoder with push button. Queue delta values
/// to simulate rotation, and control button state directly.
///
/// # Example
///
/// ```rust
/// use rs_trainz::hal::MockEncoder;
/// use rs_trainz::traits::EncoderInput;
///
/// let mut encoder = MockEncoder::new();
///
/// // Simulate rotation
/// encoder.queue_delta(5);  // 5 clicks clockwise
/// encoder.queue_delta(-3); // 3 clicks counter-clockwise
///
/// // Deltas come out in LIFO order
/// assert_eq!(encoder.read_delta(), -3);
/// assert_eq!(encoder.read_delta(), 5);
/// assert_eq!(encoder.read_delta(), 0); // Empty
///
/// // Simulate button press
/// encoder.press_button();
/// assert!(encoder.button_just_pressed()); // Once
/// assert!(!encoder.button_just_pressed()); // Consumed
/// assert!(encoder.button_pressed()); // Still held
/// ```
#[derive(Debug, Default)]
pub struct MockEncoder {
    delta_queue: Vec<i32>,
    button_state: bool,
    button_just_pressed_state: bool,
}

impl MockEncoder {
    /// Creates a new mock encoder with no pending deltas.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue up encoder deltas to be returned
    pub fn queue_delta(&mut self, delta: i32) {
        self.delta_queue.push(delta);
    }

    /// Queue up multiple deltas
    pub fn queue_deltas(&mut self, deltas: &[i32]) {
        self.delta_queue.extend_from_slice(deltas);
    }

    /// Set the button state
    pub fn set_button(&mut self, pressed: bool) {
        self.button_state = pressed;
    }

    /// Simulate a button press (just_pressed will be true once)
    pub fn press_button(&mut self) {
        self.button_state = true;
        self.button_just_pressed_state = true;
    }
}

impl EncoderInput for MockEncoder {
    fn read_delta(&mut self) -> i32 {
        self.delta_queue.pop().unwrap_or(0)
    }

    fn button_pressed(&self) -> bool {
        self.button_state
    }

    fn button_just_pressed(&mut self) -> bool {
        let was_pressed = self.button_just_pressed_state;
        self.button_just_pressed_state = false;
        was_pressed
    }
}

/// Mock fault detector for testing.
///
/// Simulates fault conditions for testing error handling.
///
/// # Example
///
/// ```rust
/// use rs_trainz::hal::MockFault;
/// use rs_trainz::traits::{FaultDetector, FaultKind};
///
/// let mut fault = MockFault::new();
/// assert!(fault.active_fault().is_none());
///
/// fault.trigger_overcurrent(1500);
/// assert_eq!(fault.active_fault(), Some(FaultKind::Overcurrent));
/// assert_eq!(fault.fault_current_ma(), Some(1500));
///
/// fault.clear();
/// assert!(fault.active_fault().is_none());
/// ```
#[derive(Debug, Default)]
pub struct MockFault {
    /// Whether a short circuit fault is active.
    pub short_circuit: bool,
    /// Whether an overcurrent fault is active.
    pub overcurrent: bool,
    /// Current reading during fault (milliamps).
    pub current_ma: Option<u32>,
}

impl MockFault {
    /// Creates a new mock fault detector with no active faults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Triggers a short circuit fault condition.
    pub fn trigger_short(&mut self) {
        self.short_circuit = true;
    }

    /// Triggers an overcurrent fault condition.
    pub fn trigger_overcurrent(&mut self, current_ma: u32) {
        self.overcurrent = true;
        self.current_ma = Some(current_ma);
    }

    /// Clears all fault conditions.
    pub fn clear(&mut self) {
        self.short_circuit = false;
        self.overcurrent = false;
        self.current_ma = None;
    }
}

impl FaultDetector for MockFault {
    fn is_short_circuit(&self) -> bool {
        self.short_circuit
    }

    fn is_overcurrent(&self) -> bool {
        self.overcurrent
    }

    fn fault_current_ma(&self) -> Option<u32> {
        self.current_ma
    }
}

/// Mock clock for testing.
///
/// Provides a controllable time source for testing time-dependent behavior.
///
/// # Example
///
/// ```rust
/// use rs_trainz::hal::MockClock;
/// use rs_trainz::traits::Clock;
///
/// let mut clock = MockClock::new();
/// assert_eq!(clock.now_ms(), 0);
///
/// clock.set(1000);
/// assert_eq!(clock.now_ms(), 1000);
///
/// clock.advance(500);
/// assert_eq!(clock.now_ms(), 1500);
/// ```
#[derive(Debug)]
pub struct MockClock {
    current_ms: u64,
}

impl MockClock {
    /// Creates a new mock clock starting at 0ms.
    pub fn new() -> Self {
        Self { current_ms: 0 }
    }

    /// Sets the current time in milliseconds.
    pub fn set(&mut self, ms: u64) {
        self.current_ms = ms;
    }

    /// Advances the clock by the given duration.
    pub fn advance(&mut self, ms: u64) {
        self.current_ms += ms;
    }
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MockClock {
    fn now_ms(&self) -> u64 {
        self.current_ms
    }
}

// ============================================================================
// Network Mocks
// ============================================================================

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// Mock MQTT client for testing.
///
/// Records all publish/subscribe operations and allows injecting
/// incoming messages for testing message handling.
///
/// # Example
///
/// ```rust
/// use rs_trainz::hal::MockMqtt;
///
/// let mut mqtt = MockMqtt::new();
///
/// // Queue incoming message
/// mqtt.queue_message("train/speed/set", b"75".to_vec());
///
/// // Check subscriptions
/// mqtt.subscriptions.push("train/#".into());
/// assert!(mqtt.is_subscribed("train/#"));
///
/// // Check published messages
/// mqtt.published.push(("train/speed".into(), b"50".to_vec(), true));
/// assert_eq!(mqtt.published_to("train/speed").len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct MockMqtt {
    /// Messages that have been published (topic, payload, retain).
    pub published: Vec<(String, Vec<u8>, bool)>,
    /// Topics that have been subscribed to.
    pub subscriptions: Vec<String>,
    /// Queue of incoming messages to be returned by `recv()`.
    pub incoming: Vec<MqttMessage>,
    /// Whether the client is connected.
    pub connected: bool,
}

impl MockMqtt {
    /// Creates a new mock MQTT client in connected state.
    pub fn new() -> Self {
        Self {
            connected: true,
            ..Default::default()
        }
    }

    /// Queue an incoming message
    pub fn queue_message(&mut self, topic: impl Into<String>, payload: impl Into<Vec<u8>>) {
        self.incoming.push(MqttMessage {
            topic: topic.into(),
            payload: payload.into(),
        });
    }

    /// Check if a topic was subscribed to
    pub fn is_subscribed(&self, topic: &str) -> bool {
        self.subscriptions.iter().any(|t| t == topic)
    }

    /// Get published messages for a topic
    pub fn published_to(&self, topic: &str) -> Vec<&(String, Vec<u8>, bool)> {
        self.published
            .iter()
            .filter(|(t, _, _)| t == topic)
            .collect()
    }
}

impl MqttClient for MockMqtt {
    type Error = ();

    fn publish(&mut self, topic: &str, payload: &[u8], retain: bool) -> Result<(), ()> {
        self.published
            .push((topic.into(), payload.to_vec(), retain));
        Ok(())
    }

    fn subscribe(&mut self, topic: &str) -> Result<(), ()> {
        self.subscriptions.push(topic.into());
        Ok(())
    }

    fn try_recv(&mut self) -> Option<MqttMessage> {
        if self.incoming.is_empty() {
            None
        } else {
            Some(self.incoming.remove(0))
        }
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

#[cfg(feature = "std")]
impl MqttClientAsync for MockMqtt {
    async fn publish_async(&mut self, topic: &str, payload: &[u8], retain: bool) -> Result<(), ()> {
        self.publish(topic, payload, retain)
    }

    async fn subscribe_async(&mut self, topic: &str) -> Result<(), ()> {
        self.subscribe(topic)
    }

    async fn recv_async(&mut self) -> Option<MqttMessage> {
        self.try_recv()
    }
}

/// Mock HTTP server for testing.
///
/// Allows queuing requests and inspecting sent responses.
///
/// # Example
///
/// ```rust
/// use rs_trainz::hal::MockHttp;
/// use rs_trainz::traits::{HttpRequest, HttpMethod};
///
/// let mut http = MockHttp::new();
///
/// // Queue a request
/// http.queue_request(HttpRequest {
///     method: HttpMethod::Get,
///     path: "/api/state".into(),
///     body: None,
/// });
///
/// assert_eq!(http.requests.len(), 1);
/// ```
#[derive(Debug, Default)]
pub struct MockHttp {
    /// Queue of requests to be returned by `recv_request()`.
    pub requests: Vec<HttpRequest>,
    /// Responses that have been sent.
    pub responses: Vec<HttpResponse>,
}

// ============================================================================
// Display Mocks
// ============================================================================

/// Mock display for testing UI rendering.
///
/// Tracks render calls and stores the last rendered state for verification.
///
/// # Example
///
/// ```
/// use rs_trainz::hal::MockDisplay;
/// use rs_trainz::traits::ThrottleDisplay;
///
/// let mut display = MockDisplay::new();
/// display.init().unwrap();
/// assert_eq!(display.render_count, 0);
/// ```
#[derive(Debug, Default)]
pub struct MockDisplay {
    /// The last state that was rendered.
    pub last_state: Option<crate::ThrottleState>,
    /// Number of times render() was called.
    pub render_count: usize,
    /// Last message shown via show_message().
    pub last_message: Option<(String, Option<String>)>,
    /// Whether init() was called.
    pub initialized: bool,
}

impl MockDisplay {
    /// Creates a new mock display.
    pub fn new() -> Self {
        Self::default()
    }
}

impl crate::traits::ThrottleDisplay for MockDisplay {
    type Error = ();

    fn init(&mut self) -> Result<(), ()> {
        self.initialized = true;
        Ok(())
    }

    fn clear(&mut self) -> Result<(), ()> {
        self.last_state = None;
        Ok(())
    }

    fn render(&mut self, state: &crate::ThrottleState) -> Result<(), ()> {
        self.last_state = Some(state.clone());
        self.render_count += 1;
        Ok(())
    }

    fn show_message(&mut self, line1: &str, line2: Option<&str>) -> Result<(), ()> {
        self.last_message = Some((line1.into(), line2.map(Into::into)));
        Ok(())
    }
}

impl MockHttp {
    /// Creates a new mock HTTP server.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a request to be returned
    pub fn queue_request(&mut self, request: HttpRequest) {
        self.requests.push(request);
    }
}

impl HttpServer for MockHttp {
    type Error = ();

    async fn recv_request(&mut self) -> Option<HttpRequest> {
        if self.requests.is_empty() {
            None
        } else {
            Some(self.requests.remove(0))
        }
    }

    async fn send_response(&mut self, response: HttpResponse) -> Result<(), ()> {
        self.responses.push(response);
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ThrottleDisplay;
    use crate::{Direction, ThrottleState};

    // =========================================================================
    // MockMotor Tests
    // =========================================================================

    #[test]
    fn mock_motor_default() {
        let motor = MockMotor::new();
        assert_eq!(motor.speed, 0.0);
        assert_eq!(motor.direction, Direction::Stopped);
        assert_eq!(motor.current_ma, 0);
        assert_eq!(motor.call_count, 0);
    }

    #[test]
    fn mock_motor_set_speed() {
        let mut motor = MockMotor::new();
        motor.set_speed(0.75).unwrap();
        assert_eq!(motor.speed, 0.75);
        assert_eq!(motor.call_count, 1);

        motor.set_speed(0.25).unwrap();
        assert_eq!(motor.speed, 0.25);
        assert_eq!(motor.call_count, 2);
    }

    #[test]
    fn mock_motor_set_direction() {
        let mut motor = MockMotor::new();
        motor.set_direction(Direction::Forward).unwrap();
        assert_eq!(motor.direction, Direction::Forward);

        motor.set_direction(Direction::Reverse).unwrap();
        assert_eq!(motor.direction, Direction::Reverse);
    }

    #[test]
    fn mock_motor_with_current() {
        let motor = MockMotor::new().with_current(500);
        assert_eq!(motor.read_current_ma().unwrap(), Some(500));
    }

    // =========================================================================
    // MockEncoder Tests
    // =========================================================================

    #[test]
    fn mock_encoder_default() {
        let mut encoder = MockEncoder::new();
        assert_eq!(encoder.read_delta(), 0);
        assert!(!encoder.button_pressed());
        assert!(!encoder.button_just_pressed());
    }

    #[test]
    fn mock_encoder_queue_delta() {
        let mut encoder = MockEncoder::new();
        encoder.queue_delta(5);
        encoder.queue_delta(-3);

        // Deltas come out in LIFO order (pop from Vec)
        assert_eq!(encoder.read_delta(), -3);
        assert_eq!(encoder.read_delta(), 5);
        assert_eq!(encoder.read_delta(), 0); // Empty
    }

    #[test]
    fn mock_encoder_queue_deltas() {
        let mut encoder = MockEncoder::new();
        encoder.queue_deltas(&[1, 2, 3]);

        assert_eq!(encoder.read_delta(), 3);
        assert_eq!(encoder.read_delta(), 2);
        assert_eq!(encoder.read_delta(), 1);
    }

    #[test]
    fn mock_encoder_button() {
        let mut encoder = MockEncoder::new();
        assert!(!encoder.button_pressed());

        encoder.set_button(true);
        assert!(encoder.button_pressed());

        encoder.set_button(false);
        assert!(!encoder.button_pressed());
    }

    #[test]
    fn mock_encoder_button_just_pressed() {
        let mut encoder = MockEncoder::new();
        encoder.press_button();

        assert!(encoder.button_pressed());
        assert!(encoder.button_just_pressed()); // First call returns true
        assert!(!encoder.button_just_pressed()); // Second call returns false
        assert!(encoder.button_pressed()); // Still pressed though
    }

    // =========================================================================
    // MockFault Tests
    // =========================================================================

    #[test]
    fn mock_fault_default() {
        let fault = MockFault::new();
        assert!(!fault.is_short_circuit());
        assert!(!fault.is_overcurrent());
        assert!(fault.fault_current_ma().is_none());
    }

    #[test]
    fn mock_fault_trigger_short() {
        let mut fault = MockFault::new();
        fault.trigger_short();
        assert!(fault.is_short_circuit());
        assert!(!fault.is_overcurrent());
    }

    #[test]
    fn mock_fault_trigger_overcurrent() {
        let mut fault = MockFault::new();
        fault.trigger_overcurrent(1500);
        assert!(!fault.is_short_circuit());
        assert!(fault.is_overcurrent());
        assert_eq!(fault.fault_current_ma(), Some(1500));
    }

    #[test]
    fn mock_fault_clear() {
        let mut fault = MockFault::new();
        fault.trigger_short();
        fault.trigger_overcurrent(2000);
        fault.clear();

        assert!(!fault.is_short_circuit());
        assert!(!fault.is_overcurrent());
        assert!(fault.fault_current_ma().is_none());
    }

    // =========================================================================
    // MockClock Tests
    // =========================================================================

    #[test]
    fn mock_clock_default() {
        let clock = MockClock::new();
        assert_eq!(clock.now_ms(), 0);
    }

    #[test]
    fn mock_clock_set() {
        let mut clock = MockClock::new();
        clock.set(1000);
        assert_eq!(clock.now_ms(), 1000);
    }

    #[test]
    fn mock_clock_advance() {
        let mut clock = MockClock::new();
        clock.advance(500);
        assert_eq!(clock.now_ms(), 500);
        clock.advance(250);
        assert_eq!(clock.now_ms(), 750);
    }

    // =========================================================================
    // MockDisplay Tests
    // =========================================================================

    #[test]
    fn mock_display_default() {
        let display = MockDisplay::new();
        assert!(display.last_state.is_none());
        assert_eq!(display.render_count, 0);
        assert!(display.last_message.is_none());
        assert!(!display.initialized);
    }

    #[test]
    fn mock_display_init() {
        let mut display = MockDisplay::new();
        assert!(!display.initialized);
        display.init().unwrap();
        assert!(display.initialized);
    }

    #[test]
    fn mock_display_render() {
        let mut display = MockDisplay::new();
        display.init().unwrap();

        let state = ThrottleState {
            speed: 0.5,
            target_speed: Some(0.8),
            direction: Direction::Forward,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: None,
        };

        display.render(&state).unwrap();
        assert_eq!(display.render_count, 1);
        assert!(display.last_state.is_some());

        let rendered = display.last_state.as_ref().unwrap();
        assert_eq!(rendered.speed, 0.5);
        assert_eq!(rendered.direction, Direction::Forward);

        // Render again
        display.render(&state).unwrap();
        assert_eq!(display.render_count, 2);
    }

    #[test]
    fn mock_display_show_message() {
        let mut display = MockDisplay::new();
        display.show_message("Hello", Some("World")).unwrap();

        let (line1, line2) = display.last_message.as_ref().unwrap();
        assert_eq!(line1, "Hello");
        assert_eq!(line2.as_deref(), Some("World"));
    }

    #[test]
    fn mock_display_show_message_single_line() {
        let mut display = MockDisplay::new();
        display.show_message("Only one line", None).unwrap();

        let (line1, line2) = display.last_message.as_ref().unwrap();
        assert_eq!(line1, "Only one line");
        assert!(line2.is_none());
    }

    #[test]
    fn mock_display_clear() {
        let mut display = MockDisplay::new();
        let state = ThrottleState::default();
        display.render(&state).unwrap();
        assert!(display.last_state.is_some());

        display.clear().unwrap();
        assert!(display.last_state.is_none());
    }

    // =========================================================================
    // MockMqtt Tests
    // =========================================================================

    #[test]
    fn mock_mqtt_default() {
        let mqtt = MockMqtt::new();
        assert!(mqtt.connected);
        assert!(mqtt.published.is_empty());
        assert!(mqtt.subscriptions.is_empty());
        assert!(mqtt.incoming.is_empty());
    }

    #[test]
    fn mock_mqtt_queue_message() {
        let mut mqtt = MockMqtt::new();
        mqtt.queue_message("test/topic", b"payload".to_vec());

        assert_eq!(mqtt.incoming.len(), 1);
        assert_eq!(mqtt.incoming[0].topic, "test/topic");
        assert_eq!(mqtt.incoming[0].payload, b"payload");
    }

    #[test]
    fn mock_mqtt_is_subscribed() {
        let mut mqtt = MockMqtt::new();
        mqtt.subscriptions.push("train/speed".into());

        assert!(mqtt.is_subscribed("train/speed"));
        assert!(!mqtt.is_subscribed("train/direction"));
    }

    #[test]
    fn mock_mqtt_published_to() {
        let mut mqtt = MockMqtt::new();
        mqtt.published
            .push(("topic/a".into(), vec![1, 2, 3], false));
        mqtt.published.push(("topic/b".into(), vec![4, 5], true));
        mqtt.published.push(("topic/a".into(), vec![6], false));

        let topic_a = mqtt.published_to("topic/a");
        assert_eq!(topic_a.len(), 2);

        let topic_b = mqtt.published_to("topic/b");
        assert_eq!(topic_b.len(), 1);
        assert!(topic_b[0].2); // retain flag
    }

    // =========================================================================
    // MockHttp Tests
    // =========================================================================

    #[test]
    fn mock_http_default() {
        let http = MockHttp::new();
        assert!(http.requests.is_empty());
        assert!(http.responses.is_empty());
    }

    #[test]
    fn mock_http_queue_request() {
        let mut http = MockHttp::new();
        http.queue_request(HttpRequest {
            method: crate::HttpMethod::Get,
            path: "/api/state".into(),
            body: None,
        });

        assert_eq!(http.requests.len(), 1);
        assert_eq!(http.requests[0].path, "/api/state");
    }
}
