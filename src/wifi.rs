//! Wi-Fi station (client) setup.

use std::net::Ipv4Addr;

use anyhow::anyhow;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};

pub type Wifi = BlockingWifi<EspWifi<'static>>;

/// Creates the Wi-Fi driver in station mode and starts the radio, without connecting yet.
///
/// The returned driver must be kept alive for as long as Wi-Fi is needed:
/// dropping it shuts Wi-Fi down.
pub fn start(
    modem: Modem<'static>,
    sys_loop: EspSystemEventLoop,
    nvs: EspDefaultNvsPartition,
    ssid: &str,
    password: &str,
) -> anyhow::Result<Wifi> {
    validate_password(password)?;

    // NVS lets the driver cache RF calibration data, which speeds up later boots.
    let mut wifi = BlockingWifi::wrap(EspWifi::new(modem, sys_loop.clone(), Some(nvs))?, sys_loop)?;

    wifi.set_configuration(&Configuration::Client(ClientConfiguration {
        ssid: ssid
            .try_into()
            .map_err(|_| anyhow!("Wi-Fi SSID is longer than 32 bytes"))?,
        password: password
            .try_into()
            .map_err(|_| anyhow!("Wi-Fi password is too long"))?,
        // The minimum accepted security. With a password, WPA2/WPA3 mixed-mode networks
        // also work; an empty password means an open network.
        auth_method: if password.is_empty() {
            AuthMethod::None
        } else {
            AuthMethod::WPA2Personal
        },
        ..Default::default()
    }))?;
    wifi.start()?;

    Ok(wifi)
}

/// Connects to the configured network and blocks until DHCP assigns an address.
///
/// Gives up after about 15 seconds each for joining the network and for getting an address.
pub fn connect(wifi: &mut Wifi) -> anyhow::Result<Ipv4Addr> {
    // Start from a clean state: a previous attempt may have timed out while the driver was
    // still connecting, or the connection may have lost its IP address while staying
    // associated. The error is ignored because there may be nothing to disconnect.
    let _ = wifi.disconnect();

    wifi.connect()?;
    wifi.wait_netif_up()?;

    Ok(wifi.wifi().sta_netif().get_ip_info()?.ip)
}

/// Rejects passwords the Wi-Fi driver would accept but that can never authenticate.
fn validate_password(password: &str) -> anyhow::Result<()> {
    let valid = match password.len() {
        0 => true,                                             // Open network
        8..=63 => true,                                        // WPA2/WPA3 passphrase
        64 => password.chars().all(|c| c.is_ascii_hexdigit()), // Raw hex key
        _ => false,
    };
    anyhow::ensure!(
        valid,
        "Wi-Fi password must be 8-63 characters, a 64-digit hex key, or empty for an open network"
    );

    Ok(())
}

/// Limits the Wi-Fi driver's own logging to errors.
///
/// The driver prints many radio and PHY diagnostics at info and warning level; the
/// application's log lines already report what matters.
pub fn quiet_driver_logs() {
    use esp_idf_svc::sys::{esp_log_level_set, esp_log_level_t_ESP_LOG_ERROR};

    let tags = [
        c"wifi",
        c"wifi_init",
        c"phy_init",
        c"pp",
        c"net80211",
        c"esp_netif_handlers",
    ];
    for tag in tags {
        // SAFETY: `tag` is a valid, NUL-terminated string with static lifetime.
        unsafe { esp_log_level_set(tag.as_ptr(), esp_log_level_t_ESP_LOG_ERROR) };
    }
}
