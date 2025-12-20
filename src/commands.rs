//! Command types and priority system for the throttle controller.
//!
//! This module defines the command infrastructure for controlling the throttle,
//! including command types, sources, priorities, and outcomes.
//!
//! # Command Flow
//!
//! Commands in rs-trainz flow through a priority system before reaching the motor:
//!
//! 1. Commands arrive from different [`CommandSource`]s (MQTT, WebAPI, Physical, etc.)
//! 2. Each command has a [`CommandType`] that affects secondary priority ordering
//! 3. E-stop commands from any source are automatically promoted to [`CommandSource::Emergency`]
//! 4. Commands can be wrapped as [`PrioritizedCommand`] for queue-based processing
//!
//! # Typed vs Dynamic Commands
//!
//! The module provides two command representations:
//!
//! - [`ThrottleCommand<S>`]: Compile-time typed with a specific [`ExecutionStrategy`]
//! - [`ThrottleCommandDyn`]: Runtime polymorphic via type erasure for queue storage
//!
//! Use typed commands when you know the strategy at compile time:
//!
//! ```rust
//! use rs_trainz::{ThrottleCommand, traits::EaseInOut};
//!
//! let cmd = ThrottleCommand::SetSpeed {
//!     target: 0.8,
//!     strategy: EaseInOut::departure(2000),
//! };
//! ```
//!
//! Convert to dynamic for queuing with mixed strategy types:
//!
//! ```rust
//! use rs_trainz::{ThrottleCommand, ThrottleCommandDyn, traits::Linear};
//!
//! let typed = ThrottleCommand::SetSpeed {
//!     target: 0.5,
//!     strategy: Linear::new(1000),
//! };
//! let dynamic: ThrottleCommandDyn = typed.into();
//! ```
//!
//! # Command Outcomes
//!
//! When a command is applied, it returns a [`CommandOutcome`] indicating what happened:
//!
//! - [`CommandOutcome::Applied`]: Command was applied immediately
//! - [`CommandOutcome::SpeedTransition`]: For speed commands, contains a [`TransitionResult`]
//!
//! [`ExecutionStrategy`]: crate::traits::ExecutionStrategy

extern crate alloc;

use crate::strategy_dyn::AnyStrategy;
use crate::traits::{Direction, ExecutionStrategy, Immediate};

// ============================================================================
// Command Source Priority
// ============================================================================

/// Source of a command, ordered by priority (lower = lower priority).
///
/// Priority ordering determines which commands can interrupt or override others:
/// - Lower priority sources cannot interrupt higher priority sources during lockout
/// - E-stop commands from any source are automatically promoted to [`Emergency`](Self::Emergency)
/// - Physical controls take precedence over remote commands
///
/// # Priority Order (lowest to highest)
///
/// 1. [`Mqtt`](Self::Mqtt) - Remote MQTT commands
/// 2. [`WebApi`](Self::WebApi) - REST API commands
/// 3. [`WebLocal`](Self::WebLocal) - Local network web UI
/// 4. [`Physical`](Self::Physical) - Rotary encoder, buttons
/// 5. [`Fault`](Self::Fault) - System-detected faults
/// 6. [`Emergency`](Self::Emergency) - E-stop from any source
///
/// # Example
///
/// ```rust
/// use rs_trainz::CommandSource;
///
/// // Physical controls have higher priority than MQTT
/// assert!(CommandSource::Physical > CommandSource::Mqtt);
///
/// // Emergency always wins
/// assert!(CommandSource::Emergency > CommandSource::Physical);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum CommandSource {
    /// Remote MQTT command (lowest priority).
    ///
    /// Used for home automation integration (e.g., Home Assistant, Node-RED).
    Mqtt = 0,

    /// Web API command via REST endpoints.
    ///
    /// Used for programmatic control from external applications.
    WebApi = 1,

    /// Web UI on local network.
    ///
    /// Used for browser-based control interface. Slightly higher priority
    /// than API since it typically indicates active user interaction.
    WebLocal = 2,

    /// Physical controls (rotary encoder, buttons).
    ///
    /// Highest priority for normal operation. When active, creates a lockout
    /// that prevents lower-priority sources from overriding.
    Physical = 3,

    /// System-detected fault (overcurrent, short circuit).
    ///
    /// Used by fault detection systems to trigger automatic stops.
    Fault = 4,

    /// Emergency stop from any source (highest priority).
    ///
    /// E-stop commands from any [`CommandSource`] are automatically promoted
    /// to this level to ensure they always take effect.
    Emergency = 5,
}

