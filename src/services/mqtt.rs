//! MQTT client handler for the throttle controller.
//!
//! Subscribes to command topics and publishes state updates:
//!
//! **Subscribe Topics:**
//! - `train/speed/set` - Set speed `{"speed": 0.5, "duration_ms": 1000}`
//! - `train/direction/set` - Set direction `"forward"`, `"reverse"`, or `"stopped"`
//! - `train/estop` - Emergency stop (any payload)
//! - `train/max-speed/set` - Set max speed `{"max_speed": 0.8}`
//!
//! **Publish Topics:**
//! - `train/state` - Full state JSON (on change + heartbeat)
//! - `train/speed` - Current speed value (retained)
//! - `train/direction` - Current direction (retained)
//!
//! # Shared State
//!
//! For real-time model train control with multiple input sources, use
//! `MqttHandler::with_shared_state()` to share state with the web server:
//!
//! ```ignore
//! let state = Arc::new(SharedThrottleState::new(controller));
//! let handler = MqttHandler::with_shared_state(Arc::clone(&state), mqtt_config);
//! ```

use std::sync::Arc;
use std::time::Duration;

use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tokio::sync::mpsc;

use crate::config::MqttConfig as SharedMqttConfig;
use crate::traits::{EaseInOut, Immediate, Linear, MotorController};
use crate::{CommandSource, Direction, ThrottleCommand, ThrottleController};

use super::api::{SetDirectionRequest, SetMaxSpeedRequest, SetSpeedRequest, StateResponse};
use super::shared::SharedThrottleState;

// ============================================================================
// Configuration
// ============================================================================

/// Runtime MQTT client configuration for `rumqttc`.
///
/// This struct uses `String` for runtime compatibility with the `rumqttc` library.
/// For embedded/no-alloc contexts, use [`crate::config::MqttConfig`] which uses
/// fixed-size `ShortString` types and convert with [`MqttRuntimeConfig::from_config`].
#[derive(Debug, Clone)]
pub struct MqttRuntimeConfig {
    /// MQTT broker hostname
    pub host: String,
    /// MQTT broker port
    pub port: u16,
    /// Client ID
    pub client_id: String,
    /// Topic prefix (default: "train")
    pub topic_prefix: String,
    /// Heartbeat interval in milliseconds
    pub heartbeat_ms: u64,
    /// Keep-alive interval in seconds
    pub keep_alive_secs: u16,
}

impl Default for MqttRuntimeConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 1883,
            client_id: "rs-trainz".to_string(),
            topic_prefix: "train".to_string(),
            heartbeat_ms: 5000,
            keep_alive_secs: 30,
        }
    }
}

impl MqttRuntimeConfig {
    /// Create a new config with the given broker address
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    /// Create from shared MqttConfig
    pub fn from_config(config: &SharedMqttConfig) -> Self {
        Self {
            host: config.host.as_str().to_string(),
            port: config.port,
            client_id: config.client_id.as_str().to_string(),
            topic_prefix: config.topic_prefix.as_str().to_string(),
            heartbeat_ms: config.heartbeat_ms as u64,
            keep_alive_secs: config.keep_alive_secs,
        }
    }

    /// Set the client ID
    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = id.into();
        self
    }

    /// Set the topic prefix
    pub fn topic_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.topic_prefix = prefix.into();
        self
    }

    /// Set the heartbeat interval
    pub fn heartbeat_ms(mut self, ms: u64) -> Self {
        self.heartbeat_ms = ms;
        self
    }

    fn topic(&self, suffix: &str) -> String {
        format!("{}/{}", self.topic_prefix, suffix)
    }
}

// ============================================================================
// MQTT Handler
// ============================================================================

/// Legacy type alias for backward compatibility.
///
/// New code should use `SharedThrottleState` directly.
#[deprecated(since = "0.2.0", note = "Use SharedThrottleState instead")]
pub type MqttState<M> = SharedThrottleState<M>;

/// MQTT handler that bridges MQTT messages to the throttle controller
pub struct MqttHandler<M: MotorController + Send + 'static> {
    state: Arc<SharedThrottleState<M>>,
    config: MqttRuntimeConfig,
}

