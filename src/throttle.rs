//! Main throttle controller that ties everything together.
//!
//! This module provides [`ThrottleController`], the central component that
//! coordinates commands, transitions, and motor control.
//!
//! # Overview
//!
//! The throttle controller:
//! - Accepts commands from various sources (physical, web, MQTT)
//! - Manages smooth speed transitions
//! - Handles faults and emergency stops
//! - Provides state snapshots for UI/API
//!
//! # Example
//!
//! ```rust
//! use rs_trainz::{
//!     ThrottleController, ThrottleCommand, CommandSource,
//!     hal::MockMotor,
//!     traits::{EaseInOut, MotorController},
//! };
//!
//! // Create controller with a mock motor
//! let motor = MockMotor::new();
//! let mut controller = ThrottleController::new(motor);
//!
//! // Apply a speed command with smooth transition
//! let cmd = ThrottleCommand::SetSpeed {
//!     target: 0.8,
//!     strategy: EaseInOut::departure(2000), // 2 second departure
//! };
//! controller.apply_command(cmd.into(), CommandSource::Physical, 0).unwrap();
//!
//! // Main loop - call update() every tick (e.g., 20ms)
//! for tick in 0..100 {
//!     controller.update(tick * 20).unwrap();
//! }
//!
//! // Get current state for UI
//! let state = controller.state(2000);
//! println!("Speed: {:.1}%, Direction: {:?}", state.speed * 100.0, state.direction);
//! ```
//!
//! # Fault Handling
//!
//! The controller can handle hardware faults:
//!
//! ```rust
//! use rs_trainz::{ThrottleController, hal::MockMotor, FaultKind};
//!
//! let motor = MockMotor::new();
//! let mut controller = ThrottleController::new(motor);
//!
//! // Handle a detected fault
//! controller.handle_fault(FaultKind::Overcurrent).unwrap();
//! assert!(controller.has_fault());
//!
//! // Clear after fault is resolved
//! controller.clear_fault();
//! assert!(!controller.has_fault());
//! ```

use crate::commands::{CommandOutcome, CommandSource, ThrottleCommandDyn};
use crate::strategy_dyn::AnyStrategy;
use crate::traits::{Direction, FaultKind, Immediate, MotorController};
use crate::transition::{LockStatus, TransitionManager, TransitionProgress};

/// Main throttle controller.
///
/// Coordinates commands, transitions, and motor control. This is the
/// primary interface for controlling the train throttle.
///
/// # Type Parameter
///
/// - `M`: The motor controller implementation ([`MotorController`] trait)
///
/// # Thread Safety
///
/// The controller itself is not thread-safe. For multi-threaded scenarios
/// (e.g., web server + main loop), wrap in `Arc<Mutex<ThrottleController>>`
/// or use the `SharedThrottleState` wrapper from the services module
/// (requires `web` or `mqtt` feature).
pub struct ThrottleController<M: MotorController> {
    motor: M,
    speed_transition: TransitionManager,
    direction: Direction,
    max_speed: f32,
    fault: Option<FaultKind>,
}

impl<M: MotorController> ThrottleController<M> {
    /// Create a new throttle controller
    pub fn new(motor: M) -> Self {
        Self {
            motor,
            speed_transition: TransitionManager::new(0.0),
            direction: Direction::Stopped,
            max_speed: 1.0,
            fault: None,
        }
    }

