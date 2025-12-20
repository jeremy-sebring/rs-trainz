//! Desktop server example for testing the web UI and MQTT integration.
//!
//! This example runs a web server with mock motor hardware, allowing you to:
//! - Access the web UI at http://localhost:8080
//! - Test all API endpoints
//! - Optionally connect to an MQTT broker
//!
//! # Shared State
//!
//! When both web and MQTT features are enabled, they share a single
//! `ThrottleController` via `SharedThrottleState`. Commands from either
//! source are immediately visible to the other, enabling real-time
//! model train control from multiple interfaces.
//!
//! # Usage
//!
//! Web server only:
//! ```sh
//! cargo run --example desktop_server --features web
//! ```
//!
//! Web server + MQTT:
//! ```sh
//! cargo run --example desktop_server --features web,mqtt
//! ```
//!
//! # Configuration
//!
//! Edit the `Config::default()` call in `main()` to customize settings.
//! See the commented example for how to use the builder pattern.

use rs_trainz::hal::MockMotor;
use rs_trainz::{Config, ThrottleController};

#[cfg(any(feature = "web", feature = "mqtt"))]
use std::sync::Arc;

#[cfg(any(feature = "web", feature = "mqtt"))]
use std::time::Duration;

#[cfg(any(feature = "web", feature = "mqtt"))]
use rs_trainz::services::SharedThrottleState;

#[cfg(feature = "web")]
use rs_trainz::services::WebServerConfig;

#[cfg(feature = "mqtt")]
use rs_trainz::services::{MqttHandler, MqttRuntimeConfig};

fn main() {
    // Initialize the tokio runtime
    let rt = tokio::runtime::Runtime::new().expect("Failed to create runtime");

    rt.block_on(async {
        println!("=================================");
        println!("  rs-trainz Desktop Server");
        println!("=================================");
        println!();

        // Central configuration - modify this for your setup
        let config = Config::default();
        // Example of customization:
        // let config = Config::default()
        //     .with_mqtt(rs_trainz::MqttConfig::default()
        //         .with_host("192.168.1.100")
        //         .with_topic_prefix("trains/loco1"))
        //     .with_web(rs_trainz::WebConfig::default()
        //         .with_port(3000))
        //     .with_device(rs_trainz::DeviceConfig::default()
        //         .with_name("My Train")
        //         .with_id("loco1"));

        // Create mock motor and controller
        let motor = MockMotor::new();
        let controller = ThrottleController::new(motor);

        #[cfg(all(feature = "web", feature = "mqtt"))]
        {
            run_web_and_mqtt(controller, &config).await;
        }

        #[cfg(all(feature = "web", not(feature = "mqtt")))]
        {
            run_web_only(controller, &config).await;
        }

        #[cfg(all(feature = "mqtt", not(feature = "web")))]
        {
            run_mqtt_only(controller, &config).await;
        }

        #[cfg(not(any(feature = "web", feature = "mqtt")))]
        {
            let _ = controller; // suppress unused warning
            let _ = config; // suppress unused warning
            eprintln!("No features enabled. Run with --features web or --features mqtt");
            std::process::exit(1);
        }
    });
}

#[cfg(all(feature = "web", not(feature = "mqtt")))]
async fn run_web_only(controller: ThrottleController<MockMotor>, config: &Config) {
    let web_config = WebServerConfig::from_config(&config.web);

    println!("Starting web server...");
    println!("  Web UI: http://{}", web_config.addr);
    println!("  API:    http://{}/api/state", web_config.addr);
    println!();
    println!("Press Ctrl+C to stop.");
    println!();

    // Create shared state
    let state = Arc::new(SharedThrottleState::new(controller));

    // Spawn controller update task
    spawn_update_loop(Arc::clone(&state));

    // Run web server
    let router = rs_trainz::services::build_router(state, &web_config);
    let listener = tokio::net::TcpListener::bind(web_config.addr)
        .await
        .unwrap();
    axum::serve(listener, router).await.unwrap();
}

