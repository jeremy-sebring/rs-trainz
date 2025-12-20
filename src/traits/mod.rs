//! Trait definitions for hardware abstraction, networking, and execution strategies.
//!
//! This module defines the core abstractions that allow rs-trainz to:
//! - Run on different hardware (ESP32, desktop mock)
//! - Use different network implementations
//! - Support various speed transition behaviors
//!
//! # Submodules
//!
//! - `hardware`: Motor control, encoder input, fault detection, clock
//! - `network`: MQTT client and HTTP server traits
//! - `strategy`: Execution strategies for speed transitions
//! - `display`: Display rendering trait
//!
//! # Hardware Abstraction
//!
//! The key hardware traits are:
//!
//! - [`MotorController`]: PWM-based DC motor control
//! - [`EncoderInput`]: Rotary encoder for physical control
//! - [`FaultDetector`]: Short circuit and overcurrent detection
//! - [`Clock`]: Time source for `no_std` environments
//!
//! # Execution Strategies
//!
//! Speed transitions use the [`ExecutionStrategy`] trait with built-in implementations:
//!
//! - [`Immediate`]: Instant change
//! - [`Linear`]: Constant rate over duration
//! - [`EaseInOut`]: Smoothstep curve (good for stations)
//! - [`Momentum`]: Physics-based acceleration

pub mod display;
pub mod hardware;
pub mod network;
pub mod strategy;

pub use display::*;
pub use hardware::*;
pub use network::*;
pub use strategy::*;
