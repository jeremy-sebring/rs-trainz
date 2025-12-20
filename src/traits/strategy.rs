//! Execution strategy traits for controlling how speed transitions happen.
//!
//! This module defines the [`ExecutionStrategy`] trait and provides four
//! built-in implementations for different transition behaviors.
//!
//! # Built-in Strategies
//!
//! | Strategy | Use Case | Lock Support |
//! |----------|----------|--------------|
//! | [`Immediate`] | Instant changes, e-stop | No |
//! | [`Linear`] | Simple speed ramps | Yes |
//! | [`EaseInOut`] | Station arrivals/departures | Yes |
//! | [`Momentum`] | Realistic physics feel | Yes |
//!
//! # Transition Locks
//!
//! Strategies can specify a [`TransitionLock`] to protect against interruption:
//!
//! - `None`: Any command can interrupt
//! - `Source`: Only same or higher priority source can interrupt
//! - `Hard`: Only e-stop can interrupt
//!
//! # Interrupt Behavior
//!
//! When something tries to interrupt, [`InterruptBehavior`] determines what happens:
//!
//! - `Replace`: New command replaces current (default)
//! - `Queue`: New command waits for current to finish
//! - `Reject`: New command is rejected
//!
//! # Examples
//!
//! ## Station Departure (Protected)
//!
//! ```rust
//! use rs_trainz::traits::EaseInOut;
//!
//! // Hard-locked departure - only e-stop can interrupt
//! let departure = EaseInOut::departure(3000); // 3 second acceleration
//! ```
//!
//! ## Station Arrival (Queued)
//!
//! ```rust
//! use rs_trainz::traits::EaseInOut;
//!
//! // Source-locked arrival - queues follow-up commands
//! let arrival = EaseInOut::arrival(2000); // 2 second deceleration
//! ```
//!
//! ## Responsive Physical Control
//!
//! ```rust
//! use rs_trainz::traits::Momentum;
//!
//! // Feels like a real throttle
//! let momentum = Momentum::responsive();
//! ```

/// How a transition is protected from interruption.
///
/// Used by [`ExecutionStrategy::lock`] to specify protection level.
/// Higher lock levels provide stronger guarantees that a transition
/// will complete uninterrupted.
///
/// # Example
///
/// ```rust
/// use rs_trainz::traits::{TransitionLock, EaseInOut};
/// use rs_trainz::ExecutionStrategy;
///
/// // Departure is hard-locked
/// assert_eq!(EaseInOut::departure(1000).lock(), TransitionLock::Hard);
///
/// // Arrival is source-locked
/// assert_eq!(EaseInOut::arrival(1000).lock(), TransitionLock::Source);
///
/// // Basic ease-in-out has no lock
/// assert_eq!(EaseInOut::new(1000).lock(), TransitionLock::None);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum TransitionLock {
    /// Can be interrupted by any command.
    ///
    /// Default for most strategies. Suitable for responsive control.
    #[default]
    None,

    /// Can be interrupted by same or higher priority source.
    ///
    /// Prevents lower-priority sources from interrupting. Useful for
    /// station arrivals where you want to protect the deceleration but
    /// still allow the operator to override.
    Source,

    /// Can only be interrupted by e-stop.
    ///
    /// Maximum protection. Use for safety-critical transitions like
    /// station departures where interruption could cause jarring motion.
    Hard,
}

/// What happens when something tries to interrupt a locked transition.
///
/// Used by [`ExecutionStrategy::on_interrupt`] to specify behavior when
/// a lower-priority command attempts to interrupt a locked transition.
///
/// # Example
///
/// ```rust
/// use rs_trainz::traits::{InterruptBehavior, EaseInOut};
/// use rs_trainz::ExecutionStrategy;
///
/// // Arrival queues follow-up commands
/// assert_eq!(EaseInOut::arrival(1000).on_interrupt(), InterruptBehavior::Queue);
///
/// // Departure rejects interrupts
/// assert_eq!(EaseInOut::departure(1000).on_interrupt(), InterruptBehavior::Reject);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum InterruptBehavior {
    /// New command replaces current transition.
    ///
    /// Default behavior. The current transition is cancelled and the
    /// new one starts immediately from the current speed.
    #[default]
    Replace,

    /// New command queues after current completes.
    ///
    /// The new command waits and starts automatically when the current
    /// transition finishes. Only one command can be queued at a time.
    Queue,

    /// New command is rejected.
    ///
    /// The command fails with [`RejectReason::TransitionLocked`].
    ///
    /// [`RejectReason::TransitionLocked`]: crate::RejectReason::TransitionLocked
    Reject,
}

