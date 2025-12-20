//! Transition manager for handling smooth speed changes with locks.
//!
//! This module manages speed transitions with support for locking,
//! queueing, and progress tracking. It's the core of the smooth
//! acceleration/deceleration system.
//!
//! # Transition Locks
//!
//! Transitions can be protected from interruption using [`TransitionLock`]:
//!
//! - [`TransitionLock::None`]: Any command can interrupt
//! - [`TransitionLock::Source`]: Only same or higher priority source can interrupt
//! - [`TransitionLock::Hard`]: Only e-stop can interrupt
//!
//! # Example
//!
//! ```rust
//! use rs_trainz::transition::TransitionManager;
//! use rs_trainz::{AnyStrategy, CommandSource};
//! use rs_trainz::traits::Linear;
//!
//! let mut manager = TransitionManager::new(0.0);
//!
//! // Start a 1-second linear transition
//! let result = manager.try_start(
//!     1.0,                            // target
//!     AnyStrategy::new(Linear::new(1000)),
//!     CommandSource::Physical,
//!     false,                          // not e-stop
//!     0,                              // current time
//! );
//!
//! // Update in your main loop
//! let (current_speed, is_complete) = manager.update(500); // at 500ms
//! assert!((current_speed - 0.5).abs() < 0.1); // ~50% complete
//! ```
//!
//! # Queueing
//!
//! When a locked transition with [`InterruptBehavior::Queue`] is interrupted,
//! the new command is queued and executes after the current transition completes.
//!
//! [`TransitionLock`]: crate::traits::TransitionLock
//! [`TransitionLock::None`]: crate::traits::TransitionLock::None
//! [`TransitionLock::Source`]: crate::traits::TransitionLock::Source
//! [`TransitionLock::Hard`]: crate::traits::TransitionLock::Hard
//! [`InterruptBehavior::Queue`]: crate::traits::InterruptBehavior::Queue

use crate::commands::{CommandSource, RejectReason, TransitionResult};
use crate::strategy_dyn::AnyStrategy;
use crate::traits::{InterruptBehavior, TransitionLock};

/// An active speed transition.
///
/// Contains all state for an in-progress speed change, including the
/// interpolation strategy and lock settings.
pub struct ActiveTransition {
    /// Starting speed value (0.0 to 1.0).
    pub from: f32,
    /// Target speed value (0.0 to 1.0).
    pub to: f32,
    /// Strategy for interpolating between values.
    pub strategy: AnyStrategy,
    /// Timestamp when the transition started (milliseconds since start).
    pub started_ms: u64,
    /// Source that initiated this transition.
    pub source: CommandSource,
    /// Lock level for this transition.
    pub lock: TransitionLock,
    /// Behavior when something tries to interrupt.
    pub interrupt_behavior: InterruptBehavior,
}

/// A queued transition waiting to execute
struct QueuedTransition {
    to: f32,
    strategy: AnyStrategy,
    source: CommandSource,
}

/// Manages speed transitions with locking and queuing.
///
/// The transition manager handles:
/// - Smooth speed interpolation over time
/// - Transition locking to prevent interruption
/// - Command queueing for sequential execution
/// - Progress tracking for UI feedback
///
/// # Usage
///
/// Create with an initial speed, start transitions, and call `update()` each tick:
///
/// ```rust
/// use rs_trainz::transition::TransitionManager;
/// use rs_trainz::{AnyStrategy, CommandSource};
/// use rs_trainz::traits::EaseInOut;
///
/// let mut manager = TransitionManager::new(0.0);
///
/// // Start a departure transition (locked)
/// manager.try_start(
///     0.8,
///     AnyStrategy::new(EaseInOut::departure(2000)),
///     CommandSource::Physical,
///     false,
///     0,
/// );
///
/// // Main loop
/// loop {
///     let now_ms = 100; // get current time
///     let (speed, complete) = manager.update(now_ms);
///     // Apply speed to motor...
///     if complete {
///         break;
///     }
///     # break; // for doctest
/// }
/// ```
pub struct TransitionManager {
    active: Option<ActiveTransition>,
    queued: Option<QueuedTransition>,
    current_value: f32,
}