/// Type of command, used for secondary priority ordering.
///
/// When two commands have the same [`CommandSource`], the command type
/// determines which takes precedence. Emergency stops always win.
///
/// # Priority Order (lowest to highest)
///
/// 1. [`SetMaxSpeed`](Self::SetMaxSpeed) - Configuration commands
/// 2. [`SetDirection`](Self::SetDirection) - Direction changes
/// 3. [`SetSpeed`](Self::SetSpeed) - Speed control
/// 4. [`EmergencyStop`](Self::EmergencyStop) - Always highest priority
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum CommandType {
    /// Set maximum speed limit (configuration).
    SetMaxSpeed = 0,
    /// Set direction of travel (forward/reverse/stopped).
    SetDirection = 1,
    /// Set speed with optional transition strategy.
    SetSpeed = 2,
    /// Emergency stop - immediately halts the motor.
    EmergencyStop = 3,
}

// ============================================================================
// Typed Commands (compile-time strategy)
// ============================================================================

/// A throttle command with a specific execution strategy.
///
/// This is the compile-time typed command variant. The strategy type `S`
/// determines how speed transitions are executed (immediate, linear, eased, etc.).
///
/// # Type Parameter
///
/// - `S`: The [`ExecutionStrategy`] for speed transitions. Defaults to [`Immediate`].
///
/// # Converting to Dynamic
///
/// Use `.into()` to convert to [`ThrottleCommandDyn`] for storage in mixed-strategy queues:
///
/// ```rust
/// use rs_trainz::{ThrottleCommand, ThrottleCommandDyn, traits::Linear};
///
/// let typed = ThrottleCommand::SetSpeed {
///     target: 0.5,
///     strategy: Linear::new(1000),
/// };
/// let dynamic: ThrottleCommandDyn = typed.into();
/// ```
///
/// [`ExecutionStrategy`]: crate::traits::ExecutionStrategy
#[derive(Clone, Debug)]
pub enum ThrottleCommand<S: ExecutionStrategy = Immediate> {
    /// Set the target speed with a transition strategy.
    ///
    /// The `strategy` determines how the speed change is applied over time.
    SetSpeed {
        /// Target speed value (0.0 to 1.0, clamped to max_speed).
        target: f32,
        /// Strategy for transitioning to the target speed.
        strategy: S,
    },

    /// Set the direction of travel.
    ///
    /// Direction changes are applied immediately without transitions.
    SetDirection(Direction),

    /// Emergency stop - immediately halts the motor.
    ///
    /// This command:
    /// - Sets speed to 0 immediately
    /// - Sets direction to [`Direction::Stopped`]
    /// - Cancels any in-progress transitions
    /// - Is promoted to [`CommandSource::Emergency`] priority
    EmergencyStop,

    /// Set the maximum allowed speed.
    ///
    /// Speed commands will be clamped to this value. Does not affect
    /// currently running transitions.
    SetMaxSpeed(f32),
}

impl ThrottleCommand<Immediate> {
    /// Create an immediate speed change
    pub fn speed_immediate(target: f32) -> Self {
        Self::SetSpeed {
            target,
            strategy: Immediate,
        }
    }

    /// Create an emergency stop command
    pub fn estop() -> Self {
        Self::EmergencyStop
    }
}

