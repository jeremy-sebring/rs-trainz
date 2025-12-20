//! Display abstraction for throttle state visualization.
//!
//! This module defines the [`ThrottleDisplay`] trait for rendering throttle
//! state to various display devices (OLED, LCD, etc.).

use crate::ThrottleState;

/// Display trait for rendering throttle state.
///
/// Implementors provide hardware-specific rendering for displays like
/// SSD1306 OLED, character LCDs, or simulated displays for testing.
///
/// # Example
///
/// ```ignore
/// use rs_trainz::traits::ThrottleDisplay;
/// use rs_trainz::ThrottleState;
///
/// struct MyDisplay { /* ... */ }
///
/// impl ThrottleDisplay for MyDisplay {
///     type Error = ();
///     
///     fn init(&mut self) -> Result<(), ()> { Ok(()) }
///     fn clear(&mut self) -> Result<(), ()> { Ok(()) }
///     fn render(&mut self, state: &ThrottleState) -> Result<(), ()> {
///         // Render speed bar, direction, etc.
///         Ok(())
///     }
///     fn show_message(&mut self, line1: &str, line2: Option<&str>) -> Result<(), ()> {
///         Ok(())
///     }
/// }
/// ```
pub trait ThrottleDisplay {
    /// Error type for display operations.
    type Error;

    /// Initializes the display hardware.
    ///
    /// Called once at startup. Implementations should:
    /// - Configure display controller
    /// - Clear the screen
    /// - Set up any required modes
    fn init(&mut self) -> Result<(), Self::Error>;

    /// Clears the display.
    fn clear(&mut self) -> Result<(), Self::Error>;

    /// Renders the current throttle state.
    ///
    /// This is the main rendering method, called each update cycle.
    /// Implementations should display:
    /// - Speed (as percentage and/or bar graph)
    /// - Direction (Forward/Reverse/Stopped)
    /// - Fault status if any
    /// - Transition progress if any
    fn render(&mut self, state: &ThrottleState) -> Result<(), Self::Error>;

    /// Shows a simple message (e.g., for startup or errors).
    ///
    /// # Arguments
    ///
    /// * `line1` - First line of text
    /// * `line2` - Optional second line of text
    fn show_message(&mut self, line1: &str, line2: Option<&str>) -> Result<(), Self::Error>;
}
