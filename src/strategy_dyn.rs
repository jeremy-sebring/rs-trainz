//! Type-erased execution strategies for runtime polymorphism.
//!
//! This module provides [`AnyStrategy`], a type-erased wrapper for execution
//! strategies that enables storing different strategy types together.
//!
//! # When to Use
//!
//! Use type erasure when you need to:
//! - Store commands with different strategies in a queue
//! - Accept strategies from external sources (API, config)
//! - Mix strategy types at runtime
//!
//! # How It Works
//!
//! The [`ExecutionStrategyDyn`] trait is an object-safe version of
//! [`ExecutionStrategy`]. [`AnyStrategy`] wraps any strategy implementing
//! `ExecutionStrategy + Send + Sync + 'static` in an `Arc` for cheap cloning.
//!
//! ```rust
//! use rs_trainz::{AnyStrategy, traits::{Linear, EaseInOut, Momentum}};
//!
//! // Different strategies, same type
//! let strategies: Vec<AnyStrategy> = vec![
//!     AnyStrategy::new(Linear::new(1000)),
//!     AnyStrategy::new(EaseInOut::departure(2000)),
//!     AnyStrategy::new(Momentum::gentle()),
//! ];
//!
//! for strategy in &strategies {
//!     println!("Duration: {:?}", strategy.duration_ms());
//! }
//! ```
//!
//! # Performance
//!
//! Type erasure adds a small overhead:
//! - One allocation per strategy (amortized via Arc)
//! - Virtual dispatch for trait methods
//!
//! For most throttle control applications, this overhead is negligible.
//!
//! [`ExecutionStrategy`]: crate::traits::ExecutionStrategy

extern crate alloc;

use crate::traits::{ExecutionStrategy, InterruptBehavior, TransitionLock};
use alloc::sync::Arc;

/// Object-safe version of [`ExecutionStrategy`].
///
/// This trait removes the `Clone` requirement to enable dynamic dispatch.
/// It's automatically implemented for all types that implement
/// `ExecutionStrategy + Send + Sync + 'static`.
///
/// You typically don't interact with this trait directly; use [`AnyStrategy`]
/// instead.
///
/// [`ExecutionStrategy`]: crate::traits::ExecutionStrategy
pub trait ExecutionStrategyDyn: Send + Sync {
    /// Interpolate the value during a transition.
    fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool);
    /// Estimated total duration in milliseconds (if known).
    fn duration_ms(&self) -> Option<u64>;
    /// What lock level does this transition require?
    fn lock(&self) -> TransitionLock;
    /// What happens if something tries to interrupt?
    fn on_interrupt(&self) -> InterruptBehavior;
}

/// Blanket implementation for any ExecutionStrategy
impl<S: ExecutionStrategy + Send + Sync + 'static> ExecutionStrategyDyn for S {
    fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool) {
        ExecutionStrategy::interpolate(self, from, to, elapsed_ms)
    }

    fn duration_ms(&self) -> Option<u64> {
        ExecutionStrategy::duration_ms(self)
    }

    fn lock(&self) -> TransitionLock {
        ExecutionStrategy::lock(self)
    }

    fn on_interrupt(&self) -> InterruptBehavior {
        ExecutionStrategy::on_interrupt(self)
    }
}

/// Type-erased wrapper for any execution strategy.
///
/// Wraps any [`ExecutionStrategy`] implementation in an `Arc` for
/// cheap cloning and dynamic dispatch.
///
/// # Example
///
/// ```rust
/// use rs_trainz::{AnyStrategy, ExecutionStrategy};
/// use rs_trainz::traits::{Linear, TransitionLock};
///
/// let strategy = AnyStrategy::new(Linear::locked(1000));
///
/// // Use like any strategy
/// assert_eq!(strategy.duration_ms(), Some(1000));
/// assert_eq!(strategy.lock(), TransitionLock::Hard);
///
/// let (value, complete) = strategy.interpolate(0.0, 1.0, 500);
/// assert!((value - 0.5).abs() < 0.01);
/// ```
///
/// [`ExecutionStrategy`]: crate::traits::ExecutionStrategy
#[derive(Clone)]
pub struct AnyStrategy {
    inner: Arc<dyn ExecutionStrategyDyn>,
}

impl core::fmt::Debug for AnyStrategy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AnyStrategy")
            .field("duration_ms", &self.duration_ms())
            .field("lock", &self.lock())
            .field("on_interrupt", &self.on_interrupt())
            .finish()
    }
}

