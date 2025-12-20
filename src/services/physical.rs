//! Physical input handler for encoder-based throttle control.
//!
//! This module provides a handler for physical rotary encoders and buttons
//! that integrates with the shared throttle state.
//!
//! # Usage
//!
//! ```ignore
//! use rs_trainz::services::{PhysicalInputHandler, SharedThrottleState};
//! use rs_trainz::hal::MockEncoder;
//!
//! let state = Arc::new(SharedThrottleState::new(controller));
//! let encoder = MockEncoder::new();
//! let mut handler = PhysicalInputHandler::new(Arc::clone(&state), encoder);
//!
//! // In your update loop:
//! handler.poll();
//! ```

use std::sync::Arc;

use crate::traits::{EncoderInput, MotorController};
use crate::{CommandSource, ThrottleCommand};

use super::shared::SharedThrottleState;

/// Handler for physical encoder input.
///
/// Polls an encoder and applies speed changes to the shared throttle state
/// using `CommandSource::Physical` priority (higher than web/MQTT).
pub struct PhysicalInputHandler<M: MotorController, E: EncoderInput> {
    /// Shared throttle state
    state: Arc<SharedThrottleState<M>>,
    /// The encoder input device
    encoder: E,
    /// Speed change per encoder click (0.0-1.0 range per click)
    sensitivity: f32,
    /// Dead zone for filtering encoder noise (minimum delta to register)
    dead_zone: i32,
}

impl<M: MotorController, E: EncoderInput> PhysicalInputHandler<M, E> {
    /// Create a new physical input handler.
    ///
    /// # Arguments
    ///
    /// * `state` - Shared throttle state to modify
    /// * `encoder` - The encoder input device
    ///
    /// Default sensitivity is 0.05 (5% speed change per click).
    pub fn new(state: Arc<SharedThrottleState<M>>, encoder: E) -> Self {
        Self {
            state,
            encoder,
            sensitivity: 0.05,
            dead_zone: 0,
        }
    }

    /// Set the sensitivity (speed change per encoder click).
    ///
    /// For example, 0.05 means 5% speed change per click.
    /// A 20-step encoder would need 20 clicks for full speed.
    pub fn with_sensitivity(mut self, sensitivity: f32) -> Self {
        self.sensitivity = sensitivity;
        self
    }

    /// Set the dead zone (minimum delta to register a change).
    ///
    /// Useful for filtering encoder noise. A value of 1 means
    /// single clicks are ignored, only 2+ click movements register.
    pub fn with_dead_zone(mut self, dead_zone: i32) -> Self {
        self.dead_zone = dead_zone;
        self
    }

    /// Poll the encoder and apply any speed changes.
    ///
    /// Call this frequently (e.g., every 10-20ms) in your main loop.
    /// Returns `true` if a command was applied, `false` otherwise.
    pub fn poll(&mut self) -> bool {
        let now_ms = self.state.now_ms();

        // Check for button press (e-stop)
        if self.encoder.button_just_pressed() {
            let cmd = ThrottleCommand::estop().into();
            self.state.with_controller(|controller| {
                let _ = controller.apply_command(cmd, CommandSource::Physical, now_ms);
            });
            return true;
        }

        // Check encoder rotation
        let delta = self.encoder.read_delta();

        if delta.abs() <= self.dead_zone {
            return false;
        }

        // Calculate new speed
        let speed_delta = delta as f32 * self.sensitivity;

        self.state.with_controller(|controller| {
            let current = controller.current_speed();
            let new_speed = (current + speed_delta).clamp(0.0, 1.0);

            // Only apply if speed actually changed
            if (new_speed - current).abs() > 0.001 {
                let cmd = ThrottleCommand::speed_immediate(new_speed).into();
                let _ = controller.apply_command(cmd, CommandSource::Physical, now_ms);
            }
        });

        true
    }

    /// Get a reference to the encoder.
    pub fn encoder(&self) -> &E {
        &self.encoder
    }

    /// Get a mutable reference to the encoder.
    pub fn encoder_mut(&mut self) -> &mut E {
        &mut self.encoder
    }

    /// Get the current sensitivity.
    pub fn sensitivity(&self) -> f32 {
        self.sensitivity
    }

    /// Get the current dead zone.
    pub fn dead_zone(&self) -> i32 {
        self.dead_zone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::{MockEncoder, MockMotor};
    use crate::ThrottleController;

    #[test]
    fn test_encoder_speed_change() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let encoder = MockEncoder::new();
        let mut handler =
            PhysicalInputHandler::new(Arc::clone(&state), encoder).with_sensitivity(0.1); // 10% per click

        // Simulate 3 clicks clockwise (queue_delta adds to the queue, read_delta pops)
        handler.encoder_mut().queue_delta(3);
        assert!(handler.poll());

        // Update controller to apply the command to the state snapshot
        let now_ms = state.now_ms();
        state.with_controller(|c| {
            let _ = c.update(now_ms);
        });

        // Should have increased speed by 30%
        let current = state.state().speed;
        assert!((current - 0.3).abs() < 0.01);
    }

    #[test]
    fn test_encoder_estop() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        // Set initial speed
        let now_ms = state.now_ms();
        state.with_controller(|c| {
            let cmd = ThrottleCommand::speed_immediate(0.5).into();
            let _ = c.apply_command(cmd, CommandSource::Physical, now_ms);
        });

        let encoder = MockEncoder::new();
        let mut handler = PhysicalInputHandler::new(Arc::clone(&state), encoder);

        // Simulate button press
        handler.encoder_mut().press_button();
        assert!(handler.poll());

        // Speed should be 0 after e-stop
        let current = state.state().speed;
        assert!(current < 0.01);
    }

    #[test]
    fn test_dead_zone() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let encoder = MockEncoder::new();
        let mut handler = PhysicalInputHandler::new(Arc::clone(&state), encoder).with_dead_zone(2);

        // Single click should be ignored
        handler.encoder_mut().queue_delta(1);
        assert!(!handler.poll());

        // Three clicks should work
        handler.encoder_mut().queue_delta(3);
        assert!(handler.poll());
    }

    #[test]
    fn test_speed_clamping() {
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);
        let state = Arc::new(SharedThrottleState::new(controller));

        let encoder = MockEncoder::new();
        let mut handler =
            PhysicalInputHandler::new(Arc::clone(&state), encoder).with_sensitivity(0.5); // Large sensitivity

        // Try to go above 1.0
        handler.encoder_mut().queue_delta(10);
        handler.poll();
        assert!(state.state().speed <= 1.0);

        // Try to go below 0.0
        handler.encoder_mut().queue_delta(-30);
        handler.poll();
        assert!(state.state().speed >= 0.0);
    }
}
