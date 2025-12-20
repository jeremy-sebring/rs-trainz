//! ESP32-C3 SuperMini train throttle controller.
//!
//! This is the main entry point for the physical hardware controller.
//! It runs a 50Hz control loop that:
//! - Polls the rotary encoder for speed/direction input
//! - Monitors current sense for fault detection
//! - Updates the motor PWM output
//! - Renders state to the OLED display (if enabled)
//! - Serves HTTP API and web UI (if enabled)
//! - Connects to MQTT broker (if enabled)
//!
//! # Hardware Setup
//!
//! See `docs/ESP32_HARDWARE_PLAN.md` for complete wiring diagram.
//!
//! # Build
//!
//! ```bash
//! # Basic (motor + encoder)
//! make esp
//!
//! # With display
//! make esp-display
//!
//! # With WiFi + HTTP
//! make esp-http
//!
//! # With WiFi + MQTT
//! make esp-mqtt
//!
//! # Full (display + http + mqtt)
//! make esp-full
//!
//! # Flash and monitor
//! make flash-monitor
//! ```

use esp_idf_hal::adc::oneshot::AdcDriver;
use esp_idf_hal::peripherals::Peripherals;
use esp_idf_hal::prelude::*;
use rs_trainz::hal::esp32::{Esp32Clock, Esp32Encoder, Esp32Fault, Esp32Motor};
use rs_trainz::traits::{Clock, EncoderInput, FaultDetector};
use rs_trainz::{
    CommandSource, Config, Direction, ThrottleCommand, ThrottleCommandDyn, ThrottleController,
};
use std::thread;
use std::time::Duration;

/// Main loop interval in milliseconds (50Hz = 20ms)
const LOOP_INTERVAL_MS: u64 = 20;

/// Speed adjustment per encoder click (5% per click)
const SPEED_STEP: f32 = 0.05;

/// MQTT state publish interval in loop ticks (every 10 ticks = 200ms at 50Hz)
#[cfg(feature = "esp32-mqtt")]
const MQTT_PUBLISH_INTERVAL: u32 = 10;