impl<M: MotorController + Send + 'static> MqttHandler<M> {
    /// Create a new MQTT handler with its own state.
    ///
    /// For sharing state with the web server, use `with_shared_state()` instead.
    pub fn new(controller: ThrottleController<M>, config: MqttRuntimeConfig) -> Self {
        Self {
            state: Arc::new(SharedThrottleState::new(controller)),
            config,
        }
    }

    /// Create a new MQTT handler with shared state.
    ///
    /// Use this constructor when you want to share state with the web server
    /// or other services for real-time synchronization.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let state = Arc::new(SharedThrottleState::new(controller));
    ///
    /// // Web and MQTT share the same state
    /// let web_router = build_router(Arc::clone(&state), &web_config);
    /// let mqtt_handler = MqttHandler::with_shared_state(Arc::clone(&state), mqtt_config);
    /// ```
    pub fn with_shared_state(state: Arc<SharedThrottleState<M>>, config: MqttRuntimeConfig) -> Self {
        Self { state, config }
    }

    /// Get a reference to the shared state.
    pub fn state(&self) -> Arc<SharedThrottleState<M>> {
        Arc::clone(&self.state)
    }

    /// Run the MQTT handler
    ///
    /// This function blocks and handles MQTT messages until shutdown.
    pub async fn run(self) -> Result<(), MqttError> {
        let mut options =
            MqttOptions::new(&self.config.client_id, &self.config.host, self.config.port);
        options.set_keep_alive(Duration::from_secs(self.config.keep_alive_secs as u64));

        let (client, mut eventloop) = AsyncClient::new(options, 10);

        // Subscribe to command topics
        let topics = [
            self.config.topic("speed/set"),
            self.config.topic("direction/set"),
            self.config.topic("estop"),
            self.config.topic("max-speed/set"),
        ];

        for topic in &topics {
            client
                .subscribe(topic, QoS::AtLeastOnce)
                .await
                .map_err(|e| MqttError::Subscribe(e.to_string()))?;
        }

        println!(
            "MQTT connected to {}:{}",
            self.config.host, self.config.port
        );
        println!("Subscribed to: {:?}", topics);

        // Channel for state updates to publish
        let (tx, mut rx) = mpsc::channel::<StateUpdate>(32);

        // Spawn heartbeat task
        let heartbeat_tx = tx.clone();
        let heartbeat_interval = self.config.heartbeat_ms;
        let state_for_heartbeat = Arc::clone(&self.state);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(heartbeat_interval));
            loop {
                interval.tick().await;
                let throttle_state = state_for_heartbeat.state();
                let state_response = StateResponse::from(&throttle_state);
                let _ = heartbeat_tx
                    .send(StateUpdate::Heartbeat(state_response))
                    .await;
            }
        });

        // Spawn publisher task
        let client_for_publish = client.clone();
        let config_for_publish = self.config.clone();
        tokio::spawn(async move {
            while let Some(update) = rx.recv().await {
                let state_json = match &update {
                    StateUpdate::Changed(s) | StateUpdate::Heartbeat(s) => {
                        serde_json::to_string(s).unwrap_or_default()
                    }
                };

                // Always publish full state
                let _ = client_for_publish
                    .publish(
                        config_for_publish.topic("state"),
                        QoS::AtLeastOnce,
                        false,
                        state_json.as_bytes(),
                    )
                    .await;

                // Publish individual values as retained
                if let StateUpdate::Changed(s) = &update {
                    let speed_str = format!("{:.3}", s.speed);
                    let _ = client_for_publish
                        .publish(
                            config_for_publish.topic("speed"),
                            QoS::AtLeastOnce,
                            true,
                            speed_str.as_bytes(),
                        )
                        .await;

                    let dir_str = format!("{:?}", s.direction).to_lowercase();
                    let _ = client_for_publish
                        .publish(
                            config_for_publish.topic("direction"),
                            QoS::AtLeastOnce,
                            true,
                            dir_str.as_bytes(),
                        )
                        .await;
                }
            }
        });

        // Main event loop
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    self.handle_message(&publish.topic, &publish.payload, &tx)
                        .await;
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("MQTT error: {:?}", e);
                    // Could add reconnection logic here
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn handle_message(&self, topic: &str, payload: &[u8], tx: &mpsc::Sender<StateUpdate>) {
        let suffix = topic
            .strip_prefix(&self.config.topic_prefix)
            .map(|s| s.trim_start_matches('/'))
            .unwrap_or(topic);

        let now_ms = self.state.now_ms();

        match suffix {
            "speed/set" => {
                if let Ok(req) = serde_json::from_slice::<SetSpeedRequest>(payload) {
                    let cmd = if req.duration_ms == 0 {
                        ThrottleCommand::speed_immediate(req.speed).into()
                    } else if req.smooth {
                        ThrottleCommand::SetSpeed {
                            target: req.speed,
                            strategy: EaseInOut::new(req.duration_ms),
                        }
                        .into()
                    } else {
                        ThrottleCommand::SetSpeed {
                            target: req.speed,
                            strategy: Linear::new(req.duration_ms),
                        }
                        .into()
                    };
                    self.state.with_controller(|controller| {
                        let _ = controller.apply_command(cmd, CommandSource::Mqtt, now_ms);
                    });
                    self.check_and_publish_changes(tx).await;
                }
            }

            "direction/set" => {
                // Accept either JSON or plain text
                let direction =
                    if let Ok(req) = serde_json::from_slice::<SetDirectionRequest>(payload) {
                        Some(req.direction)
                    } else if let Ok(text) = std::str::from_utf8(payload) {
                        match text.trim().to_lowercase().as_str() {
                            "forward" => Some(Direction::Forward),
                            "reverse" => Some(Direction::Reverse),
                            "stopped" | "stop" => Some(Direction::Stopped),
                            _ => None,
                        }
                    } else {
                        None
                    };

                if let Some(dir) = direction {
                    let cmd = ThrottleCommand::<Immediate>::SetDirection(dir).into();
                    self.state.with_controller(|controller| {
                        let _ = controller.apply_command(cmd, CommandSource::Mqtt, now_ms);
                    });
                    self.check_and_publish_changes(tx).await;
                }
            }

            "estop" => {
                let cmd = ThrottleCommand::estop().into();
                self.state.with_controller(|controller| {
                    let _ = controller.apply_command(cmd, CommandSource::Mqtt, now_ms);
                });
                self.check_and_publish_changes(tx).await;
            }

            "max-speed/set" => {
                if let Ok(req) = serde_json::from_slice::<SetMaxSpeedRequest>(payload) {
                    let cmd = ThrottleCommand::<Immediate>::SetMaxSpeed(req.max_speed).into();
                    self.state.with_controller(|controller| {
                        let _ = controller.apply_command(cmd, CommandSource::Mqtt, now_ms);
                    });
                    self.check_and_publish_changes(tx).await;
                }
            }

            _ => {}
        }
    }

    /// Check for state changes and publish if changed.
    async fn check_and_publish_changes(&self, tx: &mpsc::Sender<StateUpdate>) {
        if let Some(state) = self.state.check_changes() {
            let _ = tx.send(StateUpdate::Changed(state.into())).await;
        }
    }
}