impl TransitionManager {
    /// Create a new transition manager with an initial value
    pub fn new(initial: f32) -> Self {
        Self {
            active: None,
            queued: None,
            current_value: initial,
        }
    }

    /// Attempt to start a new transition
    ///
    /// Returns the result indicating whether the transition was started,
    /// queued, rejected, or interrupted an existing transition.
    #[must_use]
    pub fn try_start(
        &mut self,
        to: f32,
        strategy: AnyStrategy,
        source: CommandSource,
        is_estop: bool,
        now_ms: u64,
    ) -> TransitionResult {
        // E-stop always wins immediately
        if is_estop {
            let previous = self.active.as_ref().map(|t| t.to);
            self.active = None;
            self.queued = None;
            self.current_value = to;
            return match previous {
                Some(prev) => TransitionResult::Interrupted {
                    previous_target: prev,
                },
                None => TransitionResult::Started,
            };
        }

        // Check if we can interrupt the current transition
        if let Some(ref active) = self.active {
            match active.lock {
                TransitionLock::Hard => {
                    // Only e-stop can interrupt (handled above)
                    return self.handle_blocked_command(
                        to,
                        strategy,
                        source,
                        active.interrupt_behavior,
                    );
                }

                TransitionLock::Source => {
                    // Same or higher priority source can interrupt
                    if source < active.source {
                        return self.handle_blocked_command(
                            to,
                            strategy,
                            source,
                            active.interrupt_behavior,
                        );
                    }
                    // Fall through to start new transition
                }

                TransitionLock::None => {
                    // Always interruptible
                }
            }
        }

        // Start the new transition
        let previous = self.active.as_ref().map(|t| t.to);

        let lock = strategy.lock();
        let interrupt_behavior = strategy.on_interrupt();

        self.active = Some(ActiveTransition {
            from: self.current_value,
            to,
            strategy,
            started_ms: now_ms,
            source,
            lock,
            interrupt_behavior,
        });

        match previous {
            Some(prev) => TransitionResult::Interrupted {
                previous_target: prev,
            },
            None => TransitionResult::Started,
        }
    }

    /// Handle a command that can't interrupt the current transition
    fn handle_blocked_command(
        &mut self,
        to: f32,
        strategy: AnyStrategy,
        source: CommandSource,
        interrupt_behavior: InterruptBehavior,
    ) -> TransitionResult {
        match interrupt_behavior {
            InterruptBehavior::Queue => {
                if self.queued.is_some() {
                    TransitionResult::Rejected {
                        reason: RejectReason::QueueFull,
                    }
                } else {
                    self.queued = Some(QueuedTransition {
                        to,
                        strategy,
                        source,
                    });
                    TransitionResult::Queued
                }
            }
            InterruptBehavior::Reject => TransitionResult::Rejected {
                reason: RejectReason::TransitionLocked,
            },
            InterruptBehavior::Replace => {
                // Contradictory: locked but replace behavior
                // Treat as rejection for safety
                TransitionResult::Rejected {
                    reason: RejectReason::LowerPriority,
                }
            }
        }
    }

    /// Update the transition state - call every tick
    ///
    /// Returns (current_value, is_complete)
    pub fn update(&mut self, now_ms: u64) -> (f32, bool) {
        match &self.active {
            None => {
                // Check for queued transition
                if let Some(queued) = self.queued.take() {
                    let lock = queued.strategy.lock();
                    let interrupt_behavior = queued.strategy.on_interrupt();

                    self.active = Some(ActiveTransition {
                        from: self.current_value,
                        to: queued.to,
                        strategy: queued.strategy,
                        started_ms: now_ms,
                        source: queued.source,
                        lock,
                        interrupt_behavior,
                    });
                    // Recurse to process the new transition
                    return self.update(now_ms);
                }
                (self.current_value, true)
            }
            Some(transition) => {
                let elapsed = now_ms.saturating_sub(transition.started_ms);
                let (value, complete) =
                    transition
                        .strategy
                        .interpolate(transition.from, transition.to, elapsed);

                self.current_value = value;

                if complete {
                    self.active = None;
                    // Queued transition will be picked up next update
                }

                (value, complete)
            }
        }
    }