/// Describes how a value change should be applied over time.
///
/// Implement this trait to create custom transition behaviors. The trait
/// is used by the transition manager to interpolate speed values.
///
/// # Required Methods
///
/// - [`interpolate`](Self::interpolate): Calculate current value given elapsed time
/// - [`duration_ms`](Self::duration_ms): Return estimated duration (or `None`)
///
/// # Optional Methods
///
/// - [`lock`](Self::lock): Return [`TransitionLock`] level (default: `None`)
/// - [`on_interrupt`](Self::on_interrupt): Return [`InterruptBehavior`] (default: `Replace`)
///
/// # Example Implementation
///
/// ```rust
/// use rs_trainz::traits::{ExecutionStrategy, TransitionLock, InterruptBehavior};
///
/// #[derive(Clone)]
/// struct MyStrategy {
///     duration_ms: u64,
/// }
///
/// impl ExecutionStrategy for MyStrategy {
///     fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool) {
///         if elapsed_ms >= self.duration_ms {
///             (to, true)
///         } else {
///             let t = elapsed_ms as f32 / self.duration_ms as f32;
///             // Custom curve here
///             let value = from + (to - from) * t;
///             (value, false)
///         }
///     }
///
///     fn duration_ms(&self) -> Option<u64> {
///         Some(self.duration_ms)
///     }
/// }
/// ```
pub trait ExecutionStrategy: Clone + Send {
    /// Interpolate the value during a transition
    ///
    /// # Arguments
    /// * `from` - Starting value
    /// * `to` - Target value
    /// * `elapsed_ms` - Time since transition started
    ///
    /// # Returns
    /// Tuple of (current_value, is_complete)
    fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool);

    /// Estimated total duration in milliseconds (if known)
    fn duration_ms(&self) -> Option<u64>;

    /// What lock level does this transition require?
    fn lock(&self) -> TransitionLock {
        TransitionLock::None
    }

    /// What happens if something tries to interrupt?
    fn on_interrupt(&self) -> InterruptBehavior {
        InterruptBehavior::Replace
    }
}

// ============================================================================
// Concrete Strategies
// ============================================================================

/// Instant transition - no interpolation.
///
/// Immediately jumps to the target value. Used for e-stop and when
/// smooth transitions aren't needed.
///
/// # Example
///
/// ```rust
/// use rs_trainz::traits::Immediate;
/// use rs_trainz::ExecutionStrategy;
///
/// let strategy = Immediate;
/// let (value, complete) = strategy.interpolate(0.0, 1.0, 0);
/// assert_eq!(value, 1.0);
/// assert!(complete);
/// ```
#[derive(Clone, Debug, Default)]
pub struct Immediate;

impl ExecutionStrategy for Immediate {
    fn interpolate(&self, _from: f32, to: f32, _elapsed_ms: u64) -> (f32, bool) {
        (to, true)
    }

    fn duration_ms(&self) -> Option<u64> {
        Some(0)
    }
}

