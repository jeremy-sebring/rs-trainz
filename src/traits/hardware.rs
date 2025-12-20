//! Hardware abstraction traits for motor control, encoder input, and fault detection.
//!
//! This module defines the core hardware interfaces that allow rs-trainz to
//! work across different platforms (ESP32, desktop mocks, etc.).
//!
//! # Key Traits
//!
//! | Trait | Purpose |
//! |-------|---------|
//! | [`MotorController`] | PWM-based DC motor control |
//! | [`EncoderInput`] | Rotary encoder for physical UI |
//! | [`FaultDetector`] | Overcurrent and short circuit detection |
//! | [`Clock`] | Time source for `no_std` environments |
//! | [`Delay`] | Async delay for embedded systems |
//!
//! # Implementation
//!
//! For testing and desktop development, use the mock implementations
//! from [`crate::hal::mock`]. For ESP32 hardware, use the
//! implementations from `hal::esp32` (requires `esp32` feature).
//!
//! # Example
//!
//! ```rust
//! use rs_trainz::traits::{MotorController, Direction};
//! use rs_trainz::hal::MockMotor;
//!
//! let mut motor = MockMotor::new();
//! motor.set_direction(Direction::Forward).unwrap();
//! motor.set_speed(0.5).unwrap();
//!
//! // Check current draw
//! if let Ok(Some(current)) = motor.read_current_ma() {
//!     println!("Current: {}mA", current);
//! }
//! ```

/// Direction of train travel.
///
/// Controls the polarity of the motor output. For DC motors, this typically
/// means swapping the H-bridge outputs.
///
/// # Default
///
/// Defaults to [`Stopped`](Self::Stopped) for safety.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Direction {
    /// Moving forward (positive polarity).
    Forward,
    /// Moving in reverse (negative polarity).
    Reverse,
    /// Not moving (motor stopped).
    ///
    /// May also apply braking depending on motor driver.
    #[default]
    Stopped,
}

impl Direction {
    /// Returns the direction as a lowercase string.
    ///
    /// This is useful for JSON serialization and display purposes.
    ///
    /// # Examples
    ///
    /// ```
    /// use rs_trainz::Direction;
    ///
    /// assert_eq!(Direction::Forward.as_str(), "forward");
    /// assert_eq!(Direction::Reverse.as_str(), "reverse");
    /// assert_eq!(Direction::Stopped.as_str(), "stopped");
    /// ```
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Direction::Forward => "forward",
            Direction::Reverse => "reverse",
            Direction::Stopped => "stopped",
        }
    }

    /// Parse direction from text input.
    ///
    /// Supports multiple text formats for flexibility:
    /// - Full names: `"forward"`, `"reverse"`, `"stopped"`
    /// - Abbreviations: `"fwd"`, `"rev"`, `"stop"`
    /// - Numeric: `"1"` (forward), `"-1"` (reverse), `"0"` (stopped)
    ///
    /// Input is trimmed and case-insensitive.
    ///
    /// # Examples
    ///
    /// ```
    /// use rs_trainz::Direction;
    ///
    /// assert_eq!(Direction::from_text("forward"), Some(Direction::Forward));
    /// assert_eq!(Direction::from_text("fwd"), Some(Direction::Forward));
    /// assert_eq!(Direction::from_text("1"), Some(Direction::Forward));
    ///
    /// assert_eq!(Direction::from_text("reverse"), Some(Direction::Reverse));
    /// assert_eq!(Direction::from_text("rev"), Some(Direction::Reverse));
    /// assert_eq!(Direction::from_text("-1"), Some(Direction::Reverse));
    ///
    /// assert_eq!(Direction::from_text("stopped"), Some(Direction::Stopped));
    /// assert_eq!(Direction::from_text("stop"), Some(Direction::Stopped));
    /// assert_eq!(Direction::from_text("0"), Some(Direction::Stopped));
    ///
    /// assert_eq!(Direction::from_text("invalid"), None);
    /// assert_eq!(Direction::from_text("  FWD  "), Some(Direction::Forward));
    /// ```
    pub fn from_text(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "forward" | "fwd" | "1" => Some(Direction::Forward),
            "reverse" | "rev" | "-1" => Some(Direction::Reverse),
            "stopped" | "stop" | "0" => Some(Direction::Stopped),
            _ => None,
        }
    }
}

