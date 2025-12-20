//! Unified shared state for all throttle controller services.
//!
//! `SharedThrottleState` provides thread-safe access to a single `ThrottleController`
//! that can be shared between web, MQTT, and physical input services.
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use rs_trainz::services::SharedThrottleState;
//!
//! let state = Arc::new(SharedThrottleState::new(controller));
//!
//! // Web service uses state.state() for reads
//! let snapshot = state.state();
//!
//! // MQTT service uses state.with_controller() for commands
//! state.with_controller(|controller| {
//!     controller.apply_command(cmd, CommandSource::Mqtt, state.now_ms());
//! });
//!
//! // Change detection for MQTT publishing
//! if let Some(changed_state) = state.check_changes() {
//!     // Publish changed_state to MQTT
//! }
//! ```

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::traits::MotorController;
use crate::{
    CommandOutcome, CommandSource, Direction, ThrottleCommandDyn, ThrottleController, ThrottleState,
};

// ============================================================================
// State Provider Trait
// ============================================================================

/// Trait for providing throttle state access.
///
/// This abstraction allows services (HTTP, MQTT, etc.) to work with different
/// state management strategies on different platforms.
pub trait StateProvider: Send + Sync {
    /// Get the current throttle state.
    fn state(&self) -> ThrottleState;

    /// Get the current timestamp in milliseconds.
    fn now_ms(&self) -> u64;

    /// Apply a command to the controller.
    fn apply_command(
        &self,
        cmd: ThrottleCommandDyn,
        source: CommandSource,
    ) -> Result<CommandOutcome, ()>;
}

// ============================================================================
// Change Detection
// ============================================================================

/// Tracks last known state for change detection (used by MQTT publishing)
#[derive(Clone, Debug)]
pub struct ChangeDetection {
    /// Last published speed value
    pub last_speed: f32,
    /// Last published direction
    pub last_direction: Direction,
}

impl Default for ChangeDetection {
    fn default() -> Self {
        Self {
            last_speed: 0.0,
            last_direction: Direction::Stopped,
        }
    }
}

// ============================================================================
// Shared Throttle State
// ============================================================================

/// Unified shared state for all services (web, MQTT, physical).
///
/// This struct wraps a single `ThrottleController` and provides thread-safe
/// access for multiple services. All services share the same controller instance,
/// ensuring real-time state synchronization across web API, MQTT, and physical controls.
///
/// # Thread Safety
///
/// - Uses `Mutex` for controller access (not `RwLock`) because the 20ms update loop
///   writes frequently, making `RwLock` writer starvation a concern.
/// - Change detection has a separate lock to minimize contention during MQTT publishes.
/// - All timestamp calculations use the same `start_time` for consistency.
pub struct SharedThrottleState<M: MotorController> {
    /// The throttle controller - needs mutable access for commands and updates
    controller: Mutex<ThrottleController<M>>,

    /// Time when the state was created (for consistent timestamps across services)
    start_time: Instant,

    /// Change detection for MQTT publishing (separate lock for less contention)
    change_detection: Mutex<ChangeDetection>,
}

impl<M: MotorController> SharedThrottleState<M> {
    /// Create new shared state wrapping a controller.
    ///
    /// The `start_time` is set to `Instant::now()`, which becomes the time base
    /// for all `now_ms()` calls across all services sharing this state.
    pub fn new(controller: ThrottleController<M>) -> Self {
        Self {
            controller: Mutex::new(controller),
            start_time: Instant::now(),
            change_detection: Mutex::new(ChangeDetection::default()),
        }
    }

    /// Get current timestamp in milliseconds since state creation.
    ///
    /// This is the unified time source for all services. Using the same time base
    /// ensures consistent behavior for priority lockouts and transition timing.
    #[inline]
    pub fn now_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// Get the start time instant (for external time calculations if needed).
    #[inline]
    pub fn start_time(&self) -> Instant {
        self.start_time
    }

