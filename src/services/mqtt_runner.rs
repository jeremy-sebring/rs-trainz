//! MQTT service runner for unified polling across platforms.
//!
//! Provides a platform-agnostic MQTT handler that works with any
//! implementation of the `MqttClient` trait.
//!
//! # Example
//!
//! ```ignore
//! use rs_trainz::services::{MqttServiceRunner, SharedThrottleState};
//! use rs_trainz::traits::MqttClient;
//!
//! let state = Arc::new(SharedThrottleState::new(controller));
//! let mut runner = MqttServiceRunner::new(state, mqtt_client, config);
//!
//! // In main loop:
//! runner.poll()?;                    // Process incoming messages
//! runner.publish_if_changed()?;      // Publish state changes
//! ```

use std::sync::Arc;

use crate::config::MqttConfig;
use crate::messages::parse_mqtt_command;
use crate::traits::{MotorController, MqttClient};
use crate::{CommandSource, Direction, ThrottleCommandDyn};

use super::http_handler::state_to_json;
use super::SharedThrottleState;

// ============================================================================
// MQTT Service Runner
// ============================================================================

/// Unified MQTT service runner for both desktop and ESP32.
///
/// Wraps any `MqttClient` implementation and provides:
/// - Message polling with automatic command parsing
/// - State change publishing
/// - Heartbeat publishing
pub struct MqttServiceRunner<M, C>
where
    M: MotorController + Send + 'static,
    C: MqttClient,
{
    state: Arc<SharedThrottleState<M>>,
    client: C,
    config: MqttConfig,
    last_published_speed: f32,
    last_published_direction: Direction,
}

