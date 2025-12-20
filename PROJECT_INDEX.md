# Project Index: rs-trainz

Generated: 2025-12-12

## Overview

**rs-trainz** - DC model train throttle controller with physical, web, and MQTT control.

| Metric | Value |
|--------|-------|
| Version | 0.1.0 |
| Language | Rust (Edition 2021) |
| License | MIT |
| Lines of Code | ~950 |
| Test Count | 7 integration tests |

## Project Structure

```
rs-trainz/
├── src/
│   ├── lib.rs              # Crate root, re-exports
│   ├── commands.rs         # Command types + priority system
│   ├── priority.rs         # CommandQueue + SourceLockout
│   ├── transition.rs       # TransitionManager + locks
│   ├── throttle.rs         # Main ThrottleController
│   ├── strategy_dyn.rs     # Type-erased strategies
│   ├── traits/
│   │   ├── hardware.rs     # MotorController, EncoderInput, FaultDetector
│   │   ├── network.rs      # MqttClient, HttpServer
│   │   └── strategy.rs     # ExecutionStrategy implementations
│   └── hal/
│       └── mock.rs         # Mock implementations for testing
├── tests/
│   └── throttle_tests.rs   # Integration tests
├── Cargo.toml
├── README.md
└── CLAUDE.md
```

## Entry Points

| Entry | Path | Purpose |
|-------|------|---------|
| Library | `src/lib.rs` | Main crate exports |
| Tests | `tests/throttle_tests.rs` | Integration tests |

## Core Types

### Main Controller
- **`ThrottleController<M>`** - Main controller generic over motor
  - `new(motor)` → create controller
  - `apply_command(cmd, source, now_ms)` → apply command
  - `update(now_ms)` → tick update loop
  - `state(now_ms)` → get state snapshot

### Commands
- **`CommandSource`** - Priority enum: `Mqtt < WebApi < WebLocal < Physical < Fault < Emergency`
- **`ThrottleCommand<S>`** - Typed command with strategy
- **`ThrottleCommandDyn`** - Type-erased command for queuing
- **`PrioritizedCommand`** - Command + source + timestamp

### Strategies (ExecutionStrategy trait)
- **`Immediate`** - Instant change
- **`Linear`** - Constant rate interpolation
- **`EaseInOut`** - Smoothstep curve (`.departure()`, `.arrival()`)
- **`Momentum`** - Physics-based (`.gentle()`, `.responsive()`)

### Locks
- **`TransitionLock`** - `None | Source | Hard`
- **`InterruptBehavior`** - `Replace | Queue | Reject`

### Hardware Traits
- **`MotorController`** - `set_speed()`, `set_direction()`, `read_current_ma()`
- **`EncoderInput`** - `read_delta()`, `button_pressed()`
- **`FaultDetector`** - `is_short_circuit()`, `is_overcurrent()`

### Mock Implementations
- `MockMotor`, `MockEncoder`, `MockFault`, `MockClock`, `MockMqtt`, `MockHttp`

## Dependencies

| Dependency | Version | Purpose |
|------------|---------|---------|
| heapless | 0.8 | no_std fixed-capacity collections |
| embassy-* | 0.4-0.7 | ESP32 async runtime (optional) |

## Feature Flags

```toml
default = ["std"]
std = []
esp32 = ["dep:embassy-executor", "dep:embassy-time", "dep:embassy-sync"]
```

## Quick Commands

```bash
cargo build              # Build library
cargo test               # Run all tests
cargo test test_name     # Run single test
cargo check --no-default-features  # Verify no_std
cargo build --target riscv32imc-unknown-none-elf --features esp32  # ESP32 build
```

## Architecture Notes

1. **Command Flow**: `Source → Lockout Check → Transition Manager → Motor`
2. **Priority**: E-stop from any source auto-promotes to Emergency
3. **Lockout**: Physical control creates temporary lockout blocking remote commands
4. **Transitions**: Can be locked (Hard=only e-stop, Source=same+ priority)
5. **Type Erasure**: `ThrottleCommand<S>` converts to `ThrottleCommandDyn` via `.into()`