    /// Apply a command to the throttle
    pub fn apply_command(
        &mut self,
        cmd: ThrottleCommandDyn,
        source: CommandSource,
        now_ms: u64,
    ) -> Result<CommandOutcome, M::Error> {
        let outcome = match cmd {
            ThrottleCommandDyn::SetSpeed { target, strategy } => {
                let clamped = target.clamp(0.0, self.max_speed);
                let result = self.speed_transition.try_start(
                    clamped, strategy, source, false, // not e-stop
                    now_ms,
                );
                CommandOutcome::SpeedTransition(result)
            }

            ThrottleCommandDyn::EmergencyStop => {
                let result = self.speed_transition.try_start(
                    0.0,
                    AnyStrategy::new(Immediate),
                    source,
                    true, // is e-stop
                    now_ms,
                );
                self.direction = Direction::Stopped;
                self.motor.set_direction(Direction::Stopped)?;
                self.motor.set_speed(0.0)?;
                CommandOutcome::SpeedTransition(result)
            }

            ThrottleCommandDyn::SetDirection(dir) => {
                self.direction = dir;
                self.motor.set_direction(dir)?;
                CommandOutcome::Applied
            }

            ThrottleCommandDyn::SetMaxSpeed(max) => {
                self.max_speed = max.clamp(0.0, 1.0);
                // If current target exceeds new max, adjust
                if let Some(target) = self.speed_transition.target() {
                    if target > self.max_speed {
                        // Could trigger a new transition to max_speed here
                    }
                }
                CommandOutcome::Applied
            }
        };

        Ok(outcome)
    }

    /// Update the controller - call every tick (e.g., 20ms)
    pub fn update(&mut self, now_ms: u64) -> Result<(), M::Error> {
        let (speed, _complete) = self.speed_transition.update(now_ms);
        self.motor.set_speed(speed)?;
        Ok(())
    }

    /// Handle a detected fault
    pub fn handle_fault(&mut self, fault: FaultKind) -> Result<(), M::Error> {
        self.fault = Some(fault);
        self.speed_transition.cancel_and_set(0.0);
        self.motor.set_speed(0.0)?;
        Ok(())
    }

    /// Clear a fault condition
    pub fn clear_fault(&mut self) {
        self.fault = None;
    }

    /// Get the current state for UI/API
    pub fn state(&self, now_ms: u64) -> ThrottleState {
        ThrottleState {
            speed: self.speed_transition.current(),
            target_speed: self.speed_transition.target(),
            direction: self.direction,
            max_speed: self.max_speed,
            fault: self.fault,
            lock_status: self.speed_transition.lock_status(),
            transition_progress: self.speed_transition.progress(now_ms),
        }
    }

    /// Get just the current speed
    pub fn current_speed(&self) -> f32 {
        self.speed_transition.current()
    }

    /// Get the current direction
    pub fn current_direction(&self) -> Direction {
        self.direction
    }

    /// Check if a transition is in progress
    pub fn is_transitioning(&self) -> bool {
        self.speed_transition.is_transitioning()
    }

    /// Check if there's an active fault
    pub fn has_fault(&self) -> bool {
        self.fault.is_some()
    }
}

/// Full state snapshot for UI/API.
///
/// Contains all relevant throttle state for rendering UI or responding
/// to API requests. Implements `serde::Serialize` when the `serde` feature
/// is enabled for easy JSON serialization.
///
/// # Example
///
/// ```rust
/// use rs_trainz::{ThrottleController, hal::MockMotor, Direction};
///
/// let motor = MockMotor::new();
/// let controller = ThrottleController::new(motor);
///
/// let state = controller.state(0);
/// assert_eq!(state.speed, 0.0);
/// assert_eq!(state.direction, Direction::Stopped);
/// assert!(state.fault.is_none());
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ThrottleState {
    /// Current speed (0.0 to 1.0).
    pub speed: f32,
    /// Target speed if a transition is in progress.
    pub target_speed: Option<f32>,
    /// Current direction of travel.
    pub direction: Direction,
    /// Maximum allowed speed (0.0 to 1.0).
    pub max_speed: f32,
    /// Current fault condition, if any.
    pub fault: Option<FaultKind>,
    /// Transition lock status, if a locked transition is active.
    pub lock_status: Option<LockStatus>,
    /// Progress of current transition, if any.
    pub transition_progress: Option<TransitionProgress>,
}

impl Default for ThrottleState {
    fn default() -> Self {
        Self {
            speed: 0.0,
            target_speed: None,
            direction: Direction::Stopped,
            max_speed: 1.0,
            fault: None,
            lock_status: None,
            transition_progress: None,
        }
    }
}