/// Linear interpolation over a fixed duration.
///
/// Speed changes at a constant rate from start to finish.
/// Simple and predictable, good for general use.
///
/// # Constructors
///
/// - [`new`](Self::new): No lock, interruptible
/// - [`locked`](Self::locked): Hard lock, rejects interrupts
/// - [`source_locked`](Self::source_locked): Source lock, allows same/higher priority
///
/// # Example
///
/// ```rust
/// use rs_trainz::traits::Linear;
/// use rs_trainz::ExecutionStrategy;
///
/// let strategy = Linear::new(1000); // 1 second
///
/// // At halfway point
/// let (value, complete) = strategy.interpolate(0.0, 1.0, 500);
/// assert!((value - 0.5).abs() < 0.01);
/// assert!(!complete);
///
/// // At completion
/// let (value, complete) = strategy.interpolate(0.0, 1.0, 1000);
/// assert_eq!(value, 1.0);
/// assert!(complete);
/// ```
#[derive(Clone, Debug)]
pub struct Linear {
    /// Total duration of the transition in milliseconds.
    pub duration_ms: u64,
    /// Lock level for this transition.
    pub lock: TransitionLock,
    /// Behavior when interrupted.
    pub interrupt: InterruptBehavior,
}

impl Linear {
    /// Creates a new linear transition with no lock.
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            lock: TransitionLock::None,
            interrupt: InterruptBehavior::Replace,
        }
    }

    /// Creates a hard-locked linear transition that rejects interrupts.
    pub fn locked(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            lock: TransitionLock::Hard,
            interrupt: InterruptBehavior::Reject,
        }
    }

    /// Creates a source-locked linear transition.
    pub fn source_locked(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            lock: TransitionLock::Source,
            interrupt: InterruptBehavior::Replace,
        }
    }
}

impl ExecutionStrategy for Linear {
    fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool) {
        if self.duration_ms == 0 || elapsed_ms >= self.duration_ms {
            return (to, true);
        }

        let t = elapsed_ms as f32 / self.duration_ms as f32;
        let value = from + (to - from) * t;
        (value, false)
    }

    fn duration_ms(&self) -> Option<u64> {
        Some(self.duration_ms)
    }

    fn lock(&self) -> TransitionLock {
        self.lock
    }

    fn on_interrupt(&self) -> InterruptBehavior {
        self.interrupt
    }
}

/// Smooth ease-in-out using smoothstep function.
///
/// Starts slow, accelerates through the middle, and decelerates at the end.
/// Creates natural-looking motion, ideal for station arrivals and departures.
///
/// The smoothstep curve is: `t² × (3 - 2t)`
///
/// # Constructors
///
/// - [`new`](Self::new): No lock, general purpose
/// - [`departure`](Self::departure): Hard lock for station departures
/// - [`arrival`](Self::arrival): Source lock with queueing for arrivals
///
/// # Example
///
/// ```rust
/// use rs_trainz::traits::EaseInOut;
/// use rs_trainz::ExecutionStrategy;
///
/// let strategy = EaseInOut::new(1000);
///
/// // Starts slow (less than linear)
/// let (value, _) = strategy.interpolate(0.0, 1.0, 100);
/// assert!(value < 0.1); // Less than 10% at 10% time
///
/// // Halfway is exactly 50%
/// let (value, _) = strategy.interpolate(0.0, 1.0, 500);
/// assert!((value - 0.5).abs() < 0.01);
///
/// // Ends slow (more than linear)
/// let (value, _) = strategy.interpolate(0.0, 1.0, 900);
/// assert!(value > 0.9); // More than 90% at 90% time
/// ```
#[derive(Clone, Debug)]
pub struct EaseInOut {
    /// Total duration of the transition in milliseconds.
    pub duration_ms: u64,
    /// Lock level for this transition.
    pub lock: TransitionLock,
    /// Behavior when interrupted.
    pub interrupt: InterruptBehavior,
}

impl EaseInOut {
    /// Creates a new ease-in-out transition with no lock.
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            lock: TransitionLock::None,
            interrupt: InterruptBehavior::Replace,
        }
    }

    /// Station departure - locked, can't be interrupted
    pub fn departure(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            lock: TransitionLock::Hard,
            interrupt: InterruptBehavior::Reject,
        }
    }

    /// Station arrival - queues follow-up commands
    pub fn arrival(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            lock: TransitionLock::Source,
            interrupt: InterruptBehavior::Queue,
        }
    }

    fn smoothstep(t: f32) -> f32 {
        t * t * (3.0 - 2.0 * t)
    }
}