fn main() -> anyhow::Result<()> {
    // Initialize ESP-IDF
    esp_idf_hal::sys::link_patches();

    println!();
    println!("================================");
    println!("  rs-trainz SuperMini Controller");
    println!("================================");
    println!();

    // =========================================================================
    // Configuration
    // =========================================================================
    // TODO: Load from NVS or use compile-time env vars
    let config = Config::default()
        .with_wifi(
            rs_trainz::WifiConfig::default()
                .with_ssid(option_env!("WIFI_SSID").unwrap_or(""))
                .with_password(option_env!("WIFI_PASSWORD").unwrap_or("")),
        )
        .with_mqtt(
            rs_trainz::MqttConfig::default()
                .with_host(option_env!("MQTT_HOST").unwrap_or("localhost"))
                .with_topic_prefix("train"),
        )
        .with_web(rs_trainz::WebConfig::default().with_port(80));

    let peripherals = Peripherals::take()?;

    // =========================================================================
    // Initialize Motor (BTS7960 on GPIO2/3)
    // =========================================================================
    let motor = Esp32Motor::new(
        peripherals.pins.gpio2,
        peripherals.pins.gpio3,
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.ledc.channel1,
    )?;
    println!("[OK] Motor initialized (GPIO2/3 PWM)");

    // =========================================================================
    // Initialize Encoder (KY-040 on GPIO6/7/10)
    // =========================================================================
    let mut encoder = Esp32Encoder::new(
        peripherals.pins.gpio6,
        peripherals.pins.gpio7,
        peripherals.pins.gpio10,
    )?;
    println!("[OK] Encoder initialized (GPIO6/7/10)");

    // =========================================================================
    // Initialize Fault Detector (ADC on GPIO4)
    // =========================================================================
    let adc1 = AdcDriver::new(peripherals.adc1)?;
    let mut fault = Esp32Fault::new(&adc1, peripherals.pins.gpio4)?;
    println!("[OK] Fault detector initialized (GPIO4 ADC)");

    // =========================================================================
    // Initialize Display (SSD1306 on GPIO8/9) - Optional
    // =========================================================================
    #[cfg(feature = "display")]
    let mut display = {
        use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
        use rs_trainz::hal::esp32::Esp32Display;

        let i2c = I2cDriver::new(
            peripherals.i2c0,
            peripherals.pins.gpio8, // SDA
            peripherals.pins.gpio9, // SCL
            &I2cConfig::new().baudrate(400.kHz().into()),
        )?;

        let disp =
            Esp32Display::new(i2c).map_err(|e| anyhow::anyhow!("Display init failed: {:?}", e))?;
        println!("[OK] Display initialized (GPIO8/9 I2C)");
        disp
    };

    #[cfg(feature = "display")]
    {
        use rs_trainz::traits::ThrottleDisplay;
        let _ = display.show_message("rs-trainz", Some("Starting..."));
    }

    // =========================================================================
    // Initialize WiFi (required for HTTP and MQTT)
    // =========================================================================
    #[cfg(feature = "wifi")]
    let _wifi = {
        use esp_idf_svc::eventloop::EspSystemEventLoop;
        use esp_idf_svc::nvs::EspDefaultNvsPartition;
        use rs_trainz::hal::esp32::Esp32Wifi;

        if config.wifi.is_configured() {
            let sysloop = EspSystemEventLoop::take()?;
            let nvs = EspDefaultNvsPartition::take()?;

            #[cfg(feature = "display")]
            {
                use rs_trainz::traits::ThrottleDisplay;
                let _ = display.show_message("WiFi", Some("Connecting..."));
            }

            let wifi = Esp32Wifi::new(peripherals.modem, sysloop, Some(nvs), &config.wifi)?;
            println!("[OK] WiFi connected: {:?}", wifi.ip_addr());

            #[cfg(feature = "display")]
            if let Some(ip) = wifi.ip_addr() {
                use rs_trainz::traits::ThrottleDisplay;
                let msg = format!("{}", ip);
                let _ = display.show_message("WiFi OK", Some(&msg));
                thread::sleep(Duration::from_secs(2));
            }

            Some(wifi)
        } else {
            println!("[SKIP] WiFi not configured (set WIFI_SSID/WIFI_PASSWORD)");
            None
        }
    };

    // =========================================================================
    // Initialize HTTP Server (web API + UI)
    // =========================================================================
    #[cfg(feature = "esp32-http")]
    let http_state = {
        use rs_trainz::hal::esp32::{Esp32HttpServer, Esp32SharedState};
        use std::sync::{Arc, Mutex};

        let shared = Arc::new(Mutex::new(Esp32SharedState::default()));
        let _server = Esp32HttpServer::new(&config.web, shared.clone())?;
        println!("[OK] HTTP server started on port {}", config.web.port);
        Some(shared)
    };

    // =========================================================================
    // Initialize MQTT Client
    // =========================================================================
    #[cfg(feature = "esp32-mqtt")]
    let mut mqtt = {
        use rs_trainz::hal::esp32::Esp32Mqtt;

        if config.mqtt.enabled {
            match Esp32Mqtt::new(&config.mqtt) {
                Ok(client) => {
                    println!(
                        "[OK] MQTT connected to {}:{}",
                        config.mqtt.host, config.mqtt.port
                    );
                    Some(client)
                }
                Err(e) => {
                    println!("[WARN] MQTT connection failed: {:?}", e);
                    None
                }
            }
        } else {
            println!("[SKIP] MQTT disabled");
            None
        }
    };

    // =========================================================================
    // Initialize Clock and Controller
    // =========================================================================
    let clock = Esp32Clock::new();
    let mut controller = ThrottleController::new(motor);

    println!();
    println!("Controls:");
    println!("  Rotate encoder: Adjust speed");
    println!("  Press button:   Toggle direction");
    #[cfg(feature = "esp32-http")]
    if let Some(ref _state) = http_state {
        println!("  Web UI:         http://<ip>/");
    }
    println!();
    println!("Starting control loop (50Hz)...");
    println!();

    #[cfg(feature = "esp32-mqtt")]
    let mut mqtt_tick_counter: u32 = 0;

    // =========================================================================
    // Main Control Loop (50Hz)
    // =========================================================================
    loop {
        let now = clock.now_ms();

        // Poll inputs
        encoder.poll();
        fault.poll();

        // ---------------------------------------------------------------------
        // Process HTTP commands
        // ---------------------------------------------------------------------
        #[cfg(feature = "esp32-http")]
        if let Some(ref state) = http_state {
            let mut guard = state.lock().unwrap();
            guard.now_ms = now;

            if let Some(cmd) = guard.pending_command.take() {
                let _ = controller.apply_command(cmd, CommandSource::WebLocal, now);
            }
        }

        // ---------------------------------------------------------------------
        // Process MQTT commands
        // ---------------------------------------------------------------------
        #[cfg(feature = "esp32-mqtt")]
        if let Some(ref mut client) = mqtt {
            while let Some(cmd) = client.recv_command() {
                let _ = controller.apply_command(cmd, CommandSource::Mqtt, now);
            }
        }

        // ---------------------------------------------------------------------
        // Speed control via encoder rotation
        // ---------------------------------------------------------------------
        let delta = encoder.read_delta();
        if delta != 0 {
            let current = controller.current_speed();
            let new_speed = (current + delta as f32 * SPEED_STEP).clamp(0.0, 1.0);
            let cmd = ThrottleCommand::speed_immediate(new_speed);
            let _ = controller.apply_command(cmd.into(), CommandSource::Physical, now);
            println!("Speed: {:.0}%", new_speed * 100.0);
        }

        // ---------------------------------------------------------------------
        // Direction toggle via encoder button
        // ---------------------------------------------------------------------
        if encoder.button_just_pressed() {
            let new_dir = match controller.current_direction() {
                Direction::Forward => Direction::Reverse,
                Direction::Reverse => Direction::Forward,
                Direction::Stopped => Direction::Forward,
            };
            let cmd = ThrottleCommandDyn::SetDirection(new_dir);
            let _ = controller.apply_command(cmd, CommandSource::Physical, now);
            println!("Direction: {:?}", new_dir);
        }

        // ---------------------------------------------------------------------
        // Fault detection
        // ---------------------------------------------------------------------
        if let Some(fault_kind) = fault.active_fault() {
            println!("!! FAULT: {:?} !!", fault_kind);
            let _ = controller.handle_fault(fault_kind);
        }

        // ---------------------------------------------------------------------
        // Update controller (applies transitions, updates motor)
        // ---------------------------------------------------------------------
        let _ = controller.update(now);

        // Get current state for display/network
        let state = controller.state(now);

        // ---------------------------------------------------------------------
        // Update HTTP shared state
        // ---------------------------------------------------------------------
        #[cfg(feature = "esp32-http")]
        if let Some(ref http) = http_state {
            let mut guard = http.lock().unwrap();
            guard.state = state.clone();
        }

        // ---------------------------------------------------------------------
        // Publish MQTT state periodically
        // ---------------------------------------------------------------------
        #[cfg(feature = "esp32-mqtt")]
        {
            mqtt_tick_counter += 1;
            if mqtt_tick_counter >= MQTT_PUBLISH_INTERVAL {
                mqtt_tick_counter = 0;
                if let Some(ref mut client) = mqtt {
                    let _ = client.publish_state(&state);
                }
            }
        }

        // ---------------------------------------------------------------------
        // Update display
        // ---------------------------------------------------------------------
        #[cfg(feature = "display")]
        {
            use rs_trainz::traits::ThrottleDisplay;
            let _ = display.render(&state);
        }

        // Sleep until next tick
        thread::sleep(Duration::from_millis(LOOP_INTERVAL_MS));
    }
}