    /// Access the controller with a mutable lock.
    ///
    /// Use this for operations that need mutable access like `apply_command()` or `update()`.
    /// The closure pattern prevents accidentally holding the lock across await points.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let now_ms = state.now_ms();
    /// state.with_controller(|controller| {
    ///     controller.apply_command(cmd, CommandSource::WebApi, now_ms)
    /// });
    /// ```
    pub fn with_controller<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut ThrottleController<M>) -> R,
    {
        let mut guard = self.controller.lock().unwrap();
        f(&mut *guard)
    }

    /// Get a read-only state snapshot.
    ///
    /// This acquires the controller lock briefly to get the current state.
    /// Preferred for web GET requests where you just need the current values.
    pub fn state(&self) -> ThrottleState {
        let now_ms = self.now_ms();
        let controller = self.controller.lock().unwrap();
        controller.state(now_ms)
    }

    /// Check for state changes since last check and update detection state.
    ///
    /// Returns `Some(ThrottleState)` if speed or direction changed since the last call,
    /// `None` if unchanged. Used by MQTT to publish only when state changes.
    ///
    /// # Example
    ///
    /// ```ignore
    /// if let Some(state) = shared_state.check_changes() {
    ///     // Publish state change to MQTT
    ///     mqtt_client.publish("train/state", &state).await;
    /// }
    /// ```
    pub fn check_changes(&self) -> Option<ThrottleState> {
        let now_ms = self.now_ms();

        // Get current state (brief lock)
        let state = {
            let controller = self.controller.lock().unwrap();
            controller.state(now_ms)
        };

        // Check for changes (separate lock)
        let mut detection = self.change_detection.lock().unwrap();
        let speed_changed = (state.speed - detection.last_speed).abs() > 0.001;
        let direction_changed = state.direction != detection.last_direction;

        if speed_changed || direction_changed {
            detection.last_speed = state.speed;
            detection.last_direction = state.direction;
            Some(state)
        } else {
            None
        }
    }

    /// Force synchronization of change detection state.
    ///
    /// Call this after external state changes (e.g., physical input) to update
    /// the change detection baseline without triggering a "change" event.
    pub fn sync_change_detection(&self) {
        let now_ms = self.now_ms();
        let state = {
            let controller = self.controller.lock().unwrap();
            controller.state(now_ms)
        };

        let mut detection = self.change_detection.lock().unwrap();
        detection.last_speed = state.speed;
        detection.last_direction = state.direction;
    }

    /// Get current change detection values (for debugging/testing).
    pub fn change_detection_state(&self) -> ChangeDetection {
        self.change_detection.lock().unwrap().clone()
    }
}

// ============================================================================
// StateProvider Implementation for Arc<SharedThrottleState>
// ============================================================================