impl ExecutionStrategy for EaseInOut {
    fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool) {
        if self.duration_ms == 0 || elapsed_ms >= self.duration_ms {
            return (to, true);
        }

        let t = elapsed_ms as f32 / self.duration_ms as f32;
        let eased_t = Self::smoothstep(t);
        let value = from + (to - from) * eased_t;
        (value, false)
    }

    fn duration_ms(&self) -> Option<u64> {
        Some(self.duration_ms)
    }

    fn lock(&self) -> TransitionLock {
        self.lock
    }

    fn on_interrupt(&self) -> InterruptBehavior {
        self.interrupt
    }
}

/// Momentum-based transition - feels like a real throttle.
///
/// Simulates physical inertia with acceleration and maximum rate limits.
/// Creates a realistic "heavy" feel, as if controlling real machinery.
///
/// Unlike time-based strategies, duration depends on the distance to travel.
/// Short movements complete quickly; large movements take longer.
///
/// # Physics Model
///
/// - Accelerates from rest at the configured rate
/// - Caps velocity at the maximum rate
/// - Continues at max rate until target is reached
///
/// # Constructors
///
/// - [`new`](Self::new): Custom acceleration and max rate
/// - [`gentle`](Self::gentle): Slow, smooth changes
/// - [`responsive`](Self::responsive): Quick, snappy response
///
/// # Example
///
/// ```rust
/// use rs_trainz::traits::Momentum;
/// use rs_trainz::ExecutionStrategy;
///
/// // Gentle momentum for smooth operation
/// let strategy = Momentum::gentle();
///
/// // Duration is unknown (depends on distance)
/// assert!(strategy.duration_ms().is_none());
///
/// // Movement over time
/// let (v1, _) = strategy.interpolate(0.0, 1.0, 100);
/// let (v2, _) = strategy.interpolate(0.0, 1.0, 500);
/// assert!(v2 > v1); // Accelerating
/// ```
#[derive(Clone, Debug)]
pub struct Momentum {
    /// Acceleration rate (units per second per second).
    pub acceleration: f32,
    /// Maximum velocity (units per second).
    pub max_rate: f32,
    /// Lock level for this transition.
    pub lock: TransitionLock,
}

impl Momentum {
    /// Creates a new momentum transition with the given acceleration and max rate.
    pub fn new(acceleration: f32, max_rate: f32) -> Self {
        Self {
            acceleration,
            max_rate,
            lock: TransitionLock::None,
        }
    }

    /// Gentle acceleration for smooth operation
    pub fn gentle() -> Self {
        Self {
            acceleration: 0.5,
            max_rate: 0.3,
            lock: TransitionLock::None,
        }
    }

    /// Responsive acceleration for quick changes
    pub fn responsive() -> Self {
        Self {
            acceleration: 2.0,
            max_rate: 0.8,
            lock: TransitionLock::None,
        }
    }
}