enum StateUpdate {
    Changed(StateResponse),
    Heartbeat(StateResponse),
}

impl From<crate::ThrottleState> for StateResponse {
    fn from(state: crate::ThrottleState) -> Self {
        StateResponse::from(&state)
    }
}

/// MQTT-related errors
#[derive(Debug)]
pub enum MqttError {
    /// Failed to connect to broker
    Connect(String),
    /// Failed to subscribe to topic
    Subscribe(String),
    /// Failed to publish message
    Publish(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::MockMotor;
    use crate::ThrottleController;

    // ========================================================================
    // MqttRuntimeConfig tests
    // ========================================================================

    #[test]
    fn test_mqtt_config_default() {
        let config = MqttRuntimeConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 1883);
        assert_eq!(config.client_id, "rs-trainz");
        assert_eq!(config.topic_prefix, "train");
        assert_eq!(config.heartbeat_ms, 5000);
        assert_eq!(config.keep_alive_secs, 30);
    }

    #[test]
    fn test_mqtt_config_new() {
        let config = MqttRuntimeConfig::new("mqtt.example.com", 8883);
        assert_eq!(config.host, "mqtt.example.com");
        assert_eq!(config.port, 8883);
        // Other fields should be defaults
        assert_eq!(config.client_id, "rs-trainz");
        assert_eq!(config.topic_prefix, "train");
    }