impl<M, C> MqttServiceRunner<M, C>
where
    M: MotorController + Send + 'static,
    C: MqttClient,
{
    /// Create a new MQTT service runner.
    pub fn new(state: Arc<SharedThrottleState<M>>, client: C, config: MqttConfig) -> Self {
        Self {
            state,
            client,
            config,
            last_published_speed: 0.0,
            last_published_direction: Direction::Stopped,
        }
    }

    /// Get a reference to the MQTT client.
    pub fn client(&self) -> &C {
        &self.client
    }

    /// Get a mutable reference to the MQTT client.
    pub fn client_mut(&mut self) -> &mut C {
        &mut self.client
    }

    /// Poll for incoming MQTT messages and apply commands.
    ///
    /// This should be called regularly in the main loop. It processes
    /// all pending messages and applies any valid commands to the controller.
    pub fn poll(&mut self) -> Result<(), C::Error> {
        while let Some(msg) = self.client.try_recv() {
            if let Some(cmd) = self.parse_message(&msg.topic, &msg.payload) {
                let now_ms = self.state.now_ms();
                self.state.with_controller(|controller| {
                    let _ = controller.apply_command(cmd, CommandSource::Mqtt, now_ms);
                });
            }
        }
        Ok(())
    }

    /// Publish current state if it has changed since last publish.
    ///
    /// Returns `true` if state was published, `false` if unchanged.
    pub fn publish_if_changed(&mut self) -> Result<bool, C::Error> {
        let current_state = self.state.state();

        let speed_changed = (current_state.speed - self.last_published_speed).abs() > 0.001;
        let direction_changed = current_state.direction != self.last_published_direction;

        if speed_changed || direction_changed {
            self.publish_state_internal(&current_state)?;
            self.last_published_speed = current_state.speed;
            self.last_published_direction = current_state.direction;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Force publish current state (for heartbeat).
    pub fn publish_state(&mut self) -> Result<(), C::Error> {
        let current_state = self.state.state();
        self.publish_state_internal(&current_state)?;
        self.last_published_speed = current_state.speed;
        self.last_published_direction = current_state.direction;
        Ok(())
    }

    /// Internal state publishing.
    fn publish_state_internal(&mut self, state: &crate::ThrottleState) -> Result<(), C::Error> {
        // Publish full state JSON
        let json = state_to_json(state);
        let state_topic = self.topic("state");
        self.client.publish(&state_topic, json.as_bytes(), false)?;

        // Publish individual values as retained
        let speed_str = format!("{:.3}", state.speed);
        let speed_topic = self.topic("speed");
        self.client
            .publish(&speed_topic, speed_str.as_bytes(), true)?;

        let dir_str = state.direction.as_str();
        let direction_topic = self.topic("direction");
        self.client
            .publish(&direction_topic, dir_str.as_bytes(), true)?;

        Ok(())
    }

    /// Subscribe to control topics.
    pub fn subscribe_control_topics(&mut self) -> Result<(), C::Error> {
        let topics = ["speed/set", "direction/set", "estop", "max-speed/set"];
        for suffix in topics {
            let topic = self.topic(suffix);
            self.client.subscribe(&topic)?;
        }
        Ok(())
    }

    /// Build a full topic path.
    fn topic(&self, suffix: &str) -> String {
        format!("{}/{}", self.config.topic_prefix, suffix)
    }

    /// Parse an MQTT message into a command.
    ///
    /// Delegates to the consolidated `parse_mqtt_command` function in `messages.rs`.
    fn parse_message(&self, topic: &str, payload: &[u8]) -> Option<ThrottleCommandDyn> {
        let prefix = self.config.topic_prefix.as_str();
        let suffix = topic.strip_prefix(prefix)?.strip_prefix('/')?;
        parse_mqtt_command(suffix, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::{MockMotor, MockMqtt};
    use crate::ThrottleController;

    fn setup() -> (Arc<SharedThrottleState<MockMotor>>, MockMqtt, MqttConfig) {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let mqtt = MockMqtt::new();
        let config = MqttConfig::default();
        (state, mqtt, config)
    }

    // ========================================================================
    // Basic runner tests
    // ========================================================================

    #[test]
    fn test_runner_creation() {
        let (state, mqtt, config) = setup();
        let runner = MqttServiceRunner::new(state, mqtt, config);
        // MockMqtt::new() starts connected
        assert!(runner.client().is_connected());
    }

    #[test]
    fn test_subscribe_control_topics() {
        let (state, mqtt, config) = setup();
        let mut runner = MqttServiceRunner::new(state, mqtt, config);

        runner.subscribe_control_topics().unwrap();

        let client = runner.client();
        assert!(client
            .subscriptions
            .contains(&"train/speed/set".to_string()));
        assert!(client
            .subscriptions
            .contains(&"train/direction/set".to_string()));
        assert!(client.subscriptions.contains(&"train/estop".to_string()));
        assert!(client
            .subscriptions
            .contains(&"train/max-speed/set".to_string()));
    }

    // ========================================================================
    // Speed command tests
    // ========================================================================

    #[test]
    fn test_poll_with_speed_command() {
        let (state, mut mqtt, config) = setup();

        // Queue a speed command
        mqtt.queue_message("train/speed/set", b"0.75".to_vec());
        mqtt.connected = true;

        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        // Poll to process the message
        runner.poll().unwrap();

        // Update controller to apply the command
        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        // Check that speed was set
        let current = state.state();
        assert!((current.speed - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_poll_with_speed_zero() {
        let (state, mut mqtt, config) = setup();

        // First set a non-zero speed
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = crate::ThrottleCommand::speed_immediate(0.5).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.update(now);
        });

        // Now send speed 0 via MQTT
        mqtt.queue_message("train/speed/set", b"0".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!(current.speed.abs() < 0.01);
    }

    #[test]
    fn test_poll_with_speed_clamped_high() {
        let (state, mut mqtt, config) = setup();

        // Send speed > 1.0, should be clamped
        mqtt.queue_message("train/speed/set", b"1.5".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!((current.speed - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_poll_with_speed_clamped_negative() {
        let (state, mut mqtt, config) = setup();

        // Send negative speed, should be clamped to 0
        mqtt.queue_message("train/speed/set", b"-0.5".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!(current.speed.abs() < 0.01);
    }

    // ========================================================================
    // JSON speed command tests
    // ========================================================================

    #[test]
    fn test_poll_with_speed_json_immediate() {
        let (state, mut mqtt, config) = setup();

        // JSON format with duration_ms = 0 means immediate
        mqtt.queue_message("train/speed/set", br#"{"speed": 0.65}"#.to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!((current.speed - 0.65).abs() < 0.01);
    }

    #[test]
    fn test_poll_with_speed_json_linear() {
        let (state, mut mqtt, config) = setup();

        // JSON format with duration_ms and smooth=false means linear
        mqtt.queue_message(
            "train/speed/set",
            br#"{"speed": 0.8, "duration_ms": 1000, "smooth": false}"#.to_vec(),
        );
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        // Should have target_speed set (transition in progress)
        let current = state.state();
        assert_eq!(current.target_speed, Some(0.8));
    }

    #[test]
    fn test_poll_with_speed_json_smooth() {
        let (state, mut mqtt, config) = setup();

        // JSON format with duration_ms and smooth=true means EaseInOut
        mqtt.queue_message(
            "train/speed/set",
            br#"{"speed": 0.9, "duration_ms": 2000, "smooth": true}"#.to_vec(),
        );
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        // Should have target_speed set (transition in progress)
        let current = state.state();
        assert_eq!(current.target_speed, Some(0.9));
    }

    #[test]
    fn test_poll_with_direction_json() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/direction/set", br#"{"direction": "forward"}"#.to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[test]
    fn test_poll_with_direction_json_reverse() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/direction/set", br#"{"direction": "reverse"}"#.to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Reverse);
    }

    #[test]
    fn test_poll_with_max_speed_json() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/max-speed/set", br#"{"max_speed": 0.85}"#.to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert!((current.max_speed - 0.85).abs() < 0.01);
    }

    // ========================================================================
    // Direction command tests
    // ========================================================================

    #[test]
    fn test_poll_with_direction_forward() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/direction/set", b"forward".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[test]
    fn test_poll_with_direction_reverse() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/direction/set", b"reverse".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Reverse);
    }

    #[test]
    fn test_poll_with_direction_rev_alias() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/direction/set", b"rev".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Reverse);
    }

    #[test]
    fn test_poll_with_direction_fwd_alias() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/direction/set", b"fwd".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[test]
    fn test_poll_with_direction_numeric() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/direction/set", b"1".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[test]
    fn test_poll_with_direction_stop() {
        let (state, mut mqtt, config) = setup();

        // First set forward
        mqtt.queue_message("train/direction/set", b"forward".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config.clone());
        runner.poll().unwrap();

        // Now stop
        runner
            .client_mut()
            .queue_message("train/direction/set", b"stop".to_vec());
        runner.poll().unwrap();

        let current = state.state();
        assert_eq!(current.direction, Direction::Stopped);
    }

    // ========================================================================
    // E-stop command tests
    // ========================================================================

    #[test]
    fn test_poll_with_estop() {
        let (state, mut mqtt, config) = setup();

        // First set a speed
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = crate::ThrottleCommand::speed_immediate(0.8).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.update(now);
        });

        // Send e-stop
        mqtt.queue_message("train/estop", b"1".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!(current.speed.abs() < 0.01, "E-stop should set speed to 0");
    }

    #[test]
    fn test_poll_with_estop_empty_payload() {
        let (state, mut mqtt, config) = setup();

        // Set a speed first
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = crate::ThrottleCommand::speed_immediate(0.5).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.update(now);
        });

        // E-stop with empty payload should still work
        mqtt.queue_message("train/estop", b"".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!(current.speed.abs() < 0.01);
    }

    // ========================================================================
    // Max-speed command tests
    // ========================================================================

    #[test]
    fn test_poll_with_max_speed() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/max-speed/set", b"0.75".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert!((current.max_speed - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_poll_with_max_speed_clamped() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/max-speed/set", b"2.0".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert!((current.max_speed - 1.0).abs() < 0.01);
    }

    // ========================================================================
    // Invalid payload tests
    // ========================================================================

    #[test]
    fn test_poll_with_invalid_speed_payload() {
        let (state, mut mqtt, config) = setup();

        // Invalid payload should be ignored
        mqtt.queue_message("train/speed/set", b"not-a-number".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        // Speed should remain at 0 (unchanged)
        let current = state.state();
        assert!(current.speed.abs() < 0.01);
    }

    #[test]
    fn test_poll_with_invalid_direction_payload() {
        let (state, mut mqtt, config) = setup();

        // First set to forward
        let now = state.now_ms();
        state.with_controller(|c| {
            let _ = c.apply_command(
                crate::ThrottleCommandDyn::SetDirection(Direction::Forward),
                CommandSource::Physical,
                now,
            );
        });

        // Invalid direction should be ignored
        mqtt.queue_message("train/direction/set", b"sideways".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        // Direction should remain forward (unchanged)
        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[test]
    fn test_poll_with_invalid_utf8() {
        let (state, mut mqtt, config) = setup();

        // Invalid UTF-8 bytes
        mqtt.queue_message("train/speed/set", vec![0xFF, 0xFE]);
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        // Should not panic, just ignore
        runner.poll().unwrap();

        let current = state.state();
        assert!(current.speed.abs() < 0.01);
    }

    #[test]
    fn test_poll_with_unknown_topic() {
        let (state, mut mqtt, config) = setup();

        mqtt.queue_message("train/unknown/topic", b"value".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        // Should not panic, just ignore unknown topics
        runner.poll().unwrap();
    }

    #[test]
    fn test_poll_with_wrong_prefix() {
        let (state, mut mqtt, config) = setup();

        // Topic with wrong prefix should be ignored
        mqtt.queue_message("other/speed/set", b"0.5".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let current = state.state();
        assert!(current.speed.abs() < 0.01);
    }

    // ========================================================================
    // Publish tests
    // ========================================================================

    #[test]
    fn test_publish_if_changed() {
        let (state, mqtt, config) = setup();
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        // Initial publish (always publishes since state differs from defaults)
        // Actually, initial state is 0.0/Stopped which matches runner defaults
        let published = runner.publish_if_changed().unwrap();
        assert!(!published); // No change from initial state

        // Change state
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = crate::ThrottleCommand::speed_immediate(0.5).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.update(now);
        });

        // Now it should publish
        let published = runner.publish_if_changed().unwrap();
        assert!(published);

        // Second call without change should not publish
        let published = runner.publish_if_changed().unwrap();
        assert!(!published);
    }

    #[test]
    fn test_publish_if_changed_direction_only() {
        let (state, mqtt, config) = setup();
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        // Change only direction
        state.with_controller(|c| {
            let now = 0;
            let _ = c.apply_command(
                crate::ThrottleCommandDyn::SetDirection(Direction::Forward),
                CommandSource::Physical,
                now,
            );
        });

        let published = runner.publish_if_changed().unwrap();
        assert!(published, "Direction change should trigger publish");
    }

    #[test]
    fn test_publish_state_format() {
        let (state, mqtt, config) = setup();

        // Set specific state
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = crate::ThrottleCommand::speed_immediate(0.42).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.apply_command(
                crate::ThrottleCommandDyn::SetDirection(Direction::Reverse),
                CommandSource::Physical,
                now,
            );
            let _ = c.update(now);
        });

        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.publish_state().unwrap();

        // Check published message format
        let client = runner.client();
        let published = client.published_to("train/state");
        assert_eq!(published.len(), 1);

        let payload = core::str::from_utf8(&published[0].1).unwrap();
        assert!(payload.contains("\"speed\":"));
        assert!(payload.contains("\"direction\":"));
    }

    #[test]
    fn test_publish_state_retained() {
        let (state, mqtt, config) = setup();
        let mut runner = MqttServiceRunner::new(state, mqtt, config);

        runner.publish_state().unwrap();

        let client = runner.client();

        // State topic is NOT retained (transient updates)
        let state_published = client.published_to("train/state");
        assert_eq!(state_published.len(), 1);
        let (_, _, retained) = state_published[0];
        assert!(!*retained, "State topic should not be retained");

        // Speed and direction topics ARE retained (for new subscribers)
        let speed_published = client.published_to("train/speed");
        assert_eq!(speed_published.len(), 1);
        let (_, _, retained) = speed_published[0];
        assert!(*retained, "Speed topic should be retained");

        let direction_published = client.published_to("train/direction");
        assert_eq!(direction_published.len(), 1);
        let (_, _, retained) = direction_published[0];
        assert!(*retained, "Direction topic should be retained");
    }

    // ========================================================================
    // Custom topic prefix tests
    // ========================================================================

    #[test]
    fn test_custom_topic_prefix() {
        use crate::config::short_string;

        let (state, mut mqtt, _) = setup();
        let config = MqttConfig {
            topic_prefix: short_string("locomotive"),
            ..Default::default()
        };

        mqtt.queue_message("locomotive/speed/set", b"0.6".to_vec());
        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!((current.speed - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_multiple_messages_in_queue() {
        let (state, mut mqtt, config) = setup();

        // Queue multiple messages
        mqtt.queue_message("train/direction/set", b"forward".to_vec());
        mqtt.queue_message("train/speed/set", b"0.8".to_vec());

        let mut runner = MqttServiceRunner::new(state.clone(), mqtt, config);

        // First poll processes first message
        runner.poll().unwrap();
        // Second poll processes second message
        runner.poll().unwrap();

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
        assert!((current.speed - 0.8).abs() < 0.01);
    }
}
