//! ESP32 clock implementation using the ESP-IDF timer.

use crate::traits::Clock;

/// ESP32 clock using the hardware timer.
///
/// Provides millisecond-resolution timing using the ESP-IDF `esp_timer_get_time()`
/// function, which returns microseconds since boot.
///
/// # Example
///
/// ```ignore
/// use rs_trainz::hal::esp32::Esp32Clock;
/// use rs_trainz::traits::Clock;
///
/// let clock = Esp32Clock::new();
/// let start = clock.now_ms();
/// // ... do work ...
/// let elapsed = clock.now_ms() - start;
/// ```
pub struct Esp32Clock;

impl Esp32Clock {
    /// Creates a new ESP32 clock instance.
    #[inline]
    pub fn new() -> Self {
        Self
    }
}

impl Default for Esp32Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for Esp32Clock {
    #[inline]
    fn now_ms(&self) -> u64 {
        // esp_timer_get_time returns microseconds since boot
        // Safe: this is a simple read of the hardware timer, no side effects
        let micros = unsafe { esp_idf_hal::sys::esp_timer_get_time() };
        (micros / 1000) as u64
    }
}