#[cfg(all(feature = "mqtt", not(feature = "web")))]
async fn run_mqtt_only(controller: ThrottleController<MockMotor>, config: &Config) {
    let mqtt_config = MqttRuntimeConfig::from_config(&config.mqtt);

    println!("Starting MQTT client...");
    println!("  Broker: {}:{}", mqtt_config.host, mqtt_config.port);
    println!();
    println!("Topics:");
    println!(
        "  Subscribe: {}/speed/set, {}/direction/set, {}/estop",
        mqtt_config.topic_prefix, mqtt_config.topic_prefix, mqtt_config.topic_prefix
    );
    println!(
        "  Publish:   {}/state, {}/speed, {}/direction",
        mqtt_config.topic_prefix, mqtt_config.topic_prefix, mqtt_config.topic_prefix
    );
    println!();
    println!("Press Ctrl+C to stop.");
    println!();

    // Create shared state
    let state = Arc::new(SharedThrottleState::new(controller));

    // Spawn controller update task
    spawn_update_loop(Arc::clone(&state));

    // Create MQTT handler with shared state
    let handler = MqttHandler::with_shared_state(state, mqtt_config);

    if let Err(e) = handler.run().await {
        eprintln!("MQTT error: {}", e);
    }
}

#[cfg(all(feature = "web", feature = "mqtt"))]
async fn run_web_and_mqtt(controller: ThrottleController<MockMotor>, config: &Config) {
    let web_config = WebServerConfig::from_config(&config.web);
    let mqtt_config = MqttRuntimeConfig::from_config(&config.mqtt);

    println!("Starting web server and MQTT client with SHARED STATE...");
    println!();
    println!("Web:");
    println!("  Web UI: http://{}", web_config.addr);
    println!("  API:    http://{}/api/state", web_config.addr);
    println!();
    println!("MQTT:");
    println!("  Broker: {}:{}", mqtt_config.host, mqtt_config.port);
    println!(
        "  Topics: {}/speed/set, {}/direction/set, {}/estop",
        mqtt_config.topic_prefix, mqtt_config.topic_prefix, mqtt_config.topic_prefix
    );
    println!();
    println!("NOTE: Web and MQTT share the same controller state.");
    println!("      Commands from either source are immediately visible to both!");
    println!();
    println!("Press Ctrl+C to stop.");
    println!();

    // =========================================================================
    // SINGLE shared state for both web and MQTT
    // =========================================================================
    let shared_state = Arc::new(SharedThrottleState::new(controller));

    // =========================================================================
    // SINGLE update loop for both services
    // =========================================================================
    spawn_update_loop(Arc::clone(&shared_state));

    // =========================================================================
    // Web server with shared state
    // =========================================================================
    let state_for_web = Arc::clone(&shared_state);
    let web_config_clone = web_config.clone();
    tokio::spawn(async move {
        let router = rs_trainz::services::build_router(state_for_web, &web_config_clone);
        let listener = tokio::net::TcpListener::bind(web_config_clone.addr)
            .await
            .unwrap();
        axum::serve(listener, router).await.unwrap();
    });

    // =========================================================================
    // MQTT handler with same shared state
    // =========================================================================
    let handler = MqttHandler::with_shared_state(shared_state, mqtt_config);

    // Run MQTT handler (blocks)
    if let Err(e) = handler.run().await {
        eprintln!("MQTT error: {}", e);
    }
}

/// Spawn the single controller update loop.
///
/// This task runs every 20ms and:
/// - Progresses any active speed transitions
/// - Updates the motor with the current speed
#[cfg(any(feature = "web", feature = "mqtt"))]
fn spawn_update_loop(state: Arc<SharedThrottleState<MockMotor>>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(20));
        loop {
            interval.tick().await;
            let now_ms = state.now_ms();
            state.with_controller(|controller| {
                let _ = controller.update(now_ms);
            });
        }
    });
}
