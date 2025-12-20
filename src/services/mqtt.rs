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

/// MQTT client configuration
#[derive(Debug, Clone)]
pub struct MqttConfig {
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

impl Default for MqttConfig {
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

impl MqttConfig {
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
    config: MqttConfig,
}

impl<M: MotorController + Send + 'static> MqttHandler<M> {
    /// Create a new MQTT handler with its own state.
    ///
    /// For sharing state with the web server, use `with_shared_state()` instead.
    pub fn new(controller: ThrottleController<M>, config: MqttConfig) -> Self {
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
    pub fn with_shared_state(state: Arc<SharedThrottleState<M>>, config: MqttConfig) -> Self {
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
