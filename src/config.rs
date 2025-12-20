//! Shared configuration system for desktop and ESP32.
//!
//! Uses `heapless::String` for `no_std` compatibility while remaining
//! ergonomic to use on desktop with `std`.
//!
//! # Example
//!
//! ```rust
//! use rs_trainz::config::{Config, MqttConfig, WebConfig};
//!
//! // Use defaults
//! let config = Config::default();
//!
//! // Or customize
//! let config = Config::default()
//!     .with_mqtt(MqttConfig::default().with_host("192.168.1.100"))
//!     .with_web(WebConfig::default().with_port(3000));
//! ```

use heapless::String as HString;

/// Maximum length for short config strings (hostnames, client IDs)
pub const MAX_SHORT_STRING: usize = 64;

/// Maximum length for longer config strings (topic prefixes, paths)
pub const MAX_LONG_STRING: usize = 128;

/// Type alias for short config strings
pub type ShortString = HString<MAX_SHORT_STRING>;

/// Type alias for longer config strings
pub type LongString = HString<MAX_LONG_STRING>;

// ============================================================================
// Helper for creating heapless strings
// ============================================================================

/// Create a ShortString from a &str, truncating if too long
pub fn short_string(s: &str) -> ShortString {
    let mut hs = ShortString::new();
    // Take only what fits
    let take = s.len().min(MAX_SHORT_STRING);
    // Find valid UTF-8 boundary
    let valid_end = s
        .char_indices()
        .take_while(|(i, _)| *i < take)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let _ = hs.push_str(&s[..valid_end]);
    hs
}

/// Create a LongString from a &str, truncating if too long
pub fn long_string(s: &str) -> LongString {
    let mut hs = LongString::new();
    let take = s.len().min(MAX_LONG_STRING);
    let valid_end = s
        .char_indices()
        .take_while(|(i, _)| *i < take)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);
    let _ = hs.push_str(&s[..valid_end]);
    hs
}

// ============================================================================
// Main Config
// ============================================================================

/// Complete application configuration
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Config {
    /// WiFi connection configuration
    pub wifi: WifiConfig,
    /// MQTT client configuration
    pub mqtt: MqttConfig,
    /// Web server configuration
    pub web: WebConfig,
    /// Throttle controller configuration
    pub throttle: ThrottleConfig,
    /// Device identification
    pub device: DeviceConfig,
}

impl Config {
    /// Set WiFi configuration
    pub fn with_wifi(mut self, wifi: WifiConfig) -> Self {
        self.wifi = wifi;
        self
    }

    /// Set MQTT configuration
    pub fn with_mqtt(mut self, mqtt: MqttConfig) -> Self {
        self.mqtt = mqtt;
        self
    }

    /// Set web configuration
    pub fn with_web(mut self, web: WebConfig) -> Self {
        self.web = web;
        self
    }

    /// Set throttle configuration
    pub fn with_throttle(mut self, throttle: ThrottleConfig) -> Self {
        self.throttle = throttle;
        self
    }

    /// Set device configuration
    pub fn with_device(mut self, device: DeviceConfig) -> Self {
        self.device = device;
        self
    }
}

// ============================================================================
// MQTT Config
// ============================================================================

/// MQTT client configuration
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MqttConfig {
    /// Broker hostname or IP
    pub host: ShortString,
    /// Broker port
    pub port: u16,
    /// Client ID (should be unique per device)
    pub client_id: ShortString,
    /// Topic prefix for all pub/sub (e.g., "train" -> "train/speed")
    pub topic_prefix: ShortString,
    /// Username for authentication (empty = no auth)
    pub username: ShortString,
    /// Password for authentication
    pub password: ShortString,
    /// Heartbeat/state publish interval in milliseconds
    pub heartbeat_ms: u32,
    /// Keep-alive interval in seconds
    pub keep_alive_secs: u16,
    /// Whether MQTT is enabled
    pub enabled: bool,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: short_string("localhost"),
            port: 1883,
            client_id: short_string("rs-trainz"),
            topic_prefix: short_string("train"),
            username: ShortString::new(),
            password: ShortString::new(),
            heartbeat_ms: 5000,
            keep_alive_secs: 30,
            enabled: true,
        }
    }
}

impl MqttConfig {
    /// Set the broker host
    pub fn with_host(mut self, host: &str) -> Self {
        self.host = short_string(host);
        self
    }

    /// Set the broker port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set the client ID
    pub fn with_client_id(mut self, id: &str) -> Self {
        self.client_id = short_string(id);
        self
    }

