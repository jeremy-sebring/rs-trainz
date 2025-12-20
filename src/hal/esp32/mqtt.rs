//! MQTT client for ESP32-C3.
//!
//! Provides MQTT pub/sub functionality using esp-idf-svc for remote
//! throttle control and state publishing. Implements the `MqttClient` trait
//! for compatibility with the shared services layer.
//!
//! # Topics
//!
//! Using default prefix "train":
//! - `train/state` - Published state (JSON)
//! - `train/speed/set` - Subscribe for speed commands
//! - `train/direction/set` - Subscribe for direction commands
//! - `train/estop` - Subscribe for emergency stop
//!
//! # Example
//!
//! ```ignore
//! use rs_trainz::hal::esp32::Esp32Mqtt;
//! use rs_trainz::config::MqttConfig;
//! use rs_trainz::traits::MqttClient;
//!
//! let config = MqttConfig::default()
//!     .with_host("192.168.1.100")
//!     .with_topic_prefix("trains/loco1");
//!
//! let mut mqtt = Esp32Mqtt::new(&config)?;
//!
//! // Use trait methods
//! mqtt.publish("train/state", b"online", false)?;
//! ```

use crate::config::MqttConfig;
use crate::traits::{MqttClient, MqttMessage};
use crate::{Direction, ThrottleCommand, ThrottleCommandDyn, ThrottleState};
use esp_idf_svc::mqtt::client::{
    EspMqttClient, EspMqttConnection, EventPayload, MqttClientConfiguration, QoS,
};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

/// MQTT client for throttle control.
///
/// Connects to an MQTT broker and subscribes to control topics.
/// Implements `MqttClient` trait for unified service layer integration.
/// Incoming messages are queued and can be polled via `try_recv()`.
pub struct Esp32Mqtt {
    client: EspMqttClient<'static>,
    /// Receiver for parsed throttle commands (legacy API)
    command_rx: Receiver<ThrottleCommandDyn>,
    /// Receiver for raw MQTT messages (trait API)
    message_rx: Receiver<MqttMessage>,
    topic_prefix: heapless::String<64>,
    connected: bool,
}

impl Esp32Mqtt {
    /// Create a new MQTT client and connect to the broker.
    ///
    /// Subscribes to control topics automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if connection or subscription fails.
    pub fn new(config: &MqttConfig) -> anyhow::Result<Self> {
        let broker_url = format!("mqtt://{}:{}", config.host.as_str(), config.port);

        let mqtt_config = MqttClientConfiguration {
            client_id: Some(config.client_id.as_str()),
            keep_alive_interval: Some(Duration::from_secs(config.keep_alive_secs as u64)),
            ..Default::default()
        };

        let (command_tx, command_rx) = channel::<ThrottleCommandDyn>();
        let (message_tx, message_rx) = channel::<MqttMessage>();
        let topic_prefix_clone = config.topic_prefix.clone();

        let (client, mut connection) = EspMqttClient::new(&broker_url, &mqtt_config)?;

        // Spawn a thread to handle incoming messages
        let prefix = config.topic_prefix.clone();
        thread::spawn(move || {
            handle_mqtt_events(&mut connection, command_tx, message_tx, &prefix);
        });

        let mut mqtt = Self {
            client,
            command_rx,
            message_rx,
            topic_prefix: topic_prefix_clone,
            connected: true,
        };

        // Subscribe to control topics
        mqtt.subscribe_all()?;

        println!("[MQTT] Connected to {}", broker_url);

        Ok(mqtt)
    }

    /// Subscribe to all control topics.
    fn subscribe_all(&mut self) -> anyhow::Result<()> {
        let topics = ["speed/set", "direction/set", "estop"];
        for topic_suffix in topics {
            let mut full_topic: heapless::String<128> = heapless::String::new();
            let _ = full_topic.push_str(self.topic_prefix.as_str());
            let _ = full_topic.push('/');
            let _ = full_topic.push_str(topic_suffix);

            self.client
                .subscribe(full_topic.as_str(), QoS::AtLeastOnce)?;
            println!("[MQTT] Subscribed to {}", full_topic);
        }
        Ok(())
    }