    /// Cancel all transitions and set a specific value
    pub fn cancel_and_set(&mut self, value: f32) {
        self.active = None;
        self.queued = None;
        self.current_value = value;
    }

    /// Cancel all pending transitions
    pub fn cancel_all(&mut self) {
        self.active = None;
        self.queued = None;
    }

    /// Get the current value
    pub fn current(&self) -> f32 {
        self.current_value
    }

    /// Check if a transition is in progress
    pub fn is_transitioning(&self) -> bool {
        self.active.is_some()
    }

    /// Get the target value if a transition is active
    pub fn target(&self) -> Option<f32> {
        self.active.as_ref().map(|t| t.to)
    }

    /// Get the current lock status
    pub fn lock_status(&self) -> Option<LockStatus> {
        self.active.as_ref().map(|t| LockStatus {
            lock: t.lock,
            source: t.source,
            target: t.to,
            has_queued: self.queued.is_some(),
        })
    }

    /// Get progress information for UI feedback
    pub fn progress(&self, now_ms: u64) -> Option<TransitionProgress> {
        self.active.as_ref().map(|t| {
            let elapsed = now_ms.saturating_sub(t.started_ms);
            TransitionProgress {
                from: t.from,
                to: t.to,
                current: self.current_value,
                elapsed_ms: elapsed,
                estimated_total_ms: t.strategy.duration_ms(),
            }
        })
    }
}

/// Current lock status.
///
/// Returned by [`TransitionManager::lock_status`] when a transition is active.
/// Useful for UI feedback showing lock state and queued commands.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LockStatus {
    /// Lock level of the active transition.
    pub lock: TransitionLock,
    /// Source that owns the lock.
    pub source: CommandSource,
    /// Target speed of the locked transition (0.0 to 1.0).
    pub target: f32,
    /// Whether there is a queued command waiting to execute.
    pub has_queued: bool,
}

/// Progress information for a transition.
///
/// Returned by [`TransitionManager::progress`] for UI feedback such as
/// progress bars or ETA displays.
///
/// # Example
///
/// ```rust
/// use rs_trainz::transition::TransitionProgress;
///
/// let progress = TransitionProgress {
///     from: 0.0,
///     to: 1.0,
///     current: 0.5,
///     elapsed_ms: 500,
///     estimated_total_ms: Some(1000),
/// };
///
/// // Get percentage (0.0 to 1.0)
/// assert_eq!(progress.percent(), Some(0.5));
/// ```
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionProgress {
    /// Starting speed value (0.0 to 1.0).
    pub from: f32,
    /// Target speed value (0.0 to 1.0).
    pub to: f32,
    /// Current speed value (0.0 to 1.0).
    pub current: f32,
    /// Time elapsed since transition started (milliseconds).
    pub elapsed_ms: u64,
    /// Estimated total duration (milliseconds), if known.
    ///
    /// `None` for strategies like [`Momentum`] where duration depends
    /// on the distance to travel.
    ///
    /// [`Momentum`]: crate::traits::Momentum
    pub estimated_total_ms: Option<u64>,
}

