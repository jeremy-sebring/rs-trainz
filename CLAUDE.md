# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
# Build (desktop/testing)
cargo build

# Run all tests
cargo test

# Run a single test
cargo test test_name

# Check without std (no_std compatibility)
cargo check --no-default-features

# Build for ESP32-C3
cargo build --target riscv32imc-unknown-none-elf --features esp32
```

## Architecture

This is a DC model train throttle controller library designed to be testable on desktop while targeting ESP32-C3 hardware. It uses `no_std` compatible code with the `heapless` crate for fixed-capacity collections.

### Core Flow

Commands flow through a priority system before reaching the motor:

1. **Command Sources** (`commands.rs`) - Commands arrive from different sources with implicit priority: `Emergency > Fault > Physical > WebLocal > WebApi > Mqtt`. E-stop commands from any source are automatically promoted to Emergency priority.

2. **Source Lockout** (`priority.rs`) - When a high-priority source (Physical or above) sends a command, it creates a "lockout" that rejects lower-priority commands for a configurable duration. This prevents remote commands from fighting with physical knob control.

3. **Transition Manager** (`transition.rs`) - Speed changes can be instant or interpolated over time. Transitions can be "locked" to prevent interruption:
   - `TransitionLock::None` - any command can interrupt
   - `TransitionLock::Source` - only same/higher priority can interrupt
   - `TransitionLock::Hard` - only e-stop can interrupt

4. **ThrottleController** (`throttle.rs`) - The main controller that owns the motor and coordinates commands, transitions, and state.

### Strategy Pattern

Speed transitions use an `ExecutionStrategy` trait (`traits/strategy.rs`) with four implementations:
- `Immediate` - instant change
- `Linear` - constant rate over duration
- `EaseInOut` - smoothstep curve (good for departures/arrivals)
- `Momentum` - physics-based acceleration

Strategies can be used with compile-time types (`ThrottleCommand<S>`) or type-erased for runtime mixing (`ThrottleCommandDyn` via `AnyStrategy`).

### Hardware Abstraction

The `traits/` module defines hardware interfaces (`MotorController`, `EncoderInput`, `FaultDetector`) and network interfaces (`MqttClient`, `HttpServer`). The `hal/mock.rs` module provides test implementations that track calls and allow injecting test data.

## Key Patterns

- **Transition Locks**: Use `EaseInOut::departure()` for station departures (hard-locked, rejects interrupts) and `EaseInOut::arrival()` for arrivals (queues follow-up commands).

- **Type Erasure**: `ThrottleCommand<S>` converts to `ThrottleCommandDyn` via `.into()` for queuing commands with different strategies together.

- **State Snapshots**: `controller.state(now_ms)` returns a `ThrottleState` struct with all current state for UI/API serialization.

## Feature Flags

- `default = ["std"]` - Standard library support
- `esp32` - Enables embassy async runtime dependencies for ESP32 deployment