    /// Set the topic prefix
    pub fn with_topic_prefix(mut self, prefix: &str) -> Self {
        self.topic_prefix = short_string(prefix);
        self
    }

    /// Set authentication credentials
    pub fn with_auth(mut self, username: &str, password: &str) -> Self {
        self.username = short_string(username);
        self.password = short_string(password);
        self
    }

    /// Set the heartbeat interval
    pub fn with_heartbeat_ms(mut self, ms: u32) -> Self {
        self.heartbeat_ms = ms;
        self
    }

    /// Enable or disable MQTT
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Build a topic string with the configured prefix
    pub fn topic(&self, suffix: &str) -> LongString {
        let mut topic = LongString::new();
        let _ = topic.push_str(self.topic_prefix.as_str());
        let _ = topic.push('/');
        let _ = topic.push_str(suffix);
        topic
    }

    /// Check if authentication is configured
    pub fn has_auth(&self) -> bool {
        !self.username.is_empty()
    }
}

// ============================================================================
// Web Config
// ============================================================================

/// Web server configuration
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WebConfig {
    /// Port to listen on
    pub port: u16,
    /// Whether to enable CORS for all origins
    pub cors_permissive: bool,
    /// Polling interval hint for web UI (milliseconds)
    pub poll_interval_ms: u32,
    /// Whether web server is enabled
    pub enabled: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            cors_permissive: true,
            poll_interval_ms: 200,
            enabled: true,
        }
    }
}

impl WebConfig {
    /// Set the port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set CORS mode
    pub fn with_cors(mut self, permissive: bool) -> Self {
        self.cors_permissive = permissive;
        self
    }

    /// Set the poll interval hint
    pub fn with_poll_interval_ms(mut self, ms: u32) -> Self {
        self.poll_interval_ms = ms;
        self
    }

    /// Enable or disable web server
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

// ============================================================================
// Throttle Config
// ============================================================================

/// Throttle controller configuration
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ThrottleConfig {
    /// Maximum allowed speed (0.0 to 1.0)
    pub max_speed: f32,
    /// Default transition duration in milliseconds
    pub default_transition_ms: u32,
    /// Whether to use smooth (ease-in-out) transitions by default
    pub default_smooth: bool,
    /// Controller update interval in milliseconds
    pub update_interval_ms: u32,
    /// Physical control lockout duration in milliseconds
    pub lockout_ms: u32,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            max_speed: 1.0,
            default_transition_ms: 500,
            default_smooth: true,
            update_interval_ms: 20,
            lockout_ms: 2000,
        }
    }
}

impl ThrottleConfig {
    /// Set the maximum speed
    pub fn with_max_speed(mut self, max: f32) -> Self {
        self.max_speed = max.clamp(0.0, 1.0);
        self
    }

    /// Set the default transition duration
    pub fn with_default_transition_ms(mut self, ms: u32) -> Self {
        self.default_transition_ms = ms;
        self
    }

    /// Set whether smooth transitions are default
    pub fn with_default_smooth(mut self, smooth: bool) -> Self {
        self.default_smooth = smooth;
        self
    }

    /// Set the update interval
    pub fn with_update_interval_ms(mut self, ms: u32) -> Self {
        self.update_interval_ms = ms;
        self
    }

    /// Set the lockout duration
    pub fn with_lockout_ms(mut self, ms: u32) -> Self {
        self.lockout_ms = ms;
        self
    }
}

// ============================================================================
// WiFi Config
// ============================================================================

/// WiFi connection configuration
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WifiConfig {
    /// WiFi network SSID
    pub ssid: ShortString,
    /// WiFi password
    pub password: ShortString,
    /// Connection timeout in milliseconds
    pub connect_timeout_ms: u32,
    /// Whether WiFi is enabled
    pub enabled: bool,
    /// Maximum connection retry attempts (0 = unlimited)
    pub max_retries: u8,
}

impl Default for WifiConfig {
    fn default() -> Self {
        Self {
            ssid: ShortString::new(),
            password: ShortString::new(),
            connect_timeout_ms: 30_000,
            enabled: true,
            max_retries: 5,
        }
    }
}

impl WifiConfig {
    /// Set the SSID
    pub fn with_ssid(mut self, ssid: &str) -> Self {
        self.ssid = short_string(ssid);
        self
    }

    /// Set the password
    pub fn with_password(mut self, password: &str) -> Self {
        self.password = short_string(password);
        self
    }

