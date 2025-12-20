//! BTS7960 current sensing and fault detection via ESP32 ADC.
//!
//! The BTS7960 provides current sense outputs (L_IS and R_IS) that output
//! a voltage proportional to motor current. This module reads these via ADC
//! and detects short circuit and overcurrent conditions.
//!
//! # Wiring
//!
//! - L_IS → GPIO4 (current sense, directly usable since only one direction active at a time)
//!
//! Note: On ESP32-C3, GPIO4 is on ADC1. GPIO5 is on ADC2 which has limitations.
//! Since only one motor direction is active at a time, we use a single ADC channel.
//! Wire both L_IS and R_IS together to GPIO4, or just use L_IS.
//!
//! # Calibration
//!
//! The BTS7960 IS outputs ~8.5µA per amp of motor current through the sense
//! resistor. The exact conversion factor depends on your module's sense resistor
//! value and requires calibration with known loads.

use crate::traits::FaultDetector;
use esp_idf_hal::adc::attenuation::DB_11;
use esp_idf_hal::adc::oneshot::config::AdcChannelConfig;
use esp_idf_hal::adc::oneshot::{AdcChannelDriver, AdcDriver};
use esp_idf_hal::adc::ADC1;
use esp_idf_hal::gpio::Gpio4;
use esp_idf_hal::peripheral::Peripheral;

/// BTS7960 fault detector using ADC current sensing.
///
/// Monitors current via GPIO4 ADC and triggers faults when thresholds are exceeded.
///
/// # Example
///
/// ```ignore
/// use rs_trainz::hal::esp32::Esp32Fault;
/// use rs_trainz::traits::FaultDetector;
///
/// let peripherals = Peripherals::take()?;
/// let mut fault = Esp32Fault::new(
///     peripherals.adc1,
///     peripherals.pins.gpio4,
/// )?;
///
/// loop {
///     fault.poll();
///     if let Some(kind) = fault.active_fault() {
///         println!("FAULT: {:?}, current: {:?}mA", kind, fault.fault_current_ma());
///     }
/// }
/// ```
pub struct Esp32Fault<'d> {
    /// Current sense channel (L_IS on GPIO4)
    current_sense: AdcChannelDriver<'d, Gpio4, &'d AdcDriver<'d, ADC1>>,
    /// Short circuit threshold (raw ADC value, 0-4095)
    short_threshold_raw: u16,
    /// Overcurrent threshold (raw ADC value, 0-4095)
    overcurrent_threshold_raw: u16,
    /// Last sampled current (raw ADC value)
    last_current_raw: u16,
}

impl<'d> Esp32Fault<'d> {
    /// Default short circuit threshold (~2.8V out of 3.3V range).
    const DEFAULT_SHORT_THRESHOLD: u16 = 3500;

    /// Default overcurrent threshold (~2.0V out of 3.3V range).
    const DEFAULT_OVERCURRENT_THRESHOLD: u16 = 2500;

    /// Creates a new fault detector.
    ///
    /// # Arguments
    ///
    /// * `adc` - Reference to ADC1 driver (must outlive this struct)
    /// * `current_pin` - GPIO4 for current sense
    ///
    /// # Errors
    ///
    /// Returns an error if ADC channel initialization fails.
    pub fn new(
        adc: &'d AdcDriver<'d, ADC1>,
        current_pin: impl Peripheral<P = Gpio4> + 'd,
    ) -> Result<Self, esp_idf_hal::sys::EspError> {
        let config = AdcChannelConfig {
            attenuation: DB_11,
            ..Default::default()
        };
        let current_sense = AdcChannelDriver::new(adc, current_pin, &config)?;

        Ok(Self {
            current_sense,
            short_threshold_raw: Self::DEFAULT_SHORT_THRESHOLD,
            overcurrent_threshold_raw: Self::DEFAULT_OVERCURRENT_THRESHOLD,
            last_current_raw: 0,
        })
    }

    /// Polls the current sense ADC channel. Call every loop iteration.
    pub fn poll(&mut self) {
        self.last_current_raw = self.current_sense.read().unwrap_or(0);
    }

    /// Sets the fault detection thresholds.
    pub fn set_thresholds(&mut self, short: u16, overcurrent: u16) {
        self.short_threshold_raw = short;
        self.overcurrent_threshold_raw = overcurrent;
    }

    /// Returns the last sampled raw ADC value.
    #[inline]
    pub fn raw_current(&self) -> u16 {
        self.last_current_raw
    }

    /// Converts raw ADC value to approximate milliamps.
    fn raw_to_ma(&self, raw: u16) -> u32 {
        (raw as u32) * 10
    }
}

impl FaultDetector for Esp32Fault<'_> {
    fn is_short_circuit(&self) -> bool {
        self.last_current_raw > self.short_threshold_raw
    }

    fn is_overcurrent(&self) -> bool {
        self.last_current_raw > self.overcurrent_threshold_raw
    }

    fn fault_current_ma(&self) -> Option<u32> {
        Some(self.raw_to_ma(self.last_current_raw))
    }
}
