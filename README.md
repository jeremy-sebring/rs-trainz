# rs-trainz

A Rust-based DC model train throttle controller with support for physical controls, web UI, and MQTT integration. Designed to run on ESP32-C3 but fully testable on desktop.

## Features

- **Hardware Abstraction**: Clean traits for motor control, encoder input, and fault detection
- **Multiple Control Sources**: Physical knob, web API, MQTT with configurable priority
- **Smooth Transitions**: Configurable execution strategies
  - `Immediate` - instant changes
  - `Linear` - constant rate
  - `EaseInOut` - smooth acceleration/deceleration
  - `Momentum` - physics-based feel
- **Transition Locks**: Protect important transitions (departures/arrivals) from interruption
- **Priority System**: E-stop always wins, physical controls override remote commands
- **Source Lockout**: Physical control "takes over" for a configurable duration

## Architecture

```
src/
├── lib.rs              # Re-exports and documentation
├── traits/             # Hardware and network abstractions
│   ├── hardware.rs     # MotorController, EncoderInput, FaultDetector
│   ├── network.rs      # MqttClient, HttpServer
│   └── strategy.rs     # ExecutionStrategy implementations
├── commands.rs         # ThrottleCommand with priority system
├── priority.rs         # CommandQueue, SourceLockout
├── transition.rs       # TransitionManager with locks
├── throttle.rs         # Main ThrottleController
├── strategy_dyn.rs     # Type-erased strategies for queuing
└── hal/
    ├── mock.rs         # Mock implementations for testing
    └── (esp32.rs)      # ESP32 implementations (TODO)
```

## Quick Start

```rust
use rs_trainz::{
    ThrottleController, ThrottleCommand, CommandSource,
    hal::MockMotor,
    traits::EaseInOut,
};

// Create controller with mock motor (or real hardware)
let motor = MockMotor::new();
let mut controller = ThrottleController::new(motor);

// Immediate speed change from physical knob
let cmd = ThrottleCommand::speed_immediate(0.5);
controller.apply_command(cmd.into(), CommandSource::Physical, now_ms)?;

// Smooth departure from MQTT automation
let cmd = ThrottleCommand::SetSpeed {
    target: 0.8,
    strategy: EaseInOut::departure(3000), // 3 second locked transition
};
controller.apply_command(cmd.into(), CommandSource::Mqtt, now_ms)?;

// In your main loop (every ~20ms)
loop {
    controller.update(now_ms)?;
    // ... read encoder, handle web requests, etc.
}
```

## Command Priority

| Source | Priority | Use Case |
|--------|----------|----------|
| `Emergency` | Highest | E-stop from any source |
| `Fault` | High | System-detected short/overcurrent |
| `Physical` | Medium-High | Knob, buttons on the controller |
| `WebLocal` | Medium | Web UI on same network |
| `WebApi` | Medium-Low | Remote web API |
| `Mqtt` | Lowest | Home Assistant / automation |

E-stop commands are automatically promoted to `Emergency` priority regardless of source.

## Transition Locks

```rust
// Unlocked - can be interrupted by anything
Linear::new(1000)

// Source-locked - only same or higher priority can interrupt  
Linear::source_locked(1000)

// Hard-locked - only e-stop can interrupt
EaseInOut::departure(3000)

// Arrival mode - queues follow-up commands instead of rejecting
EaseInOut::arrival(4000)
```

## Building

```bash
# Desktop (for testing)
cargo build
cargo test

# ESP32-C3 (requires esp toolchain)
cargo build --target riscv32imc-unknown-none-elf --features esp32
```

## Hardware Requirements

For actual hardware deployment:

- ESP32-C3 dev board (~$6)
- IBT-2 (BTS7960) H-bridge motor driver (~$10)
- 12V 3A power supply
- Rotary encoder with push button
- Optional: 128x64 OLED display

## License

MIT