/// Motor controller trait - abstracts PWM-based DC motor control.
///
/// Implement this trait for your motor driver hardware. The trait handles
/// speed (via PWM duty cycle) and direction (via H-bridge control).
///
/// # Implementation Notes
///
/// - Speed should be clamped to 0.0-1.0 before applying to PWM
/// - Direction changes should be applied atomically with motor disabled
/// - Current sensing is optional; return `Ok(None)` if not available
///
/// # Example Implementation
///
/// ```rust,ignore
/// use rs_trainz::traits::{MotorController, Direction};
///
/// struct MyMotor { /* hardware handles */ }
///
/// impl MotorController for MyMotor {
///     type Error = ();
///
///     fn set_speed(&mut self, speed: f32) -> Result<(), ()> {
///         let duty = (speed.clamp(0.0, 1.0) * 255.0) as u8;
///         // Set PWM duty cycle...
///         Ok(())
///     }
///
///     fn set_direction(&mut self, dir: Direction) -> Result<(), ()> {
///         // Set H-bridge pins...
///         Ok(())
///     }
///
///     fn read_current_ma(&self) -> Result<Option<u32>, ()> {
///         // Read ADC for current sense...
///         Ok(Some(500))
///     }
/// }
/// ```
pub trait MotorController {
    /// Error type for motor operations.
    type Error;

    /// Set speed as 0.0 to 1.0 (percentage of max voltage).
    ///
    /// Values outside this range should be clamped.
    fn set_speed(&mut self, speed: f32) -> Result<(), Self::Error>;

    /// Set direction of travel.
    ///
    /// This controls the H-bridge polarity. For safety, consider
    /// disabling the motor briefly during direction changes.
    fn set_direction(&mut self, dir: Direction) -> Result<(), Self::Error>;

    /// Read current draw in milliamps (if hardware supports it).
    ///
    /// Returns `Ok(None)` if current sensing is not available.
    fn read_current_ma(&self) -> Result<Option<u32>, Self::Error>;

    /// Convenience method to stop the motor.
    ///
    /// Sets speed to 0 and direction to [`Direction::Stopped`].
    fn stop(&mut self) -> Result<(), Self::Error> {
        self.set_speed(0.0)?;
        self.set_direction(Direction::Stopped)
    }
}

/// Rotary encoder input trait.
///
/// Abstracts a rotary encoder with push button for physical throttle control.
/// Typically connected to GPIO pins with hardware or software debouncing.
///
/// # Implementation Notes
///
/// - `read_delta()` should return accumulated clicks and reset the counter
/// - Positive values = clockwise rotation (speed increase)
/// - The button is typically used for e-stop or menu selection
pub trait EncoderInput {
    /// Returns delta clicks since last call (positive = clockwise).
    ///
    /// This should reset the internal counter after reading.
    fn read_delta(&mut self) -> i32;

    /// Returns true if the encoder button is currently pressed.
    fn button_pressed(&self) -> bool;

    /// Returns true if button was just pressed (edge detection).
    ///
    /// Default implementation just returns `button_pressed()`.
    /// Override for proper edge detection.
    fn button_just_pressed(&mut self) -> bool {
        self.button_pressed()
    }
}

/// Fault detection trait for short circuits and overcurrent.
///
/// Monitors the motor driver for dangerous conditions. When a fault
/// is detected, the throttle controller should immediately stop the motor.
///
/// # Implementation Notes
///
/// - Short circuit detection typically uses a dedicated fault pin
/// - Overcurrent uses ADC current sense with configurable threshold
/// - Some motor drivers (like BTS7960) have built-in protection
pub trait FaultDetector {
    /// Returns true if a short circuit is detected.
    ///
    /// This typically indicates a dead short on the track or derailment.
    fn is_short_circuit(&self) -> bool;

    /// Returns true if current exceeds safe threshold.
    ///
    /// Threshold should be set based on motor and track capacity.
    fn is_overcurrent(&self) -> bool;

    /// Returns the fault current in milliamps if available.
    ///
    /// Useful for logging and diagnostics.
    fn fault_current_ma(&self) -> Option<u32>;

    /// Returns any active fault.
    ///
    /// Prioritizes short circuit over overcurrent.
    fn active_fault(&self) -> Option<FaultKind> {
        if self.is_short_circuit() {
            Some(FaultKind::ShortCircuit)
        } else if self.is_overcurrent() {
            Some(FaultKind::Overcurrent)
        } else {
            None
        }
    }
}

/// Types of faults that can occur.
///
/// Used by [`FaultDetector`] and [`ThrottleController`] to represent
/// dangerous conditions that require motor shutdown.
///
/// [`ThrottleController`]: crate::ThrottleController
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum FaultKind {
    /// Short circuit detected.
    ///
    /// Typically indicates a dead short on the track, derailed train,
    /// or damaged wiring. Motor should be stopped immediately.
    ShortCircuit,

    /// Current exceeds safe threshold.
    ///
    /// The motor is drawing more current than configured safe limit.
    /// May indicate overload, stall, or partial short.
    Overcurrent,
}

