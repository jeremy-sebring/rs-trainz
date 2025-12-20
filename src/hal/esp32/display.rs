//! SSD1306 OLED display implementation for ESP32.
//!
//! Provides a 128x64 pixel display for showing throttle state including:
//! - Speed bar graph
//! - Speed percentage
//! - Direction indicator
//! - Fault status
//!
//! # Wiring
//!
//! - SDA → GPIO8 (also has onboard LED)
//! - SCL → GPIO9 (also shared with BOOT button)
//! - VCC → 3.3V
//! - GND → GND

use crate::traits::ThrottleDisplay;
use crate::{Direction, ThrottleState};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use esp_idf_hal::i2c::I2cDriver;
use ssd1306::{mode::BufferedGraphicsMode, prelude::*, I2CDisplayInterface, Ssd1306};

/// SSD1306 display type alias for cleaner code.
type DisplayDriver<'d> = Ssd1306<
    I2CInterface<I2cDriver<'d>>,
    DisplaySize128x64,
    BufferedGraphicsMode<DisplaySize128x64>,
>;

/// SSD1306 OLED display for ESP32.
///
/// Uses I2C on GPIO8 (SDA) and GPIO9 (SCL) to communicate with a 128x64 OLED.
///
/// # Display Layout
///
/// ```text
/// ┌────────────────────────────┐
/// │████████████████░░░░░░░░░░░░│  Speed bar (top)
/// │                            │
/// │  Speed: 75%                │
/// │  Dir: FORWARD ->           │
/// │                            │
/// │  [fault area if needed]    │
/// └────────────────────────────┘
/// ```
pub struct Esp32Display<'d> {
    display: DisplayDriver<'d>,
}

impl<'d> Esp32Display<'d> {
    /// Creates a new display instance.
    ///
    /// # Arguments
    ///
    /// * `i2c` - I2C driver configured for GPIO8/9
    ///
    /// # Errors
    ///
    /// Returns an error if display initialization fails.
    pub fn new(i2c: I2cDriver<'d>) -> Result<Self, DisplayError> {
        let interface = I2CDisplayInterface::new(i2c);
        let display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        Ok(Self { display })
    }
}

impl ThrottleDisplay for Esp32Display<'_> {
    type Error = DisplayError;

    fn init(&mut self) -> Result<(), Self::Error> {
        self.display.init()?;
        self.clear()
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        self.display.clear(BinaryColor::Off)?;
        self.display.flush()?;
        Ok(())
    }

    fn render(&mut self, state: &ThrottleState) -> Result<(), Self::Error> {
        self.display.clear(BinaryColor::Off)?;

        let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
        let fill_style = PrimitiveStyle::with_fill(BinaryColor::On);

        // Speed bar at top (120 pixels wide max, leaving 4px margin each side)
        let bar_width = (state.speed * 120.0) as u32;
        if bar_width > 0 {
            Rectangle::new(Point::new(4, 2), Size::new(bar_width, 8))
                .into_styled(fill_style)
                .draw(&mut self.display)?;
        }

        // Speed percentage text
        // Format speed without heap allocation
        let speed_pct = (state.speed * 100.0) as u32;
        let mut speed_buf = [0u8; 16];
        let speed_text = format_speed(&mut speed_buf, speed_pct);
        Text::new(speed_text, Point::new(4, 26), text_style).draw(&mut self.display)?;

        // Direction indicator
        let dir_text = match state.direction {
            Direction::Forward => "Dir: FORWARD ->",
            Direction::Reverse => "Dir: REVERSE <-",
            Direction::Stopped => "Dir: STOPPED",
        };
        Text::new(dir_text, Point::new(4, 40), text_style).draw(&mut self.display)?;

        // Fault status (if any)
        if let Some(ref fault) = state.fault {
            let fault_text = match fault {
                crate::FaultKind::ShortCircuit => "!! SHORT CIRCUIT !!",
                crate::FaultKind::Overcurrent => "!! OVERCURRENT !!",
            };
            Text::new(fault_text, Point::new(4, 58), text_style).draw(&mut self.display)?;
        }

        self.display.flush()?;
        Ok(())
    }

    fn show_message(&mut self, line1: &str, line2: Option<&str>) -> Result<(), Self::Error> {
        self.display.clear(BinaryColor::Off)?;

        let text_style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

        // Center the text vertically
        Text::new(line1, Point::new(4, 24), text_style).draw(&mut self.display)?;

        if let Some(l2) = line2 {
            Text::new(l2, Point::new(4, 40), text_style).draw(&mut self.display)?;
        }

        self.display.flush()?;
        Ok(())
    }
}

/// Formats a speed percentage into a buffer without heap allocation.
///
/// Returns a string slice into the buffer.
fn format_speed(buf: &mut [u8; 16], speed_pct: u32) -> &str {
    // "Speed: XXX%" format
    let prefix = b"Speed: ";
    buf[..7].copy_from_slice(prefix);

    let mut idx = 7;

    // Convert number to string (simple implementation for 0-100)
    if speed_pct >= 100 {
        buf[idx] = b'1';
        buf[idx + 1] = b'0';
        buf[idx + 2] = b'0';
        idx += 3;
    } else if speed_pct >= 10 {
        buf[idx] = b'0' + (speed_pct / 10) as u8;
        buf[idx + 1] = b'0' + (speed_pct % 10) as u8;
        idx += 2;
    } else {
        buf[idx] = b'0' + speed_pct as u8;
        idx += 1;
    }

    buf[idx] = b'%';
    idx += 1;

    // Safety: we only wrote ASCII bytes
    core::str::from_utf8(&buf[..idx]).unwrap_or("Speed: ?%")
}

/// Display error type.
#[derive(Debug)]
pub struct DisplayError;

impl From<display_interface::DisplayError> for DisplayError {
    fn from(_: display_interface::DisplayError) -> Self {
        DisplayError
    }
}
