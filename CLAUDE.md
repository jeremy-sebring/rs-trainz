# CLAUDE.md - AI Assistant Guide for rs-trainz

This document provides context for AI assistants working on this codebase.

## Project Overview

**rs-trainz** is a Rust-based DC model train throttle controller designed for ESP32-C3 microcontrollers. The project supports multiple control sources (physical knob, web UI, MQTT) with a sophisticated priority system and smooth speed transitions.

### Key Characteristics

- **Dual-target architecture**: Runs on ESP32-C3 hardware and desktop (for testing)
- **`no_std` compatible core**: Core logic works without standard library
- **Feature-gated modules**: Network services (web, MQTT) are optional features
- **Hardware abstraction**: Trait-based design allows mock implementations for testing

## Project Structure

```
rs-trainz/
├── src/
│   ├── lib.rs              # Crate root, re-exports, documentation
│   ├── bin/
│   │   └── esp32_main.rs   # ESP32 binary entry point
│   ├── commands.rs         # ThrottleCommand types and priority system
│   ├── config.rs           # Configuration structs (heapless for no_std)
│   ├── priority.rs         # CommandQueue, SourceLockout
│   ├── strategy_dyn.rs     # Type-erased strategies for runtime polymorphism
│   ├── throttle.rs         # Main ThrottleController
│   ├── transition.rs       # TransitionManager with lock enforcement
│   ├── messages.rs         # HTTP/MQTT message types (serde-based)
│   ├── traits/
│   │   ├── mod.rs          # Trait re-exports
│   │   ├── hardware.rs     # MotorController, EncoderInput, FaultDetector, Clock
│   │   ├── network.rs      # MqttClient, HttpServer
│   │   ├── strategy.rs     # ExecutionStrategy (Immediate, Linear, EaseInOut, Momentum)
│   │   └── display.rs      # Display rendering trait
│   ├── hal/
│   │   ├── mod.rs          # HAL re-exports
│   │   ├── mock.rs         # Mock implementations for testing
│   │   └── esp32/          # ESP32-C3 hardware implementations
│   │       ├── mod.rs
│   │       ├── motor.rs    # BTS7960 H-bridge driver
│   │       ├── encoder.rs  # Rotary encoder input
│   │       ├── display.rs  # SSD1306 OLED
│   │       ├── wifi.rs     # WiFi connection
│   │       ├── http.rs     # HTTP server (esp-idf)
│   │       ├── mqtt.rs     # MQTT client (esp-idf)
│   │       ├── fault.rs    # Fault detection
│   │       └── clock.rs    # Hardware timer
│   └── services/           # Network services (feature-gated)
│       ├── mod.rs
│       ├── shared.rs       # SharedThrottleState for thread-safe access
│       ├── api.rs          # API types
│       ├── http_handler.rs # HTTP request handling logic
│       ├── web.rs          # Axum web server (desktop)
│       ├── mqtt.rs         # rumqttc MQTT client (desktop)
│       ├── mqtt_runner.rs  # Platform-agnostic MQTT runner
│       └── physical.rs     # Physical input handler
├── tests/
│   ├── throttle_tests.rs   # Integration tests
│   ├── edge_cases.rs       # Edge case tests
│   └── web_api_tests.rs    # Web API tests
├── examples/
│   └── desktop_server.rs   # Desktop server for testing web/MQTT
├── www/
│   └── index.html          # Web UI
├── Cargo.toml              # Crate configuration
├── Makefile                # Build automation
├── .cargo/config.toml      # ESP32 build configuration
└── sdkconfig.defaults      # ESP-IDF SDK configuration
```

## Build Commands

### Desktop Development (Testing)

```bash
# Build for desktop
cargo build

# Run all tests
cargo test

# Run tests with verbose output
cargo test -- --nocapture

# Run a specific test
cargo test test_name

# Run clippy linter
cargo clippy -- -D warnings

# Format code
cargo fmt

# Check formatting without changes
cargo fmt -- --check

# Run all CI checks (fmt, clippy, no_std, tests)
make ci

# Verify no_std compatibility
cargo check --no-default-features
```

### Desktop Server (for UI Testing)

```bash
# Web server only
cargo run --example desktop_server --features web

# Web + MQTT
cargo run --example desktop_server --features web,mqtt
```

### ESP32 Development

```bash
# Basic ESP32 build
make esp

# With display support
make esp-display

# With WiFi
make esp-wifi

# With HTTP web API
make esp-http

# With MQTT
make esp-mqtt

# HTTP + MQTT
make esp-net

# All features (display + http + mqtt)
make esp-full

# Flash to device
make flash

# Flash and monitor
make flash-monitor

# Open serial monitor
make monitor
```

### ESP32 Toolchain Setup

1. Install espup: `cargo install espup`
2. Install ESP toolchain: `espup install`
3. Source the export file: `. ~/export-esp.sh`
4. Install espflash: `cargo install espflash`

## Feature Flags

| Feature | Description |
|---------|-------------|
| `std` | Standard library (default, enables heap allocation) |
| `serde` | Serialization for config/messages |
| `serde-json-core` | JSON parsing (no_std compatible) |
| `web` | Axum web server (desktop) |
| `mqtt` | rumqttc MQTT client (desktop) |
| `esp32` | ESP32-C3 hardware support |
| `display` | SSD1306 OLED display |
| `wifi` | ESP32 WiFi support |
| `esp32-http` | ESP32 HTTP server |
| `esp32-mqtt` | ESP32 MQTT client |
| `esp32-net` | ESP32 HTTP + MQTT combined |

## Architecture Concepts

### Command Priority System

Commands flow through a priority system based on source:

| Source | Priority | Use Case |
|--------|----------|----------|
| `Mqtt` | Lowest | Home Assistant automation |
| `WebApi` | Low-Med | REST API calls |
| `WebLocal` | Medium | Local web UI |
| `Physical` | High | Rotary encoder/buttons |
| `Fault` | Higher | System-detected issues |
| `Emergency` | Highest | E-stop (auto-promoted) |

E-stop commands from any source are automatically promoted to `Emergency` priority.

### Transition Strategies

Speed changes use `ExecutionStrategy` implementations:

- **`Immediate`**: Instant change (used for e-stop)
- **`Linear`**: Constant rate over duration
- **`EaseInOut`**: Smooth start/end (smoothstep curve)
- **`Momentum`**: Physics-based acceleration feel

### Transition Locks

Transitions can be protected from interruption:

- **`TransitionLock::None`**: Can be interrupted by anything
- **`TransitionLock::Source`**: Only same/higher priority can interrupt
- **`TransitionLock::Hard`**: Only e-stop can interrupt

Use `EaseInOut::departure(ms)` for hard-locked station departures.
Use `EaseInOut::arrival(ms)` for source-locked arrivals that queue follow-up commands.

### Shared State Pattern

For web/MQTT services, use `SharedThrottleState<M>` wrapped in `Arc`:

```rust
let state = Arc::new(SharedThrottleState::new(controller));
// Clone for each service
let web_state = Arc::clone(&state);
let mqtt_state = Arc::clone(&state);
```

## Code Conventions

### Naming

- Types: `PascalCase`
- Functions/methods: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Feature flags: `kebab-case`

### Error Handling

- Use `Result<T, E>` for fallible operations
- Motor controller errors use associated `type Error`
- Network services use `anyhow::Result` for flexibility

### Documentation

- All public items should have doc comments
- Use `#![warn(missing_docs)]` (enabled in lib.rs)
- Include examples in doc comments for complex APIs

### Testing

- Unit tests go in `#[cfg(test)] mod tests` within each module
- Integration tests go in `tests/` directory
- Use `MockMotor`, `MockClock` etc. from `hal::mock` for testing

### Feature Gates

When adding feature-gated code:

```rust
#[cfg(feature = "web")]
pub mod web;

#[cfg(any(feature = "web", feature = "mqtt"))]
pub use shared::*;
```

### no_std Compatibility

The core library supports `no_std`:

- Use `heapless` collections instead of `Vec`/`HashMap`
- Avoid `std::time` in core code (use `Clock` trait)
- Feature-gate anything requiring `std`

## Common Tasks

### Adding a New Execution Strategy

1. Create a struct implementing `ExecutionStrategy` in `src/traits/strategy.rs`
2. Implement `interpolate()`, `duration_ms()`, and optionally `lock()`, `on_interrupt()`
3. Add tests in the same file
4. Re-export from `src/traits/mod.rs` and `src/lib.rs`

### Adding ESP32 Hardware Support

1. Create new module in `src/hal/esp32/`
2. Implement relevant trait(s) from `src/traits/hardware.rs`
3. Re-export from `src/hal/esp32/mod.rs`
4. Add any necessary feature flags to `Cargo.toml`

### Adding API Endpoints

1. Add handler in `src/services/http_handler.rs`
2. Wire up route in `src/services/web.rs` (desktop) or `src/hal/esp32/http.rs` (ESP32)
3. Add corresponding MQTT topic handler if applicable

### Configuration Changes

1. Modify structs in `src/config.rs`
2. Add builder methods for ergonomic construction
3. Update default values as needed
4. Add tests for new configuration options

## Hardware Setup

### Target Hardware

- **MCU**: ESP32-C3 SuperMini (~$6)
- **Motor Driver**: IBT-2 (BTS7960) H-bridge (~$10)
- **Power**: 12V 3A power supply
- **Input**: Rotary encoder with push button
- **Display**: 128x64 SSD1306 OLED (optional)

### Pin Assignments (ESP32-C3)

Defined in `src/hal/esp32/mod.rs` and individual hardware modules.

## API Reference

### REST Endpoints (Web Feature)

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/state` | Get current throttle state |
| POST | `/api/speed` | Set speed with optional transition |
| POST | `/api/direction` | Set direction (forward/reverse/stopped) |
| POST | `/api/estop` | Emergency stop |
| POST | `/api/max_speed` | Set maximum speed limit |
| GET | `/` | Web UI |

### MQTT Topics (MQTT Feature)

| Topic | Direction | Description |
|-------|-----------|-------------|
| `{prefix}/state` | Publish | Full state JSON |
| `{prefix}/speed` | Publish | Current speed |
| `{prefix}/direction` | Publish | Current direction |
| `{prefix}/speed/set` | Subscribe | Set speed |
| `{prefix}/direction/set` | Subscribe | Set direction |
| `{prefix}/estop` | Subscribe | Emergency stop |

Default prefix: `train`

## Troubleshooting

### ESP32 Build Fails

1. Ensure ESP toolchain is installed: `espup install`
2. Source the export file: `. ~/export-esp.sh`
3. First build may take a while as esp-idf-sys downloads toolchain

### Tests Fail

1. Run `cargo test` (not `make esp*` - tests don't run on ESP32)
2. Check for `no_std` compatibility if modifying core code
3. Ensure mock implementations match trait signatures

### MQTT Connection Issues

1. Check broker address in config
2. Verify network connectivity
3. Check if authentication is required

## Links

- ESP32-C3 Datasheet: https://www.espressif.com/en/products/socs/esp32-c3
- esp-rs Documentation: https://esp-rs.github.io/book/
- BTS7960 Motor Driver: Search for "IBT-2 BTS7960" module documentation
