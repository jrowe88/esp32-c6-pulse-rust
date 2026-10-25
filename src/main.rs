//! ESP32-C6 example: Rust `std` on ESP-IDF.
//!
//! Connects to Wi-Fi while the onboard RGB LED shows the connection status:
//!
//! - Pulsing blue: connecting
//! - Breathing green: connected
//! - Fast red: connection failed (it retries automatically)
//!
//! The LED animation runs in its own thread (see [`status_led`]); the main thread
//! connects to Wi-Fi and then supervises the connection, reconnecting if it drops.

mod status_led;
mod wifi;
mod wifi_credentials;
mod ws2812;

use std::thread;
use std::time::Duration;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

use status_led::{LedStatus, StatusLed};
use wifi_credentials::{WIFI_PASSWORD, WIFI_SSID};

/// How often to check that Wi-Fi is still connected.
const CONNECTION_CHECK_INTERVAL: Duration = Duration::from_secs(5);
/// How long to wait after a failed attempt before trying again.
const RETRY_DELAY: Duration = Duration::from_secs(10);

fn main() -> anyhow::Result<()> {
    // Applies runtime patches from esp-idf-sys. Must be called once, before anything else.
    esp_idf_svc::sys::link_patches();

    // Routes the `log` crate's macros to the ESP-IDF serial console.
    esp_idf_svc::log::EspLogger::initialize_default();
    wifi::quiet_driver_logs();

    let peripherals = Peripherals::take()?;

    // Start the LED first so it animates while Wi-Fi setup blocks below.
    // GPIO8 is the onboard RGB LED on Espressif's ESP32-C6 dev kits.
    let led = StatusLed::start(peripherals.pins.gpio8)?;

    let mut wifi = wifi::start(
        peripherals.modem,
        EspSystemEventLoop::take()?,
        EspDefaultNvsPartition::take()?,
        WIFI_SSID,
        WIFI_PASSWORD,
    )?;

    // Supervisor loop. It also keeps `wifi` alive: returning from main would drop it.
    loop {
        if !wifi.is_connected()? {
            led.set_status(LedStatus::Connecting);
            log::info!("Connecting to Wi-Fi network '{WIFI_SSID}'...");

            match wifi::connect(&mut wifi) {
                Ok(ip) => {
                    log::info!("Wi-Fi connected, IP address {ip}");
                    led.set_status(LedStatus::Connected);
                }
                Err(e) => {
                    log::warn!("Wi-Fi connection failed: {e:?}. Retrying in {RETRY_DELAY:?}");
                    led.set_status(LedStatus::Failed);
                    thread::sleep(RETRY_DELAY);
                    continue;
                }
            }
        }

        thread::sleep(CONNECTION_CHECK_INTERVAL);
    }
}
