//! BTS7960 motor driver implementation using ESP32 LEDC PWM.
//!
//! The BTS7960 is controlled via two PWM signals:
//! - L_PWM (GPIO2): Forward direction PWM
//! - R_PWM (GPIO3): Reverse direction PWM
//!
//! Control logic:
//! - Forward: L_PWM = duty%, R_PWM = 0%
//! - Reverse: L_PWM = 0%, R_PWM = duty%
//! - Stopped: Both = 0%

use crate::traits::{Direction, MotorController};
use esp_idf_hal::ledc::{config::TimerConfig, LedcDriver, LedcTimerDriver, Resolution};
use esp_idf_hal::peripheral::Peripheral;
use esp_idf_hal::prelude::*;

/// BTS7960 motor controller for ESP32.
///
/// Uses the LEDC peripheral for PWM generation at 20kHz with 10-bit resolution
/// (1024 duty steps).
///
/// # Hardware Setup
///
/// Connect to BTS7960 module:
/// - GPIO2 → L_PWM (forward)
/// - GPIO3 → R_PWM (reverse)
/// - R_EN + L_EN → jumpered to 3.3V (always enabled)
///
/// # Example
///
/// ```ignore
/// use rs_trainz::hal::esp32::Esp32Motor;
/// use rs_trainz::traits::{MotorController, Direction};
///
/// let peripherals = Peripherals::take()?;
/// let mut motor = Esp32Motor::new(
///     peripherals.pins.gpio2,
///     peripherals.pins.gpio3,
///     peripherals.ledc.timer0,
///     peripherals.ledc.channel0,
///     peripherals.ledc.channel1,
/// )?;
///
/// motor.set_direction(Direction::Forward)?;
/// motor.set_speed(0.5)?; // 50% speed
/// ```
pub struct Esp32Motor<'d> {
    /// Forward PWM channel (L_PWM on BTS7960)
    l_pwm: LedcDriver<'d>,
    /// Reverse PWM channel (R_PWM on BTS7960)
    r_pwm: LedcDriver<'d>,
    /// Current speed setting (0.0 to 1.0)
    current_speed: f32,
    /// Current direction
    current_direction: Direction,
}

impl<'d> Esp32Motor<'d> {
    /// PWM frequency in Hz (20kHz is above audible range)
    const PWM_FREQ_HZ: u32 = 20_000;

    /// PWM resolution (10-bit = 1024 steps)
    const PWM_RESOLUTION: Resolution = Resolution::Bits10;

    /// Maximum duty value for 10-bit resolution
    const MAX_DUTY: u32 = 1023;

    /// Creates a new BTS7960 motor controller.
    ///
    /// # Arguments
    ///
    /// * `l_pwm_pin` - GPIO for forward PWM (typically GPIO2)
    /// * `r_pwm_pin` - GPIO for reverse PWM (typically GPIO3)
    /// * `timer` - LEDC timer peripheral
    /// * `l_channel` - LEDC channel for forward PWM
    /// * `r_channel` - LEDC channel for reverse PWM
    ///
    /// # Errors
    ///
    /// Returns an error if PWM initialization fails.
    pub fn new<T, TI, LC, LCI, RC, RCI, LP, LPI, RP, RPI>(
        l_pwm_pin: LP,
        r_pwm_pin: RP,
        timer: T,
        l_channel: LC,
        r_channel: RC,
    ) -> Result<Self, esp_idf_hal::sys::EspError>
    where
        TI: esp_idf_hal::ledc::LedcTimer + 'd,
        T: Peripheral<P = TI> + 'd,
        LCI: esp_idf_hal::ledc::LedcChannel<SpeedMode = TI::SpeedMode> + 'd,
        LC: Peripheral<P = LCI> + 'd,
        RCI: esp_idf_hal::ledc::LedcChannel<SpeedMode = TI::SpeedMode> + 'd,
        RC: Peripheral<P = RCI> + 'd,
        LPI: esp_idf_hal::gpio::OutputPin + 'd,
        LP: Peripheral<P = LPI> + 'd,
        RPI: esp_idf_hal::gpio::OutputPin + 'd,
        RP: Peripheral<P = RPI> + 'd,
    {
        // Configure LEDC timer: 20kHz, 10-bit resolution
        let timer_config = TimerConfig::default()
            .frequency(Self::PWM_FREQ_HZ.Hz())
            .resolution(Self::PWM_RESOLUTION);
        let timer_driver = LedcTimerDriver::new(timer, &timer_config)?;

        // Configure PWM channels
        let l_pwm = LedcDriver::new(l_channel, &timer_driver, l_pwm_pin)?;
        let r_pwm = LedcDriver::new(r_channel, &timer_driver, r_pwm_pin)?;

        let mut motor = Self {
            l_pwm,
            r_pwm,
            current_speed: 0.0,
            current_direction: Direction::Stopped,
        };

        // Ensure motor starts stopped
        motor.apply_pwm()?;

        Ok(motor)
    }

    /// Applies the current speed and direction to the PWM outputs.
    fn apply_pwm(&mut self) -> Result<(), esp_idf_hal::sys::EspError> {
        let duty = (self.current_speed * Self::MAX_DUTY as f32) as u32;

        match self.current_direction {
            Direction::Forward => {
                self.l_pwm.set_duty(duty)?;
                self.r_pwm.set_duty(0)?;
            }
            Direction::Reverse => {
                self.l_pwm.set_duty(0)?;
                self.r_pwm.set_duty(duty)?;
            }
            Direction::Stopped => {
                self.l_pwm.set_duty(0)?;
                self.r_pwm.set_duty(0)?;
            }
        }

        Ok(())
    }

    /// Returns the current speed setting (0.0 to 1.0).
    #[inline]
    pub fn speed(&self) -> f32 {
        self.current_speed
    }

    /// Returns the current direction setting.
    #[inline]
    pub fn direction(&self) -> Direction {
        self.current_direction
    }
}

impl MotorController for Esp32Motor<'_> {
    type Error = esp_idf_hal::sys::EspError;

    fn set_speed(&mut self, speed: f32) -> Result<(), Self::Error> {
        self.current_speed = speed.clamp(0.0, 1.0);
        self.apply_pwm()
    }

    fn set_direction(&mut self, dir: Direction) -> Result<(), Self::Error> {
        self.current_direction = dir;
        self.apply_pwm()
    }

    fn read_current_ma(&self) -> Result<Option<u32>, Self::Error> {
        // Current sensing is handled by Esp32Fault via ADC
        Ok(None)
    }
}