impl<M: MotorController + Send + 'static> StateProvider for Arc<SharedThrottleState<M>> {
    fn state(&self) -> ThrottleState {
        SharedThrottleState::state(self)
    }

    fn now_ms(&self) -> u64 {
        SharedThrottleState::now_ms(self)
    }

    fn apply_command(
        &self,
        cmd: ThrottleCommandDyn,
        source: CommandSource,
    ) -> Result<CommandOutcome, ()> {
        let now_ms = self.now_ms();
        self.with_controller(|controller| {
            controller
                .apply_command(cmd, source, now_ms)
                .map_err(|_| ())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::MockMotor;
    use crate::{CommandSource, ThrottleCommand};

    // ========================================================================
    // SharedThrottleState tests
    // ========================================================================

    #[test]
    fn test_shared_state_creation() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = SharedThrottleState::new(controller);

        assert!(state.now_ms() < 100); // Should be very small right after creation
    }

    #[test]
    fn test_with_controller_access() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = SharedThrottleState::new(controller);

        // Should be able to access controller mutably
        state.with_controller(|c| {
            assert_eq!(c.current_speed(), 0.0);
        });
    }

    #[test]
    fn test_state_snapshot() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = SharedThrottleState::new(controller);

        let snapshot = state.state();
        assert_eq!(snapshot.speed, 0.0);
        assert_eq!(snapshot.direction, Direction::Stopped);
    }

    #[test]
    fn test_change_detection() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = SharedThrottleState::new(controller);

        // Initial state matches default detection state (0.0, Stopped)
        // so first check returns None
        let initial = state.check_changes();
        assert!(initial.is_none());

        // Apply speed change and update to apply it
        let now_ms = state.now_ms();
        state.with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.5).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now_ms);
            let _ = c.update(now_ms); // Apply the command
        });

        // Should detect change
        let changed = state.check_changes();
        assert!(changed.is_some());
        assert!((changed.unwrap().speed - 0.5).abs() < 0.01);

        // No further change
        let no_change = state.check_changes();
        assert!(no_change.is_none());
    }

    #[test]
    fn test_sync_change_detection() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = SharedThrottleState::new(controller);

        // Apply a change
        let now_ms = state.now_ms();
        state.with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.7).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now_ms);
        });

        // Sync without triggering change detection
        state.sync_change_detection();

        // Now check_changes should return None (already synced)
        let result = state.check_changes();
        assert!(result.is_none());
    }

    #[test]
    fn test_start_time_accessible() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = SharedThrottleState::new(controller);

        let start = state.start_time();
        assert!(start.elapsed().as_millis() < 100);
    }

    #[test]
    fn test_change_detection_state() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = SharedThrottleState::new(controller);

        let detection = state.change_detection_state();
        assert_eq!(detection.last_speed, 0.0);
        assert_eq!(detection.last_direction, Direction::Stopped);
    }

    // ========================================================================
    // StateProvider trait implementation tests
    // ========================================================================

    #[test]
    fn test_state_provider_state() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        // Test StateProvider::state() returns same as direct call
        let provider_state: ThrottleState = StateProvider::state(&state);
        let direct_state = SharedThrottleState::state(&*state);

        assert_eq!(provider_state.speed, direct_state.speed);
        assert_eq!(provider_state.direction, direct_state.direction);
    }

    #[test]
    fn test_state_provider_now_ms() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let t1: u64 = StateProvider::now_ms(&state);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let t2: u64 = StateProvider::now_ms(&state);

        assert!(t2 >= t1, "Time should advance");
        assert!(t2 - t1 >= 5, "At least 5ms should have passed");
    }

    #[test]
    fn test_state_provider_apply_command_success() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let cmd = ThrottleCommand::speed_immediate(0.6).into();
        let result = StateProvider::apply_command(&state, cmd, CommandSource::WebApi);

        assert!(result.is_ok());

        // Update and verify
        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = StateProvider::state(&state);
        assert!((current.speed - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_state_provider_apply_direction_command() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let cmd = ThrottleCommandDyn::SetDirection(Direction::Forward);
        let result = StateProvider::apply_command(&state, cmd, CommandSource::WebApi);

        assert!(result.is_ok());

        let current = StateProvider::state(&state);
        assert_eq!(current.direction, Direction::Forward);
    }

    #[test]
    fn test_state_provider_apply_estop() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        // First set a speed
        let cmd = ThrottleCommand::speed_immediate(0.8).into();
        let _ = StateProvider::apply_command(&state, cmd, CommandSource::Physical);
        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        // Now e-stop
        let cmd = ThrottleCommandDyn::EmergencyStop;
        let result = StateProvider::apply_command(&state, cmd, CommandSource::WebApi);

        assert!(result.is_ok());

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        let current = StateProvider::state(&state);
        assert!(current.speed.abs() < 0.01, "E-stop should set speed to 0");
    }

    #[test]
    fn test_state_provider_apply_max_speed() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let cmd = ThrottleCommandDyn::SetMaxSpeed(0.75);
        let result = StateProvider::apply_command(&state, cmd, CommandSource::WebApi);

        assert!(result.is_ok());

        let current = StateProvider::state(&state);
        assert!((current.max_speed - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_state_provider_multiple_sources() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        // Apply from different sources
        let cmd1 = ThrottleCommand::speed_immediate(0.3).into();
        let _ = StateProvider::apply_command(&state, cmd1, CommandSource::WebApi);

        let cmd2 = ThrottleCommand::speed_immediate(0.7).into();
        let _ = StateProvider::apply_command(&state, cmd2, CommandSource::Mqtt);

        let now = state.now_ms();
        state.with_controller(|c| c.update(now).unwrap());

        // Second command should have overwritten (unless priority lockout)
        let current = StateProvider::state(&state);
        assert!((current.speed - 0.7).abs() < 0.01 || (current.speed - 0.3).abs() < 0.01);
    }

    #[test]
    fn test_state_provider_concurrent_access() {
        use std::thread;

        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let state1 = state.clone();
        let state2 = state.clone();

        let handle1 = thread::spawn(move || {
            for i in 0..10 {
                let _ = StateProvider::state(&state1);
                let speed = (i as f32) / 20.0;
                let cmd = ThrottleCommand::speed_immediate(speed).into();
                let _ = StateProvider::apply_command(&state1, cmd, CommandSource::WebApi);
            }
        });

        let handle2 = thread::spawn(move || {
            for _ in 0..10 {
                let _ = StateProvider::state(&state2);
                let _ = StateProvider::now_ms(&state2);
            }
        });

        handle1.join().unwrap();
        handle2.join().unwrap();

        // Should complete without deadlock or panic
        let _ = StateProvider::state(&state);
    }
}