/// Time source trait for `no_std` compatibility.
///
/// Provides monotonic time in milliseconds for transition timing.
/// On desktop, this can wrap `std::time::Instant`. On embedded,
/// use a hardware timer.
///
/// # Example
///
/// ```rust
/// use rs_trainz::traits::Clock;
/// use rs_trainz::hal::MockClock;
///
/// let mut clock = MockClock::new();
/// assert_eq!(clock.now_ms(), 0);
///
/// clock.advance(100);
/// assert_eq!(clock.now_ms(), 100);
/// ```
pub trait Clock {
    /// Returns current time in milliseconds since an arbitrary epoch.
    ///
    /// Must be monotonically increasing.
    fn now_ms(&self) -> u64;
}

/// Async delay trait for embedded systems.
///
/// Used for non-blocking delays in async contexts. On ESP32,
/// this typically wraps the embassy timer.
pub trait Delay {
    /// Delay for the specified number of milliseconds.
    fn delay_ms(&mut self, ms: u32) -> impl core::future::Future<Output = ()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Direction Tests
    // =========================================================================

    #[test]
    fn direction_default() {
        let dir = Direction::default();
        assert_eq!(dir, Direction::Stopped);
    }

    #[test]
    fn direction_clone() {
        let dir = Direction::Forward;
        let cloned = dir.clone();
        assert_eq!(dir, cloned);
    }

    #[test]
    fn direction_copy() {
        let dir = Direction::Reverse;
        let copied = dir;
        assert_eq!(dir, copied);
    }

    #[test]
    fn direction_debug() {
        assert_eq!(format!("{:?}", Direction::Forward), "Forward");
        assert_eq!(format!("{:?}", Direction::Reverse), "Reverse");
        assert_eq!(format!("{:?}", Direction::Stopped), "Stopped");
    }

    #[test]
    fn direction_equality() {
        assert_eq!(Direction::Forward, Direction::Forward);
        assert_eq!(Direction::Reverse, Direction::Reverse);
        assert_eq!(Direction::Stopped, Direction::Stopped);
        assert_ne!(Direction::Forward, Direction::Reverse);
        assert_ne!(Direction::Forward, Direction::Stopped);
        assert_ne!(Direction::Reverse, Direction::Stopped);
    }

    #[test]
    fn direction_from_text_full_names() {
        assert_eq!(Direction::from_text("forward"), Some(Direction::Forward));
        assert_eq!(Direction::from_text("reverse"), Some(Direction::Reverse));
        assert_eq!(Direction::from_text("stopped"), Some(Direction::Stopped));
    }

    #[test]
    fn direction_from_text_abbreviations() {
        assert_eq!(Direction::from_text("fwd"), Some(Direction::Forward));
        assert_eq!(Direction::from_text("rev"), Some(Direction::Reverse));
        assert_eq!(Direction::from_text("stop"), Some(Direction::Stopped));
    }

    #[test]
    fn direction_from_text_numeric() {
        assert_eq!(Direction::from_text("1"), Some(Direction::Forward));
        assert_eq!(Direction::from_text("-1"), Some(Direction::Reverse));
        assert_eq!(Direction::from_text("0"), Some(Direction::Stopped));
    }

    #[test]
    fn direction_from_text_case_insensitive() {
        assert_eq!(Direction::from_text("FORWARD"), Some(Direction::Forward));
        assert_eq!(Direction::from_text("Forward"), Some(Direction::Forward));
        assert_eq!(Direction::from_text("FWD"), Some(Direction::Forward));
        assert_eq!(Direction::from_text("REVERSE"), Some(Direction::Reverse));
        assert_eq!(Direction::from_text("STOPPED"), Some(Direction::Stopped));
    }

    #[test]
    fn direction_from_text_whitespace() {
        assert_eq!(Direction::from_text("  forward  "), Some(Direction::Forward));
        assert_eq!(Direction::from_text("\tfwd\n"), Some(Direction::Forward));
        assert_eq!(Direction::from_text(" 1 "), Some(Direction::Forward));
    }

    #[test]
    fn direction_from_text_invalid() {
        assert_eq!(Direction::from_text(""), None);
        assert_eq!(Direction::from_text("invalid"), None);
        assert_eq!(Direction::from_text("f"), None);
        assert_eq!(Direction::from_text("2"), None);
        assert_eq!(Direction::from_text("forwards"), None);
    }

    // =========================================================================
    // FaultKind Tests
    // =========================================================================

    #[test]
    fn fault_kind_clone() {
        let fault = FaultKind::ShortCircuit;
        let cloned = fault.clone();
        assert_eq!(fault, cloned);
    }