    #[test]
    fn test_mqtt_config_builder_client_id() {
        let config = MqttRuntimeConfig::default().client_id("test-client");
        assert_eq!(config.client_id, "test-client");
    }

    #[test]
    fn test_mqtt_config_builder_topic_prefix() {
        let config = MqttRuntimeConfig::default().topic_prefix("locomotive");
        assert_eq!(config.topic_prefix, "locomotive");
    }

    #[test]
    fn test_mqtt_config_builder_heartbeat() {
        let config = MqttRuntimeConfig::default().heartbeat_ms(10000);
        assert_eq!(config.heartbeat_ms, 10000);
    }

    #[test]
    fn test_mqtt_config_builder_chaining() {
        let config = MqttRuntimeConfig::new("broker.local", 1883)
            .client_id("custom-id")
            .topic_prefix("my-train")
            .heartbeat_ms(2000);

        assert_eq!(config.host, "broker.local");
        assert_eq!(config.port, 1883);
        assert_eq!(config.client_id, "custom-id");
        assert_eq!(config.topic_prefix, "my-train");
        assert_eq!(config.heartbeat_ms, 2000);
    }

    #[test]
    fn test_mqtt_config_topic() {
        let config = MqttRuntimeConfig::default();
        assert_eq!(config.topic("speed/set"), "train/speed/set");
        assert_eq!(config.topic("direction/set"), "train/direction/set");
    }

    #[test]
    fn test_mqtt_config_topic_custom_prefix() {
        let config = MqttRuntimeConfig::default().topic_prefix("locomotive");
        assert_eq!(config.topic("speed/set"), "locomotive/speed/set");
    }

    #[test]
    fn test_mqtt_config_from_config() {
        use crate::config::short_string;

        let shared_config = crate::config::MqttConfig {
            enabled: true,
            host: short_string("mqtt.test.com"),
            port: 8883,
            username: short_string(""),
            password: short_string(""),
            client_id: short_string("test-id"),
            topic_prefix: short_string("test-train"),
            heartbeat_ms: 7500,
            keep_alive_secs: 60,
        };

        let config = MqttRuntimeConfig::from_config(&shared_config);
        assert_eq!(config.host, "mqtt.test.com");
        assert_eq!(config.port, 8883);
        assert_eq!(config.client_id, "test-id");
        assert_eq!(config.topic_prefix, "test-train");
        assert_eq!(config.heartbeat_ms, 7500);
        assert_eq!(config.keep_alive_secs, 60);
    }

    // ========================================================================
    // MqttHandler tests
    // ========================================================================

    #[test]
    fn test_mqtt_handler_new() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let config = MqttRuntimeConfig::default();

        let handler = MqttHandler::new(controller, config);
        let state = handler.state();

