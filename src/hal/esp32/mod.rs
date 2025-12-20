//! ESP32-C3 SuperMini hardware abstraction layer for DC train control.
//!
//! This module provides hardware implementations for the ESP32-C3 SuperMini board
//! controlling a DC model train via a BTS7960 motor driver.
//!
//! # Hardware Configuration
//!
//! - **MCU**: ESP32-C3 SuperMini (RISC-V 160MHz, 4MB Flash)
//! - **Motor Driver**: BTS7960 (43A capacity)
//! - **Encoder**: KY-040 rotary encoder with push button
//! - **Display**: SSD1306 128x64 OLED (I2C)
//!
//! # Pin Assignments
//!
//! See the [`pins`] module for GPIO assignments matching the SuperMini layout.

mod clock;
mod encoder;
mod fault;
mod motor;

pub use clock::Esp32Clock;
pub use encoder::Esp32Encoder;
pub use fault::Esp32Fault;
pub use motor::Esp32Motor;

#[cfg(feature = "display")]
mod display;
#[cfg(feature = "display")]
pub use display::Esp32Display;

#[cfg(feature = "wifi")]
mod wifi;
#[cfg(feature = "wifi")]
pub use wifi::Esp32Wifi;

#[cfg(feature = "esp32-http")]
mod http;
#[cfg(feature = "esp32-http")]
#[allow(deprecated)]
pub use http::{Esp32HttpServer, Esp32SharedState, SharedThrottleState};

#[cfg(feature = "esp32-mqtt")]
mod mqtt;
#[cfg(feature = "esp32-mqtt")]
pub use mqtt::{Esp32Mqtt, Esp32MqttError};

/// Pin assignments for SuperMini ESP32-C3.
///
/// These constants match the wiring diagram in the hardware plan:
/// - Motor control via BTS7960 on GPIO2-5
/// - Rotary encoder on GPIO6, 7, 10
/// - I2C display on GPIO8, 9
pub mod pins {
    // =========================================================================
    // Motor Control (BTS7960)
    // =========================================================================

    /// Forward PWM output (L_PWM on BTS7960)
    pub const L_PWM: i32 = 2;

    /// Reverse PWM output (R_PWM on BTS7960)
    pub const R_PWM: i32 = 3;

    /// Forward current sense input (L_IS on BTS7960) - ADC
    pub const L_IS: i32 = 4;

    /// Reverse current sense input (R_IS on BTS7960) - ADC
    pub const R_IS: i32 = 5;

    // =========================================================================
    // Rotary Encoder (KY-040)
    // =========================================================================

    /// Encoder clock/A signal
    pub const ENC_CLK: i32 = 6;

    /// Encoder data/B signal
    pub const ENC_DT: i32 = 7;

    /// Encoder push button (directly on GPIO10, active low)
    pub const ENC_SW: i32 = 10;

    // =========================================================================
    // I2C Display (SSD1306)
    // =========================================================================

    /// I2C data line (also has onboard blue LED - will flicker during I2C)
    pub const I2C_SDA: i32 = 8;

    /// I2C clock line (also shared with BOOT button - only affects programming)
    pub const I2C_SCL: i32 = 9;

    /// Default I2C address for SSD1306 OLED
    pub const OLED_I2C_ADDR: u8 = 0x3C;
}
