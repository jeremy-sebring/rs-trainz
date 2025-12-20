//! # rs-trainz
//!
//! A DC model train throttle controller with support for physical controls,
//! web UI, and MQTT integration.
//!
//! ## Features
//!
//! - **Hardware abstraction**: Traits for motor control, encoder input, and fault detection
//! - **Multiple control sources**: Physical knob, web API, MQTT with configurable priority
//! - **Smooth transitions**: Configurable execution strategies (immediate, linear, ease-in-out, momentum)
//! - **Transition locks**: Protect important transitions from interruption
//! - **Command priority**: E-stop always wins, physical controls take precedence over remote
//!
//! ## Architecture
//!
//! The crate is structured to allow testing on desktop without hardware:
//!
//! - `traits` - Hardware and network abstractions
//! - `commands` - Command types with priority system
//! - `transition` - Smooth speed transition management
//! - `throttle` - Main controller that ties everything together
//! - `hal` - Concrete implementations (mock for testing, esp32 for hardware)
//!
//! ## Example
//!
//! ```rust
//! use rs_trainz::{
//!     ThrottleController, ThrottleCommand, CommandSource, PrioritizedCommand,
//!     hal::MockMotor,
//!     traits::{EaseInOut, Immediate},
//! };
//!
//! // Create controller with mock motor
//! let motor = MockMotor::new();
//! let mut controller = ThrottleController::new(motor);
//!
//! // Apply an immediate speed command
//! let cmd = ThrottleCommand::speed_immediate(0.5);
//! controller.apply_command(cmd.into(), CommandSource::Physical, 0).unwrap();
//!
//! // Or use a smooth transition
//! let cmd = ThrottleCommand::SetSpeed {
//!     target: 0.8,
//!     strategy: EaseInOut::departure(2000), // 2 second smooth start, locked
//! };
//! controller.apply_command(cmd.into(), CommandSource::Mqtt, 0).unwrap();
//!
//! // Update in your main loop
//! controller.update(20).unwrap(); // 20ms tick
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]

extern crate alloc;

/// Command types and priority system for throttle control.
pub mod commands;
/// Hardware abstraction layer with mock implementations for testing.
pub mod hal;
/// Command queue and processor with source-based lockouts.
pub mod priority;
/// Type-erased execution strategies for runtime polymorphism.
pub mod strategy_dyn;
/// Main throttle controller that coordinates commands, transitions, and hardware.
pub mod throttle;
/// Core traits for hardware abstraction and execution strategies.
pub mod traits;
/// Transition management with lock enforcement and progress tracking.
pub mod transition;

/// Shared configuration system for desktop and ESP32.
pub mod config;

/// JSON parsing helpers for HTTP/MQTT API.
pub mod parsing;

/// Shared message types for HTTP/MQTT communication (serde-based).
#[cfg(feature = "serde")]
pub mod messages;

/// Network services for HTTP API and MQTT (feature-gated).
#[cfg(any(feature = "web", feature = "mqtt"))]
pub mod services;

// Re-exports for convenience
pub use commands::{
    CommandOutcome, CommandSource, CommandType, PrioritizedCommand, RejectReason, ThrottleCommand,
    ThrottleCommandDyn, TransitionResult,
};
pub use priority::{CommandProcessor, CommandQueue, LockoutStatus, SourceLockout};
pub use strategy_dyn::{AnyStrategy, ExecutionStrategyDyn};
pub use throttle::{ThrottleController, ThrottleState};
pub use traits::{
    // Hardware
    Clock,
    Delay,
    Direction,
    // Strategies
    EaseInOut,
    EncoderInput,
    ExecutionStrategy,
    FaultDetector,
    FaultKind,
    // Network
    HttpMethod,
    HttpRequest,
    HttpResponse,
    HttpServer,
    Immediate,
    InterruptBehavior,
    Linear,
    Momentum,
    MotorController,
    MqttClient,
    MqttMessage,
    TransitionLock,
};
pub use transition::{LockStatus, TransitionManager, TransitionProgress};

// Config re-exports
pub use config::{Config, DeviceConfig, MqttConfig, ThrottleConfig, WebConfig, WifiConfig};

// Message re-exports (for HTTP/MQTT APIs)
#[cfg(feature = "serde")]
pub use messages::{SetDirectionRequest, SetMaxSpeedRequest, SetSpeedRequest};

// Parsing function re-exports (serde-json-core based)
#[cfg(feature = "serde-json-core")]
pub use messages::{parse_direction_request, parse_max_speed_request, parse_speed_request};