impl TransitionProgress {
    /// Get progress as a percentage (0.0 - 1.0)
    pub fn percent(&self) -> Option<f32> {
        self.estimated_total_ms.map(|total| {
            if total == 0 {
                1.0
            } else {
                (self.elapsed_ms as f32 / total as f32).min(1.0)
            }
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{EaseInOut, Immediate, Linear};

    fn immediate() -> AnyStrategy {
        AnyStrategy::new(Immediate)
    }

    fn linear(ms: u64) -> AnyStrategy {
        AnyStrategy::new(Linear::new(ms))
    }

    fn linear_locked(ms: u64) -> AnyStrategy {
        AnyStrategy::new(Linear::locked(ms))
    }

    fn linear_source_locked(ms: u64) -> AnyStrategy {
        AnyStrategy::new(Linear::source_locked(ms))
    }

    fn arrival(ms: u64) -> AnyStrategy {
        AnyStrategy::new(EaseInOut::arrival(ms))
    }

    // === Basic Operations ===
    #[test]
    fn new_starts_at_initial_value() {
        let tm = TransitionManager::new(0.5);
        assert!((tm.current() - 0.5).abs() < 0.001);
        assert!(!tm.is_transitioning());
        assert!(tm.target().is_none());
    }

    #[test]
    fn start_immediate_transition() {
        let mut tm = TransitionManager::new(0.0);
        let result = tm.try_start(1.0, immediate(), CommandSource::Physical, false, 0);

        assert!(matches!(result, TransitionResult::Started));
        assert!(tm.is_transitioning());

        // Update should complete immediately
        let (val, complete) = tm.update(0);
        assert!((val - 1.0).abs() < 0.001);
        assert!(complete);
        assert!(!tm.is_transitioning());
    }

    #[test]
    fn start_linear_transition() {
        let mut tm = TransitionManager::new(0.0);
        let result = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);

        assert!(matches!(result, TransitionResult::Started));
        assert!(tm.is_transitioning());
        assert!((tm.target().unwrap() - 1.0).abs() < 0.001);
    }

    #[test]
    fn linear_transition_interpolates_correctly() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);

        // At t=0
        let (val, complete) = tm.update(0);
        assert!((val - 0.0).abs() < 0.01);
        assert!(!complete);

        // At t=500
        let (val, complete) = tm.update(500);
        assert!((val - 0.5).abs() < 0.01);
        assert!(!complete);

        // At t=1000
        let (val, complete) = tm.update(1000);
        assert!((val - 1.0).abs() < 0.01);
        assert!(complete);
        assert!(!tm.is_transitioning());
    }