    /// Set the connection timeout
    pub fn with_connect_timeout_ms(mut self, ms: u32) -> Self {
        self.connect_timeout_ms = ms;
        self
    }

    /// Enable or disable WiFi
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Set the maximum retry count
    pub fn with_max_retries(mut self, retries: u8) -> Self {
        self.max_retries = retries;
        self
    }

    /// Check if WiFi credentials are configured
    pub fn is_configured(&self) -> bool {
        !self.ssid.is_empty()
    }
}

// ============================================================================
// Device Config
// ============================================================================

/// Device identification configuration
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeviceConfig {
    /// Human-readable device name
    pub name: ShortString,
    /// Device/locomotive ID (for multi-train setups)
    pub id: ShortString,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            name: short_string("rs-trainz"),
            id: short_string("loco1"),
        }
    }
}

impl DeviceConfig {
    /// Set the device name
    pub fn with_name(mut self, name: &str) -> Self {
        self.name = short_string(name);
        self
    }

    /// Set the device ID
    pub fn with_id(mut self, id: &str) -> Self {
        self.id = short_string(id);
        self
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = Config::default();
        assert_eq!(config.mqtt.port, 1883);
        assert_eq!(config.web.port, 8080);
        assert_eq!(config.throttle.max_speed, 1.0);
    }

    #[test]
    fn mqtt_topic_building() {
        let mqtt = MqttConfig::default().with_topic_prefix("trains/loco1");
        let topic = mqtt.topic("speed/set");
        assert_eq!(topic.as_str(), "trains/loco1/speed/set");
    }

    #[test]
    fn mqtt_auth_detection() {
        let no_auth = MqttConfig::default();
        assert!(!no_auth.has_auth());

        let with_auth = MqttConfig::default().with_auth("user", "pass");
        assert!(with_auth.has_auth());
    }

    #[test]
    fn short_string_truncation() {
        let long_input = "a".repeat(100);
        let s = short_string(&long_input);
        assert!(s.len() <= MAX_SHORT_STRING);
    }

    #[test]
    fn builder_pattern() {
        let config = Config::default()
            .with_mqtt(
                MqttConfig::default()
                    .with_host("broker.local")
                    .with_port(8883),
            )
            .with_web(WebConfig::default().with_port(3000))
            .with_device(DeviceConfig::default().with_name("My Train"));

        assert_eq!(config.mqtt.host.as_str(), "broker.local");
        assert_eq!(config.mqtt.port, 8883);
        assert_eq!(config.web.port, 3000);
        assert_eq!(config.device.name.as_str(), "My Train");
    }

    #[test]
    fn throttle_max_speed_clamped() {
        let config = ThrottleConfig::default().with_max_speed(1.5);
        assert_eq!(config.max_speed, 1.0);

        let config = ThrottleConfig::default().with_max_speed(-0.5);
        assert_eq!(config.max_speed, 0.0);
    }

    // =========================================================================
    // WifiConfig Tests
    // =========================================================================

    #[test]
    fn wifi_config_default() {
        let wifi = WifiConfig::default();
        assert!(wifi.ssid.is_empty());
        assert!(wifi.password.is_empty());
        assert_eq!(wifi.connect_timeout_ms, 30_000);
        assert!(wifi.enabled);
        assert_eq!(wifi.max_retries, 5);
    }

    #[test]
    fn wifi_config_is_configured() {
        let unconfigured = WifiConfig::default();
        assert!(!unconfigured.is_configured());

        let configured = WifiConfig::default().with_ssid("MyNetwork");
        assert!(configured.is_configured());

        let empty_ssid = WifiConfig::default().with_ssid("");
        assert!(!empty_ssid.is_configured());
    }

    #[test]
    fn wifi_config_builder() {
        let wifi = WifiConfig::default()
            .with_ssid("TestNetwork")
            .with_password("secret123")
            .with_connect_timeout_ms(15_000)
            .with_max_retries(3)
            .with_enabled(false);

        assert_eq!(wifi.ssid.as_str(), "TestNetwork");
        assert_eq!(wifi.password.as_str(), "secret123");
        assert_eq!(wifi.connect_timeout_ms, 15_000);
        assert_eq!(wifi.max_retries, 3);
        assert!(!wifi.enabled);
    }

    #[test]
    fn config_with_wifi() {
        let config = Config::default().with_wifi(WifiConfig::default().with_ssid("HomeWifi"));

        assert_eq!(config.wifi.ssid.as_str(), "HomeWifi");
        assert!(config.wifi.is_configured());
    }

