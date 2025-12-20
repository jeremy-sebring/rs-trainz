//! Integration tests for the throttle controller

use rs_trainz::{
    hal::MockMotor, CommandOutcome, CommandSource, Direction, EaseInOut, Linear, ThrottleCommand,
    ThrottleCommandDyn, ThrottleController, TransitionResult,
};

#[test]
fn immediate_speed_change() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    let cmd = ThrottleCommand::speed_immediate(0.5);
    let result = controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();

    assert!(matches!(
        result,
        CommandOutcome::SpeedTransition(TransitionResult::Started)
    ));

    // After update, speed should be at target
    controller.update(0).unwrap();
    assert!((controller.current_speed() - 0.5).abs() < 0.01);
}

#[test]
fn linear_transition() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Start a 1000ms linear transition from 0 to 1.0
    let cmd = ThrottleCommand::SetSpeed {
        target: 1.0,
        strategy: Linear::new(1000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();

    // At t=0, should still be at 0
    controller.update(0).unwrap();
    assert!((controller.current_speed() - 0.0).abs() < 0.01);

    // At t=500ms, should be at 0.5
    controller.update(500).unwrap();
    assert!((controller.current_speed() - 0.5).abs() < 0.01);

    // At t=1000ms, should be at 1.0
    controller.update(1000).unwrap();
    assert!((controller.current_speed() - 1.0).abs() < 0.01);

    // Transition should be complete
    assert!(!controller.is_transitioning());
}

#[test]
fn estop_interrupts_locked_transition() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Start a locked departure transition
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.8,
        strategy: EaseInOut::departure(3000), // locked
    };
    controller
        .apply_command(cmd.into(), CommandSource::Mqtt, 0)
        .unwrap();

    // Advance partway through
    controller.update(500).unwrap();
    assert!(controller.is_transitioning());

    // Try to interrupt with a regular command - should be rejected
    let cmd = ThrottleCommand::speed_immediate(0.2);
    let result = controller
        .apply_command(cmd.into(), CommandSource::WebApi, 500)
        .unwrap();
    assert!(matches!(
        result,
        CommandOutcome::SpeedTransition(TransitionResult::Rejected { .. })
    ));

    // E-stop should work
    let cmd = ThrottleCommand::estop();
    let result = controller
        .apply_command(cmd.into(), CommandSource::WebApi, 600)
        .unwrap();
    assert!(matches!(
        result,
        CommandOutcome::SpeedTransition(TransitionResult::Interrupted { .. })
    ));

    // Speed should be 0
    assert!((controller.current_speed() - 0.0).abs() < 0.01);
    assert_eq!(controller.current_direction(), Direction::Stopped);
}

#[test]
fn direction_change() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    assert_eq!(controller.current_direction(), Direction::Stopped);

    let cmd = ThrottleCommandDyn::SetDirection(Direction::Forward);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    assert_eq!(controller.current_direction(), Direction::Forward);

    let cmd = ThrottleCommandDyn::SetDirection(Direction::Reverse);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    assert_eq!(controller.current_direction(), Direction::Reverse);
}

#[test]
fn queued_transition() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set initial speed so arrival transition has work to do
    let cmd = ThrottleCommand::speed_immediate(0.5);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();
    assert!((controller.current_speed() - 0.5).abs() < 0.01);

    // Start an arrival transition from Physical (high priority)
    // EaseInOut::arrival uses Source lock + Queue interrupt behavior
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.0,
        strategy: EaseInOut::arrival(1000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 100)
        .unwrap();

    // Try to queue a departure from Mqtt (lower priority than Physical)
    // Because the active transition is Source-locked from Physical,
    // this lower-priority command should be queued (not rejected, not replacing)
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.8,
        strategy: EaseInOut::departure(1000),
    };
    let result = controller
        .apply_command(cmd.into(), CommandSource::Mqtt, 200)
        .unwrap();

    assert!(matches!(
        result,
        CommandOutcome::SpeedTransition(TransitionResult::Queued)
    ));

    // Complete the arrival (started at t=100, duration=1000, so complete at t=1100)
    controller.update(1100).unwrap();
    assert!((controller.current_speed() - 0.0).abs() < 0.01);

    // On next update, queued transition should start
    controller.update(1101).unwrap();
    assert!(controller.is_transitioning());

    // Complete the departure (started at t=1101, duration=1000, so complete at t=2101)
    controller.update(2101).unwrap();
    assert!((controller.current_speed() - 0.8).abs() < 0.01);
}

#[test]
fn max_speed_limit() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set max speed to 50%
    let cmd = ThrottleCommandDyn::SetMaxSpeed(0.5);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    // Try to set speed to 80%
    let cmd = ThrottleCommand::speed_immediate(0.8);
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();
    controller.update(0).unwrap();

    // Should be clamped to 50%
    assert!((controller.current_speed() - 0.5).abs() < 0.01);
}

#[test]
fn state_snapshot() {
    let motor = MockMotor::new();
    let mut controller = ThrottleController::new(motor);

    // Set up some state
    let cmd = ThrottleCommand::SetSpeed {
        target: 0.6,
        strategy: Linear::new(1000),
    };
    controller
        .apply_command(cmd.into(), CommandSource::Physical, 0)
        .unwrap();

    let cmd = ThrottleCommandDyn::SetDirection(Direction::Forward);
    controller
        .apply_command(cmd, CommandSource::Physical, 0)
        .unwrap();

    controller.update(500).unwrap();

    let state = controller.state(500);
    assert!((state.speed - 0.3).abs() < 0.05); // halfway through transition
    assert_eq!(state.target_speed, Some(0.6));
    assert_eq!(state.direction, Direction::Forward);
    assert_eq!(state.max_speed, 1.0);
    assert!(state.fault.is_none());
    assert!(state.transition_progress.is_some());
}