impl<S: ExecutionStrategy> ThrottleCommand<S> {
    /// Get the command type for priority ordering
    pub fn command_type(&self) -> CommandType {
        match self {
            Self::SetSpeed { .. } => CommandType::SetSpeed,
            Self::SetDirection(_) => CommandType::SetDirection,
            Self::EmergencyStop => CommandType::EmergencyStop,
            Self::SetMaxSpeed(_) => CommandType::SetMaxSpeed,
        }
    }
}

// ============================================================================
// Dynamic Commands (runtime strategy)
// ============================================================================

/// A command with type-erased execution strategy (for queuing mixed commands).
///
/// This is the runtime polymorphic command variant, using [`AnyStrategy`] for
/// type erasure. It allows storing commands with different strategy types in
/// the same collection.
///
/// # When to Use
///
/// Use `ThrottleCommandDyn` when you need to:
/// - Store commands in a queue or collection
/// - Mix commands with different strategy types
/// - Accept commands from external sources (API, MQTT)
///
/// # Creating Dynamic Commands
///
/// Convert from typed commands:
///
/// ```rust
/// use rs_trainz::{ThrottleCommand, ThrottleCommandDyn, traits::EaseInOut};
///
/// let typed = ThrottleCommand::SetSpeed {
///     target: 0.8,
///     strategy: EaseInOut::departure(2000),
/// };
/// let dynamic: ThrottleCommandDyn = typed.into();
/// ```
///
/// Or construct directly:
///
/// ```rust
/// use rs_trainz::{ThrottleCommandDyn, AnyStrategy, traits::Immediate};
///
/// let cmd = ThrottleCommandDyn::SetSpeed {
///     target: 0.5,
///     strategy: AnyStrategy::new(Immediate),
/// };
/// ```
#[derive(Clone, Debug)]
pub enum ThrottleCommandDyn {
    /// Set the target speed with a type-erased transition strategy.
    SetSpeed {
        /// Target speed value (0.0 to 1.0, clamped to max_speed).
        target: f32,
        /// Type-erased strategy for transitioning to the target speed.
        strategy: AnyStrategy,
    },

    /// Set the direction of travel.
    SetDirection(Direction),

    /// Emergency stop - immediately halts the motor.
    ///
    /// Use [`is_estop()`](Self::is_estop) to check if a command is an e-stop.
    EmergencyStop,

    /// Set the maximum allowed speed.
    SetMaxSpeed(f32),
}

impl ThrottleCommandDyn {
    /// Returns the command type for priority ordering.
    pub fn command_type(&self) -> CommandType {
        match self {
            Self::SetSpeed { .. } => CommandType::SetSpeed,
            Self::SetDirection(_) => CommandType::SetDirection,
            Self::EmergencyStop => CommandType::EmergencyStop,
            Self::SetMaxSpeed(_) => CommandType::SetMaxSpeed,
        }
    }

    /// Returns true if this is an emergency stop command.
    pub fn is_estop(&self) -> bool {
        matches!(self, Self::EmergencyStop)
    }
}

impl<S: ExecutionStrategy + Send + Sync + 'static> From<ThrottleCommand<S>> for ThrottleCommandDyn {
    fn from(cmd: ThrottleCommand<S>) -> Self {
        match cmd {
            ThrottleCommand::SetSpeed { target, strategy } => ThrottleCommandDyn::SetSpeed {
                target,
                strategy: AnyStrategy::new(strategy),
            },
            ThrottleCommand::SetDirection(d) => ThrottleCommandDyn::SetDirection(d),
            ThrottleCommand::EmergencyStop => ThrottleCommandDyn::EmergencyStop,
            ThrottleCommand::SetMaxSpeed(s) => ThrottleCommandDyn::SetMaxSpeed(s),
        }
    }
}

// ============================================================================
// Prioritized Command Wrapper
// ============================================================================