    // =========================================================================
    // DeviceConfig Tests
    // =========================================================================

    #[test]
    fn device_config_default() {
        let device = DeviceConfig::default();
        assert_eq!(device.name.as_str(), "rs-trainz");
        assert_eq!(device.id.as_str(), "loco1");
    }

    #[test]
    fn device_config_builder() {
        let device = DeviceConfig::default()
            .with_name("My Locomotive")
            .with_id("train-42");

        assert_eq!(device.name.as_str(), "My Locomotive");
        assert_eq!(device.id.as_str(), "train-42");
    }

    // =========================================================================
    // WebConfig Tests
    // =========================================================================

    #[test]
    fn web_config_default() {
        let web = WebConfig::default();
        assert_eq!(web.port, 8080);
        assert!(web.cors_permissive);
        assert_eq!(web.poll_interval_ms, 200);
        assert!(web.enabled);
    }

    #[test]
    fn web_config_builder() {
        let web = WebConfig::default()
            .with_port(3000)
            .with_cors(false)
            .with_poll_interval_ms(500)
            .with_enabled(false);

        assert_eq!(web.port, 3000);
        assert!(!web.cors_permissive);
        assert_eq!(web.poll_interval_ms, 500);
        assert!(!web.enabled);
    }

    // =========================================================================
    // ThrottleConfig Tests
    // =========================================================================

    #[test]
    fn throttle_config_default() {
        let throttle = ThrottleConfig::default();
        assert_eq!(throttle.max_speed, 1.0);
        assert_eq!(throttle.default_transition_ms, 500);
        assert!(throttle.default_smooth);
        assert_eq!(throttle.update_interval_ms, 20);
        assert_eq!(throttle.lockout_ms, 2000);
    }

    #[test]
    fn throttle_config_builder() {
        let throttle = ThrottleConfig::default()
            .with_default_transition_ms(1000)
            .with_default_smooth(false)
            .with_update_interval_ms(50)
            .with_lockout_ms(5000);

        assert_eq!(throttle.default_transition_ms, 1000);
        assert!(!throttle.default_smooth);
        assert_eq!(throttle.update_interval_ms, 50);
        assert_eq!(throttle.lockout_ms, 5000);
    }

    // =========================================================================
    // MqttConfig Additional Tests
    // =========================================================================

    #[test]
    fn mqtt_config_default() {
        let mqtt = MqttConfig::default();
        assert_eq!(mqtt.host.as_str(), "localhost");
        assert_eq!(mqtt.port, 1883);
        assert_eq!(mqtt.client_id.as_str(), "rs-trainz");
        assert_eq!(mqtt.topic_prefix.as_str(), "train");
        assert!(mqtt.username.is_empty());
        assert!(mqtt.password.is_empty());
        assert_eq!(mqtt.heartbeat_ms, 5000);
        assert_eq!(mqtt.keep_alive_secs, 30);
        assert!(mqtt.enabled);
    }

    #[test]
    fn mqtt_config_full_builder() {
        let mqtt = MqttConfig::default()
            .with_host("broker.example.com")
            .with_port(8883)
            .with_client_id("my-train")
            .with_topic_prefix("trains/loco1")
            .with_auth("user", "pass")
            .with_heartbeat_ms(10000)
            .with_enabled(false);

        assert_eq!(mqtt.host.as_str(), "broker.example.com");
        assert_eq!(mqtt.port, 8883);
        assert_eq!(mqtt.client_id.as_str(), "my-train");
        assert_eq!(mqtt.topic_prefix.as_str(), "trains/loco1");
        assert_eq!(mqtt.username.as_str(), "user");
        assert_eq!(mqtt.password.as_str(), "pass");
        assert_eq!(mqtt.heartbeat_ms, 10000);
        assert!(!mqtt.enabled);
        assert!(mqtt.has_auth());
    }

    // =========================================================================
    // String Helper Tests
    // =========================================================================

    #[test]
    fn long_string_truncation() {
        let long_input = "b".repeat(200);
        let s = long_string(&long_input);
        assert!(s.len() <= MAX_LONG_STRING);
    }

    #[test]
    fn string_helpers_utf8_boundary() {
        // Test with multi-byte UTF-8 characters
        let input = "🚂🚃🚄🚅"; // Each emoji is 4 bytes
        let s = short_string(input);
        // Should not panic and should be valid UTF-8
        assert!(s.len() <= MAX_SHORT_STRING);
        assert!(core::str::from_utf8(s.as_bytes()).is_ok());
    }
}