    #[test]
    fn fault_kind_copy() {
        let fault = FaultKind::Overcurrent;
        let copied = fault;
        assert_eq!(fault, copied);
    }

    #[test]
    fn fault_kind_debug() {
        assert_eq!(format!("{:?}", FaultKind::ShortCircuit), "ShortCircuit");
        assert_eq!(format!("{:?}", FaultKind::Overcurrent), "Overcurrent");
    }

    #[test]
    fn fault_kind_equality() {
        assert_eq!(FaultKind::ShortCircuit, FaultKind::ShortCircuit);
        assert_eq!(FaultKind::Overcurrent, FaultKind::Overcurrent);
        assert_ne!(FaultKind::ShortCircuit, FaultKind::Overcurrent);
    }

    // =========================================================================
    // MotorController Default Methods Tests
    // =========================================================================

    struct TestMotor {
        speed: f32,
        direction: Direction,
        set_speed_called: bool,
        set_direction_called: bool,
    }

    impl TestMotor {
        fn new() -> Self {
            Self {
                speed: 0.0,
                direction: Direction::Stopped,
                set_speed_called: false,
                set_direction_called: false,
            }
        }
    }

    impl MotorController for TestMotor {
        type Error = ();

        fn set_speed(&mut self, speed: f32) -> Result<(), ()> {
            self.speed = speed;
            self.set_speed_called = true;
            Ok(())
        }

        fn set_direction(&mut self, dir: Direction) -> Result<(), ()> {
            self.direction = dir;
            self.set_direction_called = true;
            Ok(())
        }

        fn read_current_ma(&self) -> Result<Option<u32>, ()> {
            Ok(None)
        }
    }

    #[test]
    fn motor_controller_stop_default_impl() {
        let mut motor = TestMotor::new();
        motor.set_speed(0.5).unwrap();
        motor.set_direction(Direction::Forward).unwrap();

        // Reset flags
        motor.set_speed_called = false;
        motor.set_direction_called = false;

        // Test stop() default implementation
        motor.stop().unwrap();

        assert_eq!(motor.speed, 0.0);
        assert_eq!(motor.direction, Direction::Stopped);
        assert!(motor.set_speed_called);
        assert!(motor.set_direction_called);
    }

    // =========================================================================
    // EncoderInput Default Methods Tests
    // =========================================================================

    struct TestEncoder {
        button_state: bool,
    }

    impl TestEncoder {
        fn new() -> Self {
            Self {
                button_state: false,
            }
        }
    }

    impl EncoderInput for TestEncoder {
        fn read_delta(&mut self) -> i32 {
            0
        }

        fn button_pressed(&self) -> bool {
            self.button_state
        }
    }

    #[test]
    fn encoder_input_button_just_pressed_default_impl() {
        let mut encoder = TestEncoder::new();

        // Default implementation should just return button_pressed()
        assert!(!encoder.button_just_pressed());

        encoder.button_state = true;
        assert!(encoder.button_just_pressed());

        // Note: Default impl doesn't track edge - it just returns current state
        assert!(encoder.button_just_pressed()); // Still returns true
    }

    // =========================================================================
    // FaultDetector Default Methods Tests
    // =========================================================================

    struct TestFaultDetector {
        short_circuit: bool,
        overcurrent: bool,
        current_ma: Option<u32>,
    }

    impl TestFaultDetector {
        fn new() -> Self {
            Self {
                short_circuit: false,
                overcurrent: false,
                current_ma: None,
            }
        }
    }

    impl FaultDetector for TestFaultDetector {
        fn is_short_circuit(&self) -> bool {
            self.short_circuit
        }

        fn is_overcurrent(&self) -> bool {
            self.overcurrent
        }

        fn fault_current_ma(&self) -> Option<u32> {
            self.current_ma
        }
    }

    #[test]
    fn fault_detector_active_fault_none() {
        let detector = TestFaultDetector::new();
        assert_eq!(detector.active_fault(), None);
    }

    #[test]
    fn fault_detector_active_fault_short_circuit() {
        let mut detector = TestFaultDetector::new();
        detector.short_circuit = true;
        assert_eq!(detector.active_fault(), Some(FaultKind::ShortCircuit));
    }

    #[test]
    fn fault_detector_active_fault_overcurrent() {
        let mut detector = TestFaultDetector::new();
        detector.overcurrent = true;
        assert_eq!(detector.active_fault(), Some(FaultKind::Overcurrent));
    }

    #[test]
    fn fault_detector_active_fault_priority() {
        // Short circuit should take priority over overcurrent
        let mut detector = TestFaultDetector::new();
        detector.short_circuit = true;
        detector.overcurrent = true;
        assert_eq!(detector.active_fault(), Some(FaultKind::ShortCircuit));
    }
}