/// A command with source and timestamp for priority ordering.
///
/// Wraps a [`ThrottleCommandDyn`] with metadata for queue-based processing.
/// Commands are ordered by their effective priority, which is a tuple of
/// (source, command_type).
///
/// # Priority Calculation
///
/// E-stop commands are automatically promoted to [`CommandSource::Emergency`]:
///
/// ```rust
/// use rs_trainz::{PrioritizedCommand, ThrottleCommandDyn, CommandSource};
///
/// // E-stop from MQTT gets promoted to Emergency
/// let estop = PrioritizedCommand::new(
///     ThrottleCommandDyn::EmergencyStop,
///     CommandSource::Mqtt,
///     0,
/// );
/// assert_eq!(estop.priority().0, CommandSource::Emergency);
/// ```
///
/// # Ordering
///
/// Commands implement `Ord` for use with [`BinaryHeap`](std::collections::BinaryHeap):
///
/// ```rust
/// use rs_trainz::{PrioritizedCommand, ThrottleCommandDyn, CommandSource, AnyStrategy};
/// use rs_trainz::traits::Immediate;
///
/// let mqtt = PrioritizedCommand::new(
///     ThrottleCommandDyn::SetSpeed { target: 0.5, strategy: AnyStrategy::new(Immediate) },
///     CommandSource::Mqtt,
///     0,
/// );
/// let physical = PrioritizedCommand::new(
///     ThrottleCommandDyn::SetSpeed { target: 0.5, strategy: AnyStrategy::new(Immediate) },
///     CommandSource::Physical,
///     0,
/// );
///
/// // Physical has higher priority
/// assert!(physical > mqtt);
/// ```
#[derive(Clone, Debug)]
pub struct PrioritizedCommand {
    /// The actual command to execute.
    pub command: ThrottleCommandDyn,
    /// Source that issued this command.
    pub source: CommandSource,
    /// Timestamp when the command was issued (milliseconds since start).
    pub timestamp_ms: u64,
}

impl PrioritizedCommand {
    /// Creates a new prioritized command with the given source and timestamp.
    pub fn new(command: ThrottleCommandDyn, source: CommandSource, timestamp_ms: u64) -> Self {
        Self {
            command,
            source,
            timestamp_ms,
        }
    }

    /// Get effective priority (source, command_type)
    /// E-stop from any source gets promoted to Emergency level
    pub fn priority(&self) -> (CommandSource, CommandType) {
        let cmd_type = self.command.command_type();

        let effective_source = if self.command.is_estop() {
            CommandSource::Emergency
        } else {
            self.source
        };

        (effective_source, cmd_type)
    }
}

impl Eq for PrioritizedCommand {}

impl PartialEq for PrioritizedCommand {
    fn eq(&self, other: &Self) -> bool {
        self.priority() == other.priority() && self.timestamp_ms == other.timestamp_ms
    }
}

impl Ord for PrioritizedCommand {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority().cmp(&other.priority())
    }
}

impl PartialOrd for PrioritizedCommand {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// ============================================================================
// Command Outcomes
// ============================================================================

/// Result of attempting to apply a command.
///
/// Returned by [`ThrottleController::apply_command`] to indicate what
/// happened when a command was processed.
///
/// [`ThrottleController::apply_command`]: crate::ThrottleController::apply_command
#[derive(Clone, Debug)]
pub enum CommandOutcome {
    /// Command was applied immediately (direction, max_speed).
    Applied,

    /// Speed transition result with details about what happened.
    ///
    /// Contains a [`TransitionResult`] with specific outcome information.
    SpeedTransition(TransitionResult),
}

/// Result of attempting to start a speed transition.
///
/// Indicates whether a speed change command succeeded, was queued,
/// was rejected, or interrupted an existing transition.
///
/// # Examples
///
/// Handling transition results:
///
/// ```rust
/// use rs_trainz::{TransitionResult, RejectReason};
///
/// fn handle_result(result: TransitionResult) {
///     match result {
///         TransitionResult::Started => println!("Transition started"),
///         TransitionResult::Queued => println!("Command queued for later"),
///         TransitionResult::Rejected { reason } => {
///             println!("Rejected: {:?}", reason);
///         }
///         TransitionResult::Interrupted { previous_target } => {
///             println!("Interrupted transition to {}", previous_target);
///         }
///     }
/// }
/// ```
#[derive(Clone, Debug)]
pub enum TransitionResult {
    /// Transition started successfully.
    ///
    /// The speed change is now in progress.
    Started,

