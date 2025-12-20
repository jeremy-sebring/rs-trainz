//! WiFi connection management for ESP32-C3.
//!
//! Provides synchronous WiFi station mode connection using esp-idf-svc.
//!
//! # Example
//!
//! ```ignore
//! use rs_trainz::hal::esp32::Esp32Wifi;
//! use rs_trainz::config::WifiConfig;
//!
//! let config = WifiConfig::default()
//!     .with_ssid("MyNetwork")
//!     .with_password("secret123");
//!
//! let wifi = Esp32Wifi::new(modem, sysloop, nvs, &config)?;
//! // WiFi is now connected and has an IP address
//! println!("IP: {:?}", wifi.ip_info());
//! ```

use crate::config::WifiConfig;
use esp_idf_hal::modem::Modem;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use std::net::Ipv4Addr;

/// WiFi connection manager for ESP32.
///
/// Manages a station-mode WiFi connection. The connection is established
/// during construction and maintained for the lifetime of this struct.
pub struct Esp32Wifi<'a> {
    wifi: BlockingWifi<EspWifi<'a>>,
}

impl<'a> Esp32Wifi<'a> {
    /// Create a new WiFi connection.
    ///
    /// This will:
    /// 1. Initialize the WiFi driver
    /// 2. Configure station mode with the provided credentials
    /// 3. Connect to the access point
    /// 4. Wait for DHCP to assign an IP address
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - WiFi initialization fails
    /// - Connection to AP fails
    /// - DHCP times out
    pub fn new(
        modem: Modem,
        sysloop: EspSystemEventLoop,
        nvs: Option<EspDefaultNvsPartition>,
        config: &WifiConfig,
    ) -> anyhow::Result<Self> {
        let esp_wifi = EspWifi::new(modem, sysloop.clone(), nvs)?;
        let mut wifi = BlockingWifi::wrap(esp_wifi, sysloop)?;

        // Configure station mode
        let ssid = config.ssid.as_str();
        let password = config.password.as_str();

        // Create heapless strings for esp-idf
        let mut ssid_buf: heapless::String<32> = heapless::String::new();
        let _ = ssid_buf.push_str(ssid);

        let mut pass_buf: heapless::String<64> = heapless::String::new();
        let _ = pass_buf.push_str(password);

        wifi.set_configuration(&Configuration::Client(ClientConfiguration {
            ssid: ssid_buf,
            password: pass_buf,
            ..Default::default()
        }))?;

        println!("[WiFi] Starting...");
        wifi.start()?;

        println!("[WiFi] Connecting to '{}'...", ssid);
        wifi.connect()?;

        println!("[WiFi] Waiting for DHCP...");
        wifi.wait_netif_up()?;

        if let Some(ip_info) = wifi.wifi().sta_netif().get_ip_info().ok() {
            println!("[WiFi] Connected! IP: {}", ip_info.ip);
        }

        Ok(Self { wifi })
    }

    /// Get the current IP address, if connected.
    pub fn ip_addr(&self) -> Option<Ipv4Addr> {
        self.wifi
            .wifi()
            .sta_netif()
            .get_ip_info()
            .ok()
            .map(|info| info.ip)
    }

    /// Check if WiFi is connected.
    pub fn is_connected(&self) -> bool {
        self.wifi.is_connected().unwrap_or(false)
    }

    /// Disconnect from the current network.
    pub fn disconnect(&mut self) -> anyhow::Result<()> {
        self.wifi.disconnect()?;
        Ok(())
    }

    /// Get the underlying WiFi driver for advanced operations.
    pub fn driver(&self) -> &EspWifi<'a> {
        self.wifi.wifi()
    }

    /// Get mutable access to the underlying WiFi driver.
    pub fn driver_mut(&mut self) -> &mut EspWifi<'a> {
        self.wifi.wifi_mut()
    }
}