    // === E-stop Handling ===
    #[test]
    fn estop_interrupts_any_transition() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear_locked(5000), CommandSource::Physical, false, 0);

        // E-stop should interrupt even hard-locked transitions
        let result = tm.try_start(0.0, immediate(), CommandSource::Mqtt, true, 100);

        assert!(
            matches!(result, TransitionResult::Interrupted { previous_target } if (previous_target - 1.0).abs() < 0.001)
        );
        assert!(!tm.is_transitioning());
        assert!((tm.current() - 0.0).abs() < 0.001);
    }

    #[test]
    fn estop_clears_queued() {
        let mut tm = TransitionManager::new(0.5);
        let _ = tm.try_start(0.0, arrival(1000), CommandSource::Physical, false, 0);

        // Queue a command
        let result = tm.try_start(0.8, linear(500), CommandSource::Mqtt, false, 100);
        assert!(matches!(result, TransitionResult::Queued));

        // E-stop should clear both active and queued
        let _ = tm.try_start(0.0, immediate(), CommandSource::Mqtt, true, 200);

        // Complete and verify no queued transition executes
        let _ = tm.update(200);
        let _ = tm.update(201);
        assert!(!tm.is_transitioning());
        assert!((tm.current() - 0.0).abs() < 0.001);
    }

    #[test]
    fn estop_when_no_active_transition() {
        let mut tm = TransitionManager::new(0.5);
        let result = tm.try_start(0.0, immediate(), CommandSource::Mqtt, true, 0);

        // Should just start (not interrupted)
        assert!(matches!(result, TransitionResult::Started));
        assert!((tm.current() - 0.0).abs() < 0.001);
    }

    // === Lock Types ===
    #[test]
    fn no_lock_allows_interruption() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);

        // Lower priority can interrupt no-lock transition
        let result = tm.try_start(0.5, immediate(), CommandSource::Mqtt, false, 100);
        assert!(matches!(result, TransitionResult::Interrupted { .. }));
    }

    #[test]
    fn source_lock_allows_same_or_higher_priority() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(
            1.0,
            linear_source_locked(1000),
            CommandSource::WebApi,
            false,
            0,
        );

        // Higher priority can interrupt
        let result = tm.try_start(0.5, immediate(), CommandSource::Physical, false, 100);
        assert!(matches!(result, TransitionResult::Interrupted { .. }));
    }

    #[test]
    fn source_lock_blocks_lower_priority() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(
            1.0,
            linear_source_locked(1000),
            CommandSource::Physical,
            false,
            0,
        );

        // Lower priority is blocked (Replace becomes LowerPriority rejection)
        let result = tm.try_start(0.5, immediate(), CommandSource::Mqtt, false, 100);
        assert!(matches!(
            result,
            TransitionResult::Rejected {
                reason: RejectReason::LowerPriority
            }
        ));
    }

    #[test]
    fn hard_lock_blocks_all_except_estop() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear_locked(1000), CommandSource::Mqtt, false, 0);

        // Even higher priority is blocked
        let result = tm.try_start(0.5, immediate(), CommandSource::Emergency, false, 100);
        assert!(matches!(
            result,
            TransitionResult::Rejected {
                reason: RejectReason::TransitionLocked
            }
        ));
    }

    #[test]
    fn hard_lock_allows_estop() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear_locked(1000), CommandSource::Mqtt, false, 0);

        // E-stop still works
        let result = tm.try_start(0.0, immediate(), CommandSource::Mqtt, true, 100);
        assert!(matches!(result, TransitionResult::Interrupted { .. }));
    }

    // === Queuing Behavior ===
    #[test]
    fn queue_behavior_queues_command() {
        let mut tm = TransitionManager::new(0.5);
        // arrival() has Source lock + Queue interrupt behavior
        let _ = tm.try_start(0.0, arrival(1000), CommandSource::Physical, false, 0);

        // Lower priority command should be queued
        let result = tm.try_start(0.8, linear(500), CommandSource::Mqtt, false, 100);
        assert!(matches!(result, TransitionResult::Queued));
    }

    #[test]
    fn queue_full_rejects() {
        let mut tm = TransitionManager::new(0.5);
        let _ = tm.try_start(0.0, arrival(1000), CommandSource::Physical, false, 0);

        // First queue succeeds
        let result = tm.try_start(0.8, linear(500), CommandSource::Mqtt, false, 100);
        assert!(matches!(result, TransitionResult::Queued));

        // Second queue fails
        let result = tm.try_start(0.9, linear(500), CommandSource::Mqtt, false, 200);
        assert!(matches!(
            result,
            TransitionResult::Rejected {
                reason: RejectReason::QueueFull
            }
        ));
    }

    #[test]
    fn queued_starts_after_active_completes() {
        let mut tm = TransitionManager::new(0.5);
        let _ = tm.try_start(0.0, arrival(1000), CommandSource::Physical, false, 0);
        let _ = tm.try_start(0.8, linear(500), CommandSource::Mqtt, false, 100);

        // Complete the first transition
        let _ = tm.update(1000);
        assert!(!tm.is_transitioning()); // First complete

        // Next update should start queued
        let _ = tm.update(1001);
        assert!(tm.is_transitioning());
        assert!((tm.target().unwrap() - 0.8).abs() < 0.001);
    }

    // === Cancel Operations ===
    #[test]
    fn cancel_and_set() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);
        let _ = tm.try_start(0.8, linear(500), CommandSource::Physical, false, 100);

        tm.cancel_and_set(0.25);

        assert!(!tm.is_transitioning());
        assert!((tm.current() - 0.25).abs() < 0.001);

        // Queued should also be cleared
        let _ = tm.update(0);
        assert!(!tm.is_transitioning());
    }

    #[test]
    fn cancel_all() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);

        // Update partway
        let _ = tm.update(500);
        let mid_value = tm.current();

        tm.cancel_all();

        assert!(!tm.is_transitioning());
        // Value should stay where it was
        assert!((tm.current() - mid_value).abs() < 0.001);
    }

    // === Status and Progress ===
    #[test]
    fn lock_status_when_active() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear_locked(1000), CommandSource::Physical, false, 0);

        let status = tm.lock_status().unwrap();
        assert_eq!(status.lock, TransitionLock::Hard);
        assert_eq!(status.source, CommandSource::Physical);
        assert!((status.target - 1.0).abs() < 0.001);
        assert!(!status.has_queued);
    }

    #[test]
    fn lock_status_shows_queued() {
        let mut tm = TransitionManager::new(0.5);
        let _ = tm.try_start(0.0, arrival(1000), CommandSource::Physical, false, 0);
        let _ = tm.try_start(0.8, linear(500), CommandSource::Mqtt, false, 100);

        let status = tm.lock_status().unwrap();
        assert!(status.has_queued);
    }

    #[test]
    fn lock_status_none_when_no_transition() {
        let tm = TransitionManager::new(0.0);
        assert!(tm.lock_status().is_none());
    }

    #[test]
    fn progress_info() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);

        // Update to 500ms
        let _ = tm.update(500);

        let progress = tm.progress(500).unwrap();
        assert!((progress.from - 0.0).abs() < 0.001);
        assert!((progress.to - 1.0).abs() < 0.001);
        assert!((progress.current - 0.5).abs() < 0.01);
        assert_eq!(progress.elapsed_ms, 500);
        assert_eq!(progress.estimated_total_ms, Some(1000));
    }

    #[test]
    fn progress_percent() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);
        let _ = tm.update(250);

        let progress = tm.progress(250).unwrap();
        let pct = progress.percent().unwrap();
        assert!((pct - 0.25).abs() < 0.01);
    }

    #[test]
    fn progress_percent_zero_duration() {
        let progress = TransitionProgress {
            from: 0.0,
            to: 1.0,
            current: 1.0,
            elapsed_ms: 0,
            estimated_total_ms: Some(0),
        };
        assert!((progress.percent().unwrap() - 1.0).abs() < 0.001);
    }

    #[test]
    fn progress_percent_none_for_unknown_duration() {
        let progress = TransitionProgress {
            from: 0.0,
            to: 1.0,
            current: 0.5,
            elapsed_ms: 100,
            estimated_total_ms: None,
        };
        assert!(progress.percent().is_none());
    }

    #[test]
    fn progress_none_when_no_transition() {
        let tm = TransitionManager::new(0.0);
        assert!(tm.progress(0).is_none());
    }

    // === Edge Cases ===
    #[test]
    fn update_with_no_active_transition() {
        let mut tm = TransitionManager::new(0.5);
        let (val, complete) = tm.update(100);
        assert!((val - 0.5).abs() < 0.001);
        assert!(complete);
    }

    #[test]
    fn transition_continues_from_current_value() {
        let mut tm = TransitionManager::new(0.0);

        // First transition
        let _ = tm.try_start(0.5, immediate(), CommandSource::Physical, false, 0);
        let _ = tm.update(0);

        // Second transition should start from 0.5
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 100);

        let progress = tm.progress(100).unwrap();
        assert!((progress.from - 0.5).abs() < 0.001);
    }

    #[test]
    fn interrupt_updates_from_value() {
        let mut tm = TransitionManager::new(0.0);

        // Start long transition
        let _ = tm.try_start(1.0, linear(1000), CommandSource::Physical, false, 0);
        let _ = tm.update(500); // At 0.5

        // Interrupt with new transition
        let _ = tm.try_start(0.0, linear(500), CommandSource::Physical, false, 500);

        let progress = tm.progress(500).unwrap();
        assert!((progress.from - 0.5).abs() < 0.01);
        assert!((progress.to - 0.0).abs() < 0.001);
    }

    #[test]
    fn same_source_can_interrupt_source_locked() {
        let mut tm = TransitionManager::new(0.0);
        let _ = tm.try_start(
            1.0,
            linear_source_locked(1000),
            CommandSource::Physical,
            false,
            0,
        );

        // Same source can interrupt source-locked transition
        let result = tm.try_start(0.5, immediate(), CommandSource::Physical, false, 100);
        assert!(matches!(result, TransitionResult::Interrupted { .. }));
    }
}
