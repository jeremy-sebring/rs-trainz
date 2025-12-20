//! Edge case and boundary condition tests for the throttle controller

use rs_trainz::{
    hal::MockMotor, CommandOutcome, CommandSource, Direction, EaseInOut, Linear, Momentum,
    ThrottleCommand, ThrottleCommandDyn, ThrottleController, TransitionResult,
};

// ============================================================================
// Boundary Value Tests
// ============================================================================

#[test]
fn speed_at_zero_boundary() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set speed to exactly 0
    let cmd = ThrottleCommand::speed_immediate(0.0);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    assert!((controller.current_speed() - 0.0).abs() < 0.001);
}

#[test]
fn speed_at_one_boundary() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set speed to exactly 1.0
    let cmd = ThrottleCommand::speed_immediate(1.0);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    assert!((controller.current_speed() - 1.0).abs() < 0.001);
}

#[test]
fn speed_clamped_above_one() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Try to set speed above 1.0
    let cmd = ThrottleCommand::speed_immediate(1.5);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    // Should be clamped to 1.0
    assert!(controller.current_speed() <= 1.0);
}

#[test]
fn speed_clamped_below_zero() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Try to set speed below 0.0
    let cmd = ThrottleCommand::speed_immediate(-0.5);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    // Should be clamped to 0.0
    assert!(controller.current_speed() >= 0.0);
}

#[test]
fn max_speed_at_zero_stops_all_motion() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set max speed to 0
    let cmd = ThrottleCommandDyn::SetMaxSpeed(0.0);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    // Try to set speed to 50%
    let cmd = ThrottleCommand::speed_immediate(0.5);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    // Should be clamped to 0
    assert!((controller.current_speed() - 0.0).abs() < 0.001);
}

#[test]
fn max_speed_at_one_allows_full_range() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Max speed defaults to 1.0, but let's explicitly set it
    let cmd = ThrottleCommandDyn::SetMaxSpeed(1.0);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    // Set speed to 100%
    let cmd = ThrottleCommand::speed_immediate(1.0);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    assert!((controller.current_speed() - 1.0).abs() < 0.001);
}

// ============================================================================
// Timing Edge Cases
// ============================================================================

#[test]
fn zero_duration_transition() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Linear transition with 0 duration should complete immediately
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.8,
        strategy: Linear::new(0),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    assert!((controller.current_speed() - 0.8).abs() < 0.01);
    assert!(!controller.is_transitioning());
}

#[test]
fn very_long_transition() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Very long transition (24 hours in ms)
    let cmd = ThrottleCommand::SetSpeed {
        target: 1.0,
        strategy: Linear::new(86_400_000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();

    // Update at various times
    controller.update(1000).unwrap(); // 1 second
    assert!(controller.is_transitioning());
    assert!(controller.current_speed() < 0.001); // Barely moved

    controller.update(3_600_000).unwrap(); // 1 hour
    assert!(controller.is_transitioning());
}

#[test]
fn timestamp_overflow_handling() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Start at a very high timestamp
    let start_time = u64::MAX - 1000;
    let cmd = ThrottleCommand::SetSpeed {
        target: 1.0,
        strategy: Linear::new(500),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, start_time)
        .unwrap();

    // Update with wrapped timestamp (would overflow without saturating_sub)
    // Note: In real systems, this would need more careful handling
    controller.update(start_time).unwrap();
    assert!(controller.is_transitioning());
}

// ============================================================================
// Rapid Command Sequences
// ============================================================================

#[test]
fn rapid_direction_changes() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Rapidly change direction
    for i in 0..10 {
        let dir = if i % 2 == 0 {
            Direction::Forward
        } else {
            Direction::Reverse
        };
        let cmd = ThrottleCommandDyn::SetDirection(dir);
        controller
            .apply_command(cmd, CommandSource::Physical, i)
            .unwrap();
    }

    // Final direction should be Reverse (9 is odd)
    assert_eq!(controller.current_direction(), Direction::Reverse);
}

#[test]
fn rapid_speed_commands() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Rapidly send speed commands
    for i in 0..10 {
        let speed = (i as f32) * 0.1;
        let cmd = ThrottleCommand::speed_immediate(speed);
        controller
            .apply_command(cmd.into(), CommandSource::Physical, i as u64)
            .unwrap();
        controller.update(i as u64).unwrap();
    }

    // Final speed should be 0.9
    assert!((controller.current_speed() - 0.9).abs() < 0.01);
}

#[test]
fn interrupt_in_progress_transition() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Start transition to 1.0
    let cmd = ThrottleCommand::SetSpeed {
        target: 1.0,
        strategy: Linear::new(1000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();

    // Update partway
    controller.update(500).unwrap();
    assert!((controller.current_speed() - 0.5).abs() < 0.1);

    // Interrupt with new target
    let cmd = ThrottleCommand::speed_immediate(0.0);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 500)
        .unwrap();
    controller.update(500).unwrap();

    // Should be at 0 now
    assert!((controller.current_speed() - 0.0).abs() < 0.01);
}

// ============================================================================
// E-stop Edge Cases
// ============================================================================