    /// Command was queued for later execution.
    ///
    /// This happens when a locked transition is in progress and its
    /// [`InterruptBehavior`](crate::traits::InterruptBehavior) is `Queue`.
    Queued,

    /// Command was rejected.
    ///
    /// See [`RejectReason`] for why the command couldn't be executed.
    Rejected {
        /// Why the command was rejected.
        reason: RejectReason,
    },

    /// Interrupted an existing transition.
    ///
    /// The previous transition was cancelled and the new one started.
    Interrupted {
        /// The target speed of the interrupted transition.
        previous_target: f32,
    },
}

/// Reason a command was rejected.
///
/// When a [`TransitionResult::Rejected`] is returned, this enum explains
/// why the command couldn't be executed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum RejectReason {
    /// A locked transition is in progress.
    ///
    /// The current transition has [`TransitionLock::Hard`] and cannot be
    /// interrupted except by e-stop.
    ///
    /// [`TransitionLock::Hard`]: crate::traits::TransitionLock::Hard
    TransitionLocked,

    /// Command source has lower priority than active source.
    ///
    /// A source lockout is active from a higher-priority source, or the
    /// current transition has [`TransitionLock::Source`] and the new
    /// command is from a lower-priority source.
    ///
    /// [`TransitionLock::Source`]: crate::traits::TransitionLock::Source
    LowerPriority,

    /// Command queue is full.
    ///
    /// The command processor's queue is at capacity and the new command
    /// doesn't have high enough priority to displace existing commands.
    QueueFull,
}