impl ExecutionStrategy for Momentum {
    fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool) {
        let elapsed_s = elapsed_ms as f32 / 1000.0;
        let distance = to - from;
        let direction = if distance >= 0.0 { 1.0 } else { -1.0 };

        // Simplified: accelerate up to max rate
        let rate = (self.acceleration * elapsed_s).min(self.max_rate);
        let moved = rate * elapsed_s * direction;

        // no_std-compatible abs: if x < 0 then -x else x
        let moved_abs = if moved < 0.0 { -moved } else { moved };
        let distance_abs = if distance < 0.0 { -distance } else { distance };

        if moved_abs >= distance_abs {
            (to, true)
        } else {
            (from + moved, false)
        }
    }

    fn duration_ms(&self) -> Option<u64> {
        None // Depends on distance
    }

    fn lock(&self) -> TransitionLock {
        self.lock
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // === Immediate Strategy ===
    #[test]
    fn immediate_returns_target_instantly() {
        let s = Immediate;
        let (val, done) = s.interpolate(0.0, 1.0, 0);
        assert_eq!(val, 1.0);
        assert!(done);
    }

    #[test]
    fn immediate_ignores_elapsed_time() {
        let s = Immediate;
        let (val, done) = s.interpolate(0.0, 1.0, 999999);
        assert_eq!(val, 1.0);
        assert!(done);
    }

    #[test]
    fn immediate_duration_is_zero() {
        assert_eq!(Immediate.duration_ms(), Some(0));
    }

    #[test]
    fn immediate_default_lock_is_none() {
        assert_eq!(Immediate.lock(), TransitionLock::None);
    }

    // === Linear Strategy ===
    #[test]
    fn linear_at_zero_elapsed() {
        let s = Linear::new(1000);
        let (val, done) = s.interpolate(0.0, 1.0, 0);
        assert_eq!(val, 0.0);
        assert!(!done);
    }

    #[test]
    fn linear_at_half_elapsed() {
        let s = Linear::new(1000);
        let (val, done) = s.interpolate(0.0, 1.0, 500);
        assert!((val - 0.5).abs() < 0.001);
        assert!(!done);
    }

    #[test]
    fn linear_at_full_elapsed() {
        let s = Linear::new(1000);
        let (val, done) = s.interpolate(0.0, 1.0, 1000);
        assert_eq!(val, 1.0);
        assert!(done);
    }

    #[test]
    fn linear_beyond_duration() {
        let s = Linear::new(1000);
        let (val, done) = s.interpolate(0.0, 1.0, 2000);
        assert_eq!(val, 1.0);
        assert!(done);
    }

    #[test]
    fn linear_zero_duration_completes_immediately() {
        let s = Linear::new(0);
        let (val, done) = s.interpolate(0.0, 1.0, 0);
        assert_eq!(val, 1.0);
        assert!(done);
    }

    #[test]
    fn linear_reverse_direction() {
        let s = Linear::new(1000);
        let (val, _) = s.interpolate(1.0, 0.0, 500);
        assert!((val - 0.5).abs() < 0.001);
    }

    #[test]
    fn linear_locked_has_hard_lock() {
        let s = Linear::locked(1000);
        assert_eq!(s.lock(), TransitionLock::Hard);
        assert_eq!(s.on_interrupt(), InterruptBehavior::Reject);
    }

    #[test]
    fn linear_source_locked_has_source_lock() {
        let s = Linear::source_locked(1000);
        assert_eq!(s.lock(), TransitionLock::Source);
        assert_eq!(s.on_interrupt(), InterruptBehavior::Replace);
    }

    #[test]
    fn linear_new_has_no_lock() {
        let s = Linear::new(1000);
        assert_eq!(s.lock(), TransitionLock::None);
        assert_eq!(s.on_interrupt(), InterruptBehavior::Replace);
    }

    #[test]
    fn linear_duration_ms_returns_value() {
        let s = Linear::new(1234);
        assert_eq!(s.duration_ms(), Some(1234));
    }

    // === EaseInOut Strategy ===
    #[test]
    fn ease_in_out_starts_slow() {
        let s = EaseInOut::new(1000);
        let (val, _) = s.interpolate(0.0, 1.0, 100); // 10%
                                                     // Smoothstep at 0.1 = 0.1^2 * (3 - 2*0.1) = 0.01 * 2.8 = 0.028
        assert!(val < 0.05); // Should be slower than linear (which would be 0.1)
    }

    #[test]
    fn ease_in_out_midpoint_is_half() {
        let s = EaseInOut::new(1000);
        let (val, _) = s.interpolate(0.0, 1.0, 500);
        // Smoothstep(0.5) = 0.5^2 * (3 - 2*0.5) = 0.25 * 2 = 0.5
        assert!((val - 0.5).abs() < 0.001);
    }

    #[test]
    fn ease_in_out_ends_slow() {
        let s = EaseInOut::new(1000);
        let (val, _) = s.interpolate(0.0, 1.0, 900); // 90%
                                                     // Smoothstep at 0.9 = 0.81 * 1.2 = 0.972
        assert!(val > 0.95);
    }

    #[test]
    fn ease_in_out_zero_duration() {
        let s = EaseInOut::new(0);
        let (val, done) = s.interpolate(0.0, 1.0, 0);
        assert_eq!(val, 1.0);
        assert!(done);
    }

    #[test]
    fn ease_in_out_completes_at_duration() {
        let s = EaseInOut::new(1000);
        let (val, done) = s.interpolate(0.0, 1.0, 1000);
        assert_eq!(val, 1.0);
        assert!(done);
    }

    #[test]
    fn departure_is_hard_locked() {
        let s = EaseInOut::departure(1000);
        assert_eq!(s.lock(), TransitionLock::Hard);
        assert_eq!(s.on_interrupt(), InterruptBehavior::Reject);
    }

    #[test]
    fn arrival_queues_interrupts() {
        let s = EaseInOut::arrival(1000);
        assert_eq!(s.lock(), TransitionLock::Source);
        assert_eq!(s.on_interrupt(), InterruptBehavior::Queue);
    }

    #[test]
    fn ease_in_out_new_has_no_lock() {
        let s = EaseInOut::new(1000);
        assert_eq!(s.lock(), TransitionLock::None);
        assert_eq!(s.on_interrupt(), InterruptBehavior::Replace);
    }

    // === Momentum Strategy ===
    #[test]
    fn momentum_starts_from_zero_velocity() {
        let s = Momentum::new(1.0, 1.0);
        let (val, _) = s.interpolate(0.0, 1.0, 100);
        assert!(val > 0.0);
        assert!(val < 0.05); // Should be small initially due to acceleration
    }

    #[test]
    fn momentum_accelerates_over_time() {
        let s = Momentum::new(1.0, 1.0);
        let (val1, _) = s.interpolate(0.0, 1.0, 100);
        let (val2, _) = s.interpolate(0.0, 1.0, 500);
        // Quadratic relationship due to acceleration
        assert!(val2 > val1 * 3.0);
    }

    #[test]
    fn momentum_respects_max_rate() {
        let s = Momentum::new(100.0, 0.1); // Very high accel, low max rate
        let (val, done) = s.interpolate(0.0, 1.0, 1000);
        // Should be limited by max_rate
        assert!(!done);
        assert!(val < 0.5);
    }

    #[test]
    fn momentum_reverse_direction() {
        let s = Momentum::new(1.0, 1.0);
        let (val, _) = s.interpolate(1.0, 0.0, 500);
        assert!(val < 1.0);
        assert!(val > 0.0);
    }

    #[test]
    fn momentum_gentle_preset() {
        let s = Momentum::gentle();
        assert_eq!(s.acceleration, 0.5);
        assert_eq!(s.max_rate, 0.3);
        assert_eq!(s.lock, TransitionLock::None);
    }

    #[test]
    fn momentum_responsive_preset() {
        let s = Momentum::responsive();
        assert_eq!(s.acceleration, 2.0);
        assert_eq!(s.max_rate, 0.8);
    }

    #[test]
    fn momentum_duration_is_none() {
        let s = Momentum::new(1.0, 1.0);
        assert_eq!(s.duration_ms(), None);
    }

    #[test]
    fn momentum_completes_when_target_reached() {
        let s = Momentum::new(10.0, 10.0); // Fast acceleration
        let (val, done) = s.interpolate(0.0, 0.1, 1000);
        assert_eq!(val, 0.1);
        assert!(done);
    }

    // === TransitionLock and InterruptBehavior ===
    #[test]
    fn transition_lock_default_is_none() {
        assert_eq!(TransitionLock::default(), TransitionLock::None);
    }

    #[test]
    fn interrupt_behavior_default_is_replace() {
        assert_eq!(InterruptBehavior::default(), InterruptBehavior::Replace);
    }
}