impl AnyStrategy {
    /// Wrap a concrete strategy in a type-erased container
    pub fn new<S: ExecutionStrategy + Send + Sync + 'static>(strategy: S) -> Self {
        Self {
            inner: Arc::new(strategy),
        }
    }

    /// Interpolates between values using the wrapped strategy.
    pub fn interpolate(&self, from: f32, to: f32, elapsed_ms: u64) -> (f32, bool) {
        self.inner.interpolate(from, to, elapsed_ms)
    }

    /// Returns the estimated duration in milliseconds, if known.
    pub fn duration_ms(&self) -> Option<u64> {
        self.inner.duration_ms()
    }

    /// Returns the transition lock level.
    pub fn lock(&self) -> TransitionLock {
        self.inner.lock()
    }

    /// Returns the interrupt behavior.
    pub fn on_interrupt(&self) -> InterruptBehavior {
        self.inner.on_interrupt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EaseInOut, Immediate, Linear, Momentum};

    #[test]
    fn any_strategy_from_immediate() {
        let strategy = AnyStrategy::new(Immediate::default());
        assert_eq!(strategy.duration_ms(), Some(0));
        assert_eq!(strategy.lock(), TransitionLock::None);
        assert_eq!(strategy.on_interrupt(), InterruptBehavior::Replace);

        let (value, complete) = strategy.interpolate(0.0, 1.0, 0);
        assert!((value - 1.0).abs() < 0.01);
        assert!(complete);
    }

    #[test]
    fn any_strategy_from_linear() {
        let strategy = AnyStrategy::new(Linear::new(1000));
        assert_eq!(strategy.duration_ms(), Some(1000));
        assert_eq!(strategy.lock(), TransitionLock::None);

        // At t=0, should be at start
        let (value, complete) = strategy.interpolate(0.0, 1.0, 0);
        assert!((value - 0.0).abs() < 0.01);
        assert!(!complete);

        // At t=500, should be halfway
        let (value, complete) = strategy.interpolate(0.0, 1.0, 500);
        assert!((value - 0.5).abs() < 0.01);
        assert!(!complete);

        // At t=1000, should be complete
        let (value, complete) = strategy.interpolate(0.0, 1.0, 1000);
        assert!((value - 1.0).abs() < 0.01);
        assert!(complete);
    }

    #[test]
    fn any_strategy_from_ease_in_out() {
        let strategy = AnyStrategy::new(EaseInOut::new(1000));
        assert_eq!(strategy.duration_ms(), Some(1000));
        assert_eq!(strategy.lock(), TransitionLock::None);
        assert_eq!(strategy.on_interrupt(), InterruptBehavior::Replace);
    }

    #[test]
    fn any_strategy_from_departure() {
        let strategy = AnyStrategy::new(EaseInOut::departure(2000));
        assert_eq!(strategy.duration_ms(), Some(2000));
        assert_eq!(strategy.lock(), TransitionLock::Hard);
        assert_eq!(strategy.on_interrupt(), InterruptBehavior::Reject);
    }

    #[test]
    fn any_strategy_from_arrival() {
        let strategy = AnyStrategy::new(EaseInOut::arrival(1500));
        assert_eq!(strategy.duration_ms(), Some(1500));
        assert_eq!(strategy.lock(), TransitionLock::Source);
        assert_eq!(strategy.on_interrupt(), InterruptBehavior::Queue);
    }

    #[test]
    fn any_strategy_from_momentum() {
        let strategy = AnyStrategy::new(Momentum::gentle());
        // Momentum has unknown duration
        assert_eq!(strategy.duration_ms(), None);
        assert_eq!(strategy.lock(), TransitionLock::None);
    }

    #[test]
    fn any_strategy_clone() {
        let strategy1 = AnyStrategy::new(Linear::new(500));
        let strategy2 = strategy1.clone();

        // Both should behave identically
        assert_eq!(strategy1.duration_ms(), strategy2.duration_ms());
        assert_eq!(strategy1.lock(), strategy2.lock());

        let (v1, c1) = strategy1.interpolate(0.0, 1.0, 250);
        let (v2, c2) = strategy2.interpolate(0.0, 1.0, 250);
        assert!((v1 - v2).abs() < 0.001);
        assert_eq!(c1, c2);
    }

    #[test]
    fn any_strategy_debug() {
        let strategy = AnyStrategy::new(Linear::new(1000));
        let debug_str = format!("{:?}", strategy);
        assert!(debug_str.contains("AnyStrategy"));
        assert!(debug_str.contains("duration_ms"));
        assert!(debug_str.contains("1000"));
    }

    #[test]
    fn any_strategy_linear_locked() {
        let strategy = AnyStrategy::new(Linear::locked(800));
        assert_eq!(strategy.duration_ms(), Some(800));
        assert_eq!(strategy.lock(), TransitionLock::Hard);
    }

    #[test]
    fn any_strategy_linear_source_locked() {
        let strategy = AnyStrategy::new(Linear::source_locked(600));
        assert_eq!(strategy.duration_ms(), Some(600));
        assert_eq!(strategy.lock(), TransitionLock::Source);
    }

    #[test]
    fn any_strategy_interpolate_reverse() {
        let strategy = AnyStrategy::new(Linear::new(1000));

        // Going from high to low
        let (value, _) = strategy.interpolate(1.0, 0.0, 500);
        assert!((value - 0.5).abs() < 0.01);
    }

    #[test]
    fn any_strategy_with_different_ranges() {
        let strategy = AnyStrategy::new(Linear::new(100));

        // Non-zero start
        let (value, _) = strategy.interpolate(0.2, 0.8, 50);
        assert!((value - 0.5).abs() < 0.01);

        // Small range
        let (value, _) = strategy.interpolate(0.4, 0.5, 50);
        assert!((value - 0.45).abs() < 0.01);
    }
}
