//! KY-040 rotary encoder implementation for ESP32.
//!
//! The KY-040 encoder uses quadrature encoding with two output signals (CLK and DT)
//! plus a push button (SW). This implementation uses polling-based decoding.
//!
//! # Wiring
//!
//! - CLK (A) → GPIO6
//! - DT (B) → GPIO7
//! - SW → GPIO10
//! - VCC → 3.3V
//! - GND → GND

use crate::traits::EncoderInput;
use esp_idf_hal::gpio::{Input, InputPin, OutputPin, PinDriver, Pull};
use esp_idf_hal::peripheral::Peripheral;

/// KY-040 rotary encoder for ESP32.
///
/// Uses polling-based quadrature decoding. Call [`poll()`](Self::poll) regularly
/// (e.g., every 20ms) to update the encoder state.
///
/// # Example
///
/// ```ignore
/// use rs_trainz::hal::esp32::Esp32Encoder;
/// use rs_trainz::traits::EncoderInput;
///
/// let peripherals = Peripherals::take()?;
/// let mut encoder = Esp32Encoder::new(
///     peripherals.pins.gpio6,  // CLK
///     peripherals.pins.gpio7,  // DT
///     peripherals.pins.gpio10, // SW
/// )?;
///
/// loop {
///     encoder.poll();
///     let delta = encoder.read_delta();
///     if delta != 0 {
///         println!("Rotated: {}", delta);
///     }
///     if encoder.button_just_pressed() {
///         println!("Button pressed!");
///     }
/// }
/// ```
pub struct Esp32Encoder<'d, CLK, DT, SW>
where
    CLK: InputPin + OutputPin,
    DT: InputPin + OutputPin,
    SW: InputPin + OutputPin,
{
    /// Clock (A) signal input
    clk: PinDriver<'d, CLK, Input>,
    /// Data (B) signal input
    dt: PinDriver<'d, DT, Input>,
    /// Switch (button) input
    sw: PinDriver<'d, SW, Input>,
    /// Last CLK state for edge detection
    last_clk: bool,
    /// Accumulated position (absolute)
    position: i32,
    /// Position at last read_delta() call
    last_read_position: i32,
    /// Last button state for edge detection
    button_last: bool,
    /// Button just-pressed edge flag
    button_edge: bool,
}

impl<'d, CLK, DT, SW> Esp32Encoder<'d, CLK, DT, SW>
where
    CLK: InputPin + OutputPin,
    DT: InputPin + OutputPin,
    SW: InputPin + OutputPin,
{
    /// Creates a new KY-040 encoder instance.
    ///
    /// Configures the GPIO pins with internal pull-up resistors.
    ///
    /// # Arguments
    ///
    /// * `clk_pin` - GPIO for CLK (A) signal (typically GPIO6)
    /// * `dt_pin` - GPIO for DT (B) signal (typically GPIO7)
    /// * `sw_pin` - GPIO for switch/button (typically GPIO10)
    ///
    /// # Errors
    ///
    /// Returns an error if GPIO initialization fails.
    pub fn new(
        clk_pin: impl Peripheral<P = CLK> + 'd,
        dt_pin: impl Peripheral<P = DT> + 'd,
        sw_pin: impl Peripheral<P = SW> + 'd,
    ) -> Result<Self, esp_idf_hal::sys::EspError> {
        let mut clk = PinDriver::input(clk_pin)?;
        let mut dt = PinDriver::input(dt_pin)?;
        let mut sw = PinDriver::input(sw_pin)?;

        // Enable internal pull-ups (KY-040 outputs are open-drain)
        clk.set_pull(Pull::Up)?;
        dt.set_pull(Pull::Up)?;
        sw.set_pull(Pull::Up)?;

        let last_clk = clk.is_high();

        Ok(Self {
            clk,
            dt,
            sw,
            last_clk,
            position: 0,
            last_read_position: 0,
            button_last: false,
            button_edge: false,
        })
    }

    /// Polls the encoder state. Call this every loop iteration.
    ///
    /// This method:
    /// - Reads the CLK and DT signals
    /// - Detects rotation direction on CLK rising edge
    /// - Updates the position counter
    /// - Detects button press edges
    ///
    /// For best results, call at least every 20ms (50Hz).
    pub fn poll(&mut self) {
        let clk = self.clk.is_high();
        let dt = self.dt.is_high();

        // Detect rising edge on CLK
        if clk && !self.last_clk {
            // On CLK rising edge, check DT to determine direction
            // DT high = CCW (counter-clockwise), DT low = CW (clockwise)
            if dt {
                self.position -= 1; // CCW
            } else {
                self.position += 1; // CW
            }
        }
        self.last_clk = clk;

        // Button edge detection (active low - pressed when low)
        let button = self.sw.is_low();
        if button && !self.button_last {
            self.button_edge = true;
        }
        self.button_last = button;
    }

    /// Returns the absolute position counter.
    ///
    /// This is the raw accumulated position since power-on. Use [`read_delta()`](Self::read_delta)
    /// for relative movement since last read.
    #[inline]
    pub fn position(&self) -> i32 {
        self.position
    }

    /// Resets the position counter to zero.
    pub fn reset_position(&mut self) {
        self.position = 0;
        self.last_read_position = 0;
    }
}

impl<CLK, DT, SW> EncoderInput for Esp32Encoder<'_, CLK, DT, SW>
where
    CLK: InputPin + OutputPin,
    DT: InputPin + OutputPin,
    SW: InputPin + OutputPin,
{
    fn read_delta(&mut self) -> i32 {
        let delta = self.position - self.last_read_position;
        self.last_read_position = self.position;
        delta
    }

    fn button_pressed(&self) -> bool {
        self.sw.is_low() // Active low
    }

    fn button_just_pressed(&mut self) -> bool {
        let edge = self.button_edge;
        self.button_edge = false;
        edge
    }
}