    /// Publish the current throttle state (convenience method).
    pub fn publish_state(&mut self, state: &ThrottleState) -> anyhow::Result<()> {
        let mut topic: heapless::String<128> = heapless::String::new();
        let _ = topic.push_str(self.topic_prefix.as_str());
        let _ = topic.push_str("/state");

        let target = state.target_speed.unwrap_or(state.speed);
        let is_transitioning = state.transition_progress.is_some();
        let json = format!(
            r#"{{"speed":{:.2},"target_speed":{:.2},"direction":"{}","is_transitioning":{}}}"#,
            state.speed,
            target,
            direction_str(&state.direction),
            is_transitioning
        );

        self.client
            .publish(topic.as_str(), QoS::AtMostOnce, false, json.as_bytes())?;

        Ok(())
    }

    /// Receive the next pending command, if any (legacy API).
    ///
    /// Returns `None` if no commands are pending. This is non-blocking.
    /// For trait-based access, use `try_recv()` instead.
    pub fn recv_command(&mut self) -> Option<ThrottleCommandDyn> {
        match self.command_rx.try_recv() {
            Ok(cmd) => Some(cmd),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.connected = false;
                None
            }
        }
    }

    /// Get the topic prefix.
    pub fn topic_prefix(&self) -> &str {
        self.topic_prefix.as_str()
    }
}

// ============================================================================
// MqttClient Trait Implementation
// ============================================================================

/// Error type for ESP32 MQTT operations.
#[derive(Debug)]
pub struct Esp32MqttError(pub String);

impl core::fmt::Display for Esp32MqttError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "MQTT error: {}", self.0)
    }
}

impl MqttClient for Esp32Mqtt {
    type Error = Esp32MqttError;

    fn publish(&mut self, topic: &str, payload: &[u8], retain: bool) -> Result<(), Self::Error> {
        let qos = if retain {
            QoS::AtLeastOnce
        } else {
            QoS::AtMostOnce
        };
        self.client
            .publish(topic, qos, retain, payload)
            .map_err(|e| Esp32MqttError(format!("{:?}", e)))?;
        Ok(())
    }

    fn subscribe(&mut self, topic: &str) -> Result<(), Self::Error> {
        self.client
            .subscribe(topic, QoS::AtLeastOnce)
            .map_err(|e| Esp32MqttError(format!("{:?}", e)))?;
        Ok(())
    }

    fn try_recv(&mut self) -> Option<MqttMessage> {
        match self.message_rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.connected = false;
                None
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn direction_str(dir: &Direction) -> &'static str {
    match dir {
        Direction::Forward => "forward",
        Direction::Reverse => "reverse",
        Direction::Stopped => "stopped",
    }
}

fn handle_mqtt_events(
    connection: &mut EspMqttConnection,
    command_tx: Sender<ThrottleCommandDyn>,
    message_tx: Sender<MqttMessage>,
    topic_prefix: &heapless::String<64>,
) {
    loop {
        match connection.next() {
            Err(e) => {
                println!("[MQTT] Error: {:?}", e);
                thread::sleep(Duration::from_secs(1));
            }
            Ok(event) => {
                if let EventPayload::Received {
                    topic: Some(topic),
                    data,
                    ..
                } = event.payload()
                {
                    // Send raw message for trait API
                    let msg = MqttMessage::new(topic.to_string(), data.to_vec());
                    let _ = message_tx.send(msg);

                    // Also parse and send command for legacy API
                    if let Some(cmd) = parse_mqtt_message(topic, data, topic_prefix) {
                        let _ = command_tx.send(cmd);
                    }
                }
            }
        }
    }
}

fn parse_mqtt_message(
    topic: &str,
    data: &[u8],
    prefix: &heapless::String<64>,
) -> Option<ThrottleCommandDyn> {
    let suffix = topic.strip_prefix(prefix.as_str())?.strip_prefix('/')?;

    match suffix {
        "speed/set" => {
            let payload = core::str::from_utf8(data).ok()?;
            let speed: f32 = payload.trim().parse().ok()?;
            Some(ThrottleCommand::speed_immediate(speed.clamp(0.0, 1.0)).into())
        }
        "direction/set" => {
            let payload = core::str::from_utf8(data).ok()?.trim();
            let dir = match payload {
                "forward" | "fwd" | "1" => Direction::Forward,
                "reverse" | "rev" | "-1" => Direction::Reverse,
                _ => return None,
            };
            Some(ThrottleCommandDyn::SetDirection(dir))
        }
        "estop" => Some(ThrottleCommandDyn::EmergencyStop),
        _ => None,
    }
}