        // Should have created its own shared state
        let snapshot = state.state();
        assert_eq!(snapshot.speed, 0.0);
        assert_eq!(snapshot.direction, Direction::Stopped);
    }

    #[test]
    fn test_mqtt_handler_with_shared_state() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();

        let handler = MqttHandler::with_shared_state(Arc::clone(&state), config);

        // Both handler and original state should point to the same underlying controller
        let handler_state_snapshot = handler.state().state();
        let original_state_snapshot = state.state();

        assert_eq!(handler_state_snapshot.speed, original_state_snapshot.speed);
        assert_eq!(handler_state_snapshot.direction, original_state_snapshot.direction);
    }

    #[test]
    fn test_mqtt_handler_state_sharing() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();

        let handler = MqttHandler::with_shared_state(Arc::clone(&state), config);

        // Modify state via handler
        let now = state.now_ms();
        handler.state().with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.75).into();
            let _ = c.apply_command(cmd, CommandSource::Mqtt, now);
            let _ = c.update(now);
        });

        // Original state should see the change
        let snapshot = state.state();
        assert!((snapshot.speed - 0.75).abs() < 0.01);
    }

    // ========================================================================
    // StateUpdate tests
    // ========================================================================

    #[test]
    fn test_state_update_from_throttle_state() {
        let throttle_state = crate::ThrottleState {
            speed: 0.42,
            direction: Direction::Forward,
            max_speed: 0.8,
            target_speed: Some(0.6),
            transition_progress: None,
            fault: None,
            lock_status: None,
        };

        let state_response: StateResponse = throttle_state.into();
        assert_eq!(state_response.speed, 0.42);
        assert_eq!(state_response.direction, Direction::Forward);
        assert_eq!(state_response.max_speed, 0.8);
        assert_eq!(state_response.target_speed, Some(0.6));
    }

    // ========================================================================
    // MqttError tests
    // ========================================================================

    #[test]
    fn test_mqtt_error_connect_display() {
        let error = MqttError::Connect("connection refused".to_string());
        let display = format!("{}", error);
        assert!(display.contains("MQTT connect error"));
        assert!(display.contains("connection refused"));
    }

    #[test]
    fn test_mqtt_error_subscribe_display() {
        let error = MqttError::Subscribe("topic not found".to_string());
        let display = format!("{}", error);
        assert!(display.contains("MQTT subscribe error"));
        assert!(display.contains("topic not found"));
    }

    #[test]
    fn test_mqtt_error_publish_display() {
        let error = MqttError::Publish("network timeout".to_string());
        let display = format!("{}", error);
        assert!(display.contains("MQTT publish error"));
        assert!(display.contains("network timeout"));
    }

    #[test]
    fn test_mqtt_error_debug() {
        let error = MqttError::Connect("test".to_string());
        let debug = format!("{:?}", error);
        assert!(debug.contains("Connect"));
        assert!(debug.contains("test"));
    }

    #[test]
    fn test_mqtt_error_is_error() {
        let error = MqttError::Connect("test".to_string());
        // Should implement std::error::Error
        let _: &dyn std::error::Error = &error;
    }

    // ========================================================================
    // Check and publish changes tests
    // ========================================================================

    #[tokio::test]
    async fn test_check_and_publish_changes_no_change() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, mut rx) = mpsc::channel::<StateUpdate>(32);

        // Sync change detection to initial state
        state.sync_change_detection();

        // Check for changes when there are none
        handler.check_and_publish_changes(&tx).await;

        // Should not have sent any updates
        tokio::select! {
            _ = rx.recv() => panic!("Should not have received update"),
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(10)) => {}
        }
    }

    #[tokio::test]
    async fn test_check_and_publish_changes_with_change() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, mut rx) = mpsc::channel::<StateUpdate>(32);

        // Sync initial state
        state.sync_change_detection();

        // Make a change
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.5).into();
            let _ = c.apply_command(cmd, CommandSource::Mqtt, now);
            let _ = c.update(now);
        });

        // Check for changes
        handler.check_and_publish_changes(&tx).await;

        // Should have received an update
        let update = tokio::time::timeout(
            tokio::time::Duration::from_millis(100),
            rx.recv()
        ).await.expect("timeout").expect("should have update");

        match update {
            StateUpdate::Changed(state_response) => {
                assert!((state_response.speed - 0.5).abs() < 0.01);
            }
            StateUpdate::Heartbeat(_) => panic!("Expected Changed, got Heartbeat"),
        }
    }

    // ========================================================================
    // Handle message tests
    // ========================================================================

    #[tokio::test]
    async fn test_handle_message_speed_set_immediate() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        let payload = r#"{"speed": 0.75, "duration_ms": 0}"#;
        handler.handle_message("train/speed/set", payload.as_bytes(), &tx).await;

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!((current.speed - 0.75).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_handle_message_speed_set_linear() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        let payload = r#"{"speed": 0.8, "duration_ms": 1000, "smooth": false}"#;
        handler.handle_message("train/speed/set", payload.as_bytes(), &tx).await;

        let current = state.state();
        assert_eq!(current.target_speed, Some(0.8));
    }

    #[tokio::test]
    async fn test_handle_message_speed_set_smooth() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        let payload = r#"{"speed": 0.6, "duration_ms": 2000, "smooth": true}"#;
        handler.handle_message("train/speed/set", payload.as_bytes(), &tx).await;

        let current = state.state();
        assert_eq!(current.target_speed, Some(0.6));
    }

    #[tokio::test]
    async fn test_handle_message_direction_json() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        let payload = r#"{"direction": "forward"}"#;
        handler.handle_message("train/direction/set", payload.as_bytes(), &tx).await;

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[tokio::test]
    async fn test_handle_message_direction_text_forward() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        handler.handle_message("train/direction/set", b"forward", &tx).await;

        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[tokio::test]
    async fn test_handle_message_direction_text_reverse() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        handler.handle_message("train/direction/set", b"reverse", &tx).await;

        let current = state.state();
        assert_eq!(current.direction, Direction::Reverse);
    }

    #[tokio::test]
    async fn test_handle_message_direction_text_stopped() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        handler.handle_message("train/direction/set", b"stopped", &tx).await;

        let current = state.state();
        assert_eq!(current.direction, Direction::Stopped);
    }

    #[tokio::test]
    async fn test_handle_message_direction_text_stop_alias() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        handler.handle_message("train/direction/set", b"stop", &tx).await;

        let current = state.state();
        assert_eq!(current.direction, Direction::Stopped);
    }

    #[tokio::test]
    async fn test_handle_message_direction_invalid() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        // Set to forward first
        state.with_controller(|c| {
            let _ = c.apply_command(
                crate::ThrottleCommandDyn::SetDirection(Direction::Forward),
                CommandSource::Physical,
                0,
            );
        });

        // Try invalid direction
        handler.handle_message("train/direction/set", b"sideways", &tx).await;

        // Should remain forward (unchanged)
        let current = state.state();
        assert_eq!(current.direction, Direction::Forward);
    }

    #[tokio::test]
    async fn test_handle_message_estop() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        // Set initial speed
        let now = state.now_ms();
        state.with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.7).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now);
            let _ = c.update(now);
        });

        // Trigger e-stop
        handler.handle_message("train/estop", b"", &tx).await;

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!(current.speed.abs() < 0.01);
    }

    #[tokio::test]
    async fn test_handle_message_max_speed() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        let payload = r#"{"max_speed": 0.85}"#;
        handler.handle_message("train/max-speed/set", payload.as_bytes(), &tx).await;

        let current = state.state();
        assert!((current.max_speed - 0.85).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_handle_message_unknown_topic() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        // Should not panic or error on unknown topic
        handler.handle_message("train/unknown/topic", b"data", &tx).await;

        // State should be unchanged
        let current = state.state();
        assert_eq!(current.speed, 0.0);
    }

    #[tokio::test]
    async fn test_handle_message_custom_prefix() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default().topic_prefix("locomotive");
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        let payload = r#"{"speed": 0.6, "duration_ms": 0}"#;
        handler.handle_message("locomotive/speed/set", payload.as_bytes(), &tx).await;

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = state.state();
        assert!((current.speed - 0.6).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_handle_message_invalid_json() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));
        let config = MqttRuntimeConfig::default();
        let handler = MqttHandler::with_shared_state(state.clone(), config);

        let (tx, _rx) = mpsc::channel::<StateUpdate>(32);

        // Invalid JSON should be ignored without panic
        handler.handle_message("train/speed/set", b"not-json", &tx).await;

        let current = state.state();
        assert_eq!(current.speed, 0.0); // Unchanged
    }
}

impl std::fmt::Display for MqttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(e) => write!(f, "MQTT connect error: {}", e),
            Self::Subscribe(e) => write!(f, "MQTT subscribe error: {}", e),
            Self::Publish(e) => write!(f, "MQTT publish error: {}", e),
        }
    }
}

impl std::error::Error for MqttError {}