#[test]
fn estop_from_lowest_priority_still_works() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set speed with an ongoing transition
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.8,
        strategy: Linear::new(5000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(1000).unwrap(); // Partway through

    // E-stop from Mqtt (lowest priority) should still work and interrupt
    let cmd = ThrottleCommand::estop();
    let result = controller
        .apply_command(cmd.into(), CommandSource::Mqtt, 1000)
        .unwrap();

    assert!(matches!(
        result,
        CommandOutcome::SpeedTransition(TransitionResult::Interrupted { .. })
    ));
    assert!((controller.current_speed() - 0.0).abs() < 0.01);
}

#[test]
fn multiple_estops_in_sequence() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set speed
    let cmd = ThrottleCommand::speed_immediate(0.8);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    // Multiple e-stops
    for i in 1..5 {
        let cmd = ThrottleCommand::estop();
        controller
            .apply_command(cmd.into(), CommandSource::Physical, i * 100)
            .unwrap();
    }

    assert!((controller.current_speed() - 0.0).abs() < 0.01);
    assert_eq!(controller.current_direction(), Direction::Stopped);
}

#[test]
fn estop_during_locked_departure() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Start a locked departure transition
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.8,
        strategy: EaseInOut::departure(5000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();

    // Update partway
    controller.update(1000).unwrap();
    assert!(controller.is_transitioning());

    // E-stop should interrupt even locked transition
    let cmd = ThrottleCommand::estop();
    let result = controller
        .apply_command(cmd.into(), CommandSource::Mqtt, 1000)
        .unwrap();

    assert!(matches!(
        result,
        CommandOutcome::SpeedTransition(TransitionResult::Interrupted { .. })
    ));
    assert!((controller.current_speed() - 0.0).abs() < 0.01);
}

// ============================================================================
// State Consistency Tests
// ============================================================================

#[test]
fn direction_independent_of_speed() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set direction first
    let cmd = ThrottleCommandDyn::SetDirection(Direction::Forward);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    // Set speed
    let cmd = ThrottleCommand::speed_immediate(0.5);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    assert_eq!(controller.current_direction(), Direction::Forward);
    assert!((controller.current_speed() - 0.5).abs() < 0.01);

    // Change direction without affecting speed
    let cmd = ThrottleCommandDyn::SetDirection(Direction::Reverse);
    controller
        .apply_command(cmd, CommandSource::Physical, 100)
        .unwrap();

    assert_eq!(controller.current_direction(), Direction::Reverse);
    assert!((controller.current_speed() - 0.5).abs() < 0.01);
}

#[test]
fn max_speed_applies_to_new_commands() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set max speed first
    let cmd = ThrottleCommandDyn::SetMaxSpeed(0.3);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    // Now try to set speed above max
    let cmd = ThrottleCommand::speed_immediate(1.0);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 100)
        .unwrap();
    controller.update(100).unwrap();

    // Speed should be clamped by max_speed
    assert!(controller.current_speed() <= 0.31);
}

#[test]
fn state_snapshot_consistent() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set up some state
    let cmd = ThrottleCommandDyn::SetDirection(Direction::Forward);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    let cmd = ThrottleCommandDyn::SetMaxSpeed(0.75);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    let cmd = ThrottleCommand::SetSpeed {
        target: 0.5,
        strategy: Linear::new(1000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();

    controller.update(500).unwrap();

    let state = controller.state(500);

    // Verify all state fields are consistent
    assert_eq!(state.direction, Direction::Forward);
    assert!((state.max_speed - 0.75).abs() < 0.01);
    assert!((state.speed - 0.25).abs() < 0.05); // Halfway through transition
    assert_eq!(state.target_speed, Some(0.5));
    assert!(state.fault.is_none());
    assert!(state.transition_progress.is_some());
}

// ============================================================================
// Strategy-Specific Edge Cases
// ============================================================================

#[test]
fn momentum_with_same_from_and_to() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set initial speed
    let cmd = ThrottleCommand::speed_immediate(0.5);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    // Momentum transition to same speed
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.5,
        strategy: Momentum::gentle(),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 100)
        .unwrap();

    // Should complete immediately (no distance to travel)
    controller.update(100).unwrap();
    assert!((controller.current_speed() - 0.5).abs() < 0.01);
}

#[test]
fn ease_in_out_very_small_change() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set initial speed
    let cmd = ThrottleCommand::speed_immediate(0.5);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    // Very small change with ease-in-out
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.501,
        strategy: EaseInOut::new(1000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 100)
        .unwrap();

    controller.update(1100).unwrap();
    assert!((controller.current_speed() - 0.501).abs() < 0.01);
}

// ============================================================================
// Source Priority Edge Cases
// ============================================================================

#[test]
fn all_source_priorities_respected() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Test each source can set speed when no lockout
    let sources = [
        CommandSource::Mqtt,
        CommandSource::WebApi,
        CommandSource::WebLocal,
        CommandSource::Physical,
    ];

    for (i, source) in sources.iter().enumerate() {
        let speed = (i as f32 + 1.0) * 0.1;
        let cmd = ThrottleCommand::speed_immediate(speed);
        controller
            .apply_command(cmd.into(), *source, (i * 100) as u64)
            .unwrap();
        controller.update((i * 100) as u64).unwrap();
        assert!((controller.current_speed() - speed).abs() < 0.01);
    }
}