/// Type alias for priority tuple (source, command_type).
///
/// Used for ordering commands in the priority queue. Higher values
/// have higher priority (Emergency > Fault > Physical > etc.).
pub type Priority = (CommandSource, CommandType);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::Linear;

    // === CommandSource Tests ===
    #[test]
    fn command_source_ordering() {
        // Verify priority order: Mqtt < WebApi < WebLocal < Physical < Fault < Emergency
        assert!(CommandSource::Mqtt < CommandSource::WebApi);
        assert!(CommandSource::WebApi < CommandSource::WebLocal);
        assert!(CommandSource::WebLocal < CommandSource::Physical);
        assert!(CommandSource::Physical < CommandSource::Fault);
        assert!(CommandSource::Fault < CommandSource::Emergency);
    }

    #[test]
    fn command_source_equality() {
        assert_eq!(CommandSource::Physical, CommandSource::Physical);
        assert_ne!(CommandSource::Mqtt, CommandSource::WebApi);
    }

    #[test]
    fn command_source_debug() {
        let s = format!("{:?}", CommandSource::Physical);
        assert_eq!(s, "Physical");
    }

    // === CommandType Tests ===
    #[test]
    fn command_type_ordering() {
        assert!(CommandType::SetMaxSpeed < CommandType::SetDirection);
        assert!(CommandType::SetDirection < CommandType::SetSpeed);
        assert!(CommandType::SetSpeed < CommandType::EmergencyStop);
    }

    // === ThrottleCommand Tests ===
    #[test]
    fn throttle_command_speed_immediate() {
        let cmd = ThrottleCommand::speed_immediate(0.5);
        assert!(
            matches!(cmd, ThrottleCommand::SetSpeed { target, .. } if (target - 0.5).abs() < 0.001)
        );
        assert_eq!(cmd.command_type(), CommandType::SetSpeed);
    }

    #[test]
    fn throttle_command_estop() {
        let cmd = ThrottleCommand::estop();
        assert!(matches!(cmd, ThrottleCommand::EmergencyStop));
        assert_eq!(cmd.command_type(), CommandType::EmergencyStop);
    }

    #[test]
    fn throttle_command_set_direction() {
        let cmd: ThrottleCommand = ThrottleCommand::SetDirection(Direction::Forward);
        assert_eq!(cmd.command_type(), CommandType::SetDirection);
    }

    #[test]
    fn throttle_command_set_max_speed() {
        let cmd: ThrottleCommand = ThrottleCommand::SetMaxSpeed(0.8);
        assert_eq!(cmd.command_type(), CommandType::SetMaxSpeed);
    }

    #[test]
    fn throttle_command_with_strategy() {
        let cmd = ThrottleCommand::SetSpeed {
            target: 0.75,
            strategy: Linear::new(1000),
        };
        assert_eq!(cmd.command_type(), CommandType::SetSpeed);
    }

    // === ThrottleCommandDyn Tests ===
    #[test]
    fn throttle_command_dyn_from_immediate() {
        let cmd = ThrottleCommand::speed_immediate(0.5);
        let dyn_cmd: ThrottleCommandDyn = cmd.into();

        assert!(
            matches!(dyn_cmd, ThrottleCommandDyn::SetSpeed { target, .. } if (target - 0.5).abs() < 0.001)
        );
        assert_eq!(dyn_cmd.command_type(), CommandType::SetSpeed);
    }

    #[test]
    fn throttle_command_dyn_from_linear() {
        let cmd = ThrottleCommand::SetSpeed {
            target: 0.75,
            strategy: Linear::new(1000),
        };
        let dyn_cmd: ThrottleCommandDyn = cmd.into();

        assert!(
            matches!(dyn_cmd, ThrottleCommandDyn::SetSpeed { target, .. } if (target - 0.75).abs() < 0.001)
        );
    }

    #[test]
    fn throttle_command_dyn_from_direction() {
        let cmd: ThrottleCommand = ThrottleCommand::SetDirection(Direction::Reverse);
        let dyn_cmd: ThrottleCommandDyn = cmd.into();

        assert!(matches!(
            dyn_cmd,
            ThrottleCommandDyn::SetDirection(Direction::Reverse)
        ));
    }

    #[test]
    fn throttle_command_dyn_from_estop() {
        let cmd = ThrottleCommand::estop();
        let dyn_cmd: ThrottleCommandDyn = cmd.into();

        assert!(matches!(dyn_cmd, ThrottleCommandDyn::EmergencyStop));
        assert!(dyn_cmd.is_estop());
    }

    #[test]
    fn throttle_command_dyn_from_max_speed() {
        let cmd: ThrottleCommand = ThrottleCommand::SetMaxSpeed(0.6);
        let dyn_cmd: ThrottleCommandDyn = cmd.into();

        assert!(matches!(dyn_cmd, ThrottleCommandDyn::SetMaxSpeed(s) if (s - 0.6).abs() < 0.001));
    }

    #[test]
    fn throttle_command_dyn_is_estop() {
        let estop = ThrottleCommandDyn::EmergencyStop;
        let speed = ThrottleCommandDyn::SetSpeed {
            target: 0.5,
            strategy: AnyStrategy::new(Immediate),
        };

        assert!(estop.is_estop());
        assert!(!speed.is_estop());
    }

    // === PrioritizedCommand Tests ===
    #[test]
    fn prioritized_command_new() {
        let cmd = ThrottleCommandDyn::EmergencyStop;
        let pc = PrioritizedCommand::new(cmd, CommandSource::Physical, 12345);

        assert_eq!(pc.source, CommandSource::Physical);
        assert_eq!(pc.timestamp_ms, 12345);
    }

    #[test]
    fn prioritized_command_priority_normal() {
        let cmd = ThrottleCommandDyn::SetSpeed {
            target: 0.5,
            strategy: AnyStrategy::new(Immediate),
        };
        let pc = PrioritizedCommand::new(cmd, CommandSource::WebApi, 0);

        let (source, cmd_type) = pc.priority();
        assert_eq!(source, CommandSource::WebApi);
        assert_eq!(cmd_type, CommandType::SetSpeed);
    }

    #[test]
    fn prioritized_command_estop_promotes_to_emergency() {
        let cmd = ThrottleCommandDyn::EmergencyStop;
        let pc = PrioritizedCommand::new(cmd, CommandSource::Mqtt, 0);

        let (source, cmd_type) = pc.priority();
        // E-stop from Mqtt should be promoted to Emergency
        assert_eq!(source, CommandSource::Emergency);
        assert_eq!(cmd_type, CommandType::EmergencyStop);
    }

    #[test]
    fn prioritized_command_ordering_by_source() {
        let mqtt_cmd = PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.5,
                strategy: AnyStrategy::new(Immediate),
            },
            CommandSource::Mqtt,
            0,
        );
        let physical_cmd = PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.5,
                strategy: AnyStrategy::new(Immediate),
            },
            CommandSource::Physical,
            0,
        );

        // Physical > Mqtt
        assert!(physical_cmd > mqtt_cmd);
        assert!(mqtt_cmd < physical_cmd);
    }

    #[test]
    fn prioritized_command_ordering_by_type() {
        let speed_cmd = PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.5,
                strategy: AnyStrategy::new(Immediate),
            },
            CommandSource::Physical,
            0,
        );
        let estop_cmd = PrioritizedCommand::new(
            ThrottleCommandDyn::EmergencyStop,
            CommandSource::Physical, // Same source
            0,
        );

        // E-stop type > SetSpeed type, and e-stop promotes source to Emergency
        assert!(estop_cmd > speed_cmd);
    }

    #[test]
    fn prioritized_command_equality() {
        let cmd1 = PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.5,
                strategy: AnyStrategy::new(Immediate),
            },
            CommandSource::Physical,
            100,
        );
        let cmd2 = PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.8, // Different target
                strategy: AnyStrategy::new(Immediate),
            },
            CommandSource::Physical, // Same source
            100,                     // Same timestamp
        );

        // Same priority and timestamp means equal
        assert_eq!(cmd1, cmd2);
    }

    #[test]
    fn prioritized_command_inequality_different_timestamp() {
        let cmd1 = PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.5,
                strategy: AnyStrategy::new(Immediate),
            },
            CommandSource::Physical,
            100,
        );
        let cmd2 = PrioritizedCommand::new(
            ThrottleCommandDyn::SetSpeed {
                target: 0.5,
                strategy: AnyStrategy::new(Immediate),
            },
            CommandSource::Physical,
            200, // Different timestamp
        );

        assert_ne!(cmd1, cmd2);
    }

    // === RejectReason Tests ===
    #[test]
    fn reject_reason_equality() {
        assert_eq!(
            RejectReason::TransitionLocked,
            RejectReason::TransitionLocked
        );
        assert_ne!(RejectReason::TransitionLocked, RejectReason::QueueFull);
    }

    #[test]
    fn reject_reason_debug() {
        let s = format!("{:?}", RejectReason::LowerPriority);
        assert_eq!(s, "LowerPriority");
    }

    // === TransitionResult Tests ===
    #[test]
    fn transition_result_variants() {
        let started = TransitionResult::Started;
        let queued = TransitionResult::Queued;
        let rejected = TransitionResult::Rejected {
            reason: RejectReason::TransitionLocked,
        };
        let interrupted = TransitionResult::Interrupted {
            previous_target: 0.5,
        };

        assert!(matches!(started, TransitionResult::Started));
        assert!(matches!(queued, TransitionResult::Queued));
        assert!(matches!(
            rejected,
            TransitionResult::Rejected {
                reason: RejectReason::TransitionLocked
            }
        ));
        assert!(matches!(
            interrupted,
            TransitionResult::Interrupted { previous_target } if (previous_target - 0.5).abs() < 0.001
        ));
    }

    // === CommandOutcome Tests ===
    #[test]
    fn command_outcome_applied() {
        let outcome = CommandOutcome::Applied;
        assert!(matches!(outcome, CommandOutcome::Applied));
    }

    #[test]
    fn command_outcome_speed_transition() {
        let outcome = CommandOutcome::SpeedTransition(TransitionResult::Started);
        assert!(matches!(
            outcome,
            CommandOutcome::SpeedTransition(TransitionResult::Started)
        ));
    }
}
