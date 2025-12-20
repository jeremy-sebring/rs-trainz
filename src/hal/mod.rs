//! Hardware Abstraction Layer implementations.
//!
//! This module contains concrete implementations of the traits
//! defined in [`crate::traits`] for various platforms.
//!
//! # Available Implementations
//!
//! - `mock`: Test implementations for desktop development
//! - `esp32`: ESP32-C3 SuperMini with BTS7960 motor driver (requires `esp32` feature)

pub mod mock;

#[cfg(feature = "esp32")]
pub mod esp32;

pub use mock::*;

#[cfg(feature = "esp32")]
pub use esp32::*;
