//! Minimal driver for a single WS2812 ("NeoPixel") addressable RGB LED.
//!
//! The WS2812 is driven over one data wire with a precisely timed pulse train: every color
//! bit is a high pulse followed by a low pulse, and the pulse widths encode `0` or `1`.
//! The ESP32's RMT (Remote Control Transceiver) peripheral generates these pulses in
//! hardware, so the timing is unaffected by Wi-Fi interrupts or task scheduling.

use std::time::Duration;

use esp_idf_svc::hal::gpio::OutputPin;
use esp_idf_svc::hal::rmt::config::{TransmitConfig, TxChannelConfig};
use esp_idf_svc::hal::rmt::encoder::{BytesEncoder, BytesEncoderConfig};
use esp_idf_svc::hal::rmt::{PinState, Symbol, TxChannelDriver};
use esp_idf_svc::hal::units::Hertz;
use esp_idf_svc::sys::EspError;

/// RMT tick rate: 10 MHz, so one tick is 100 ns.
const RESOLUTION: Hertz = Hertz(10_000_000);

// Bit timings from the WS2812 datasheet (tolerance is about +/-150 ns).
const T0H: Duration = Duration::from_nanos(350);
const T0L: Duration = Duration::from_nanos(800);
const T1H: Duration = Duration::from_nanos(700);
const T1L: Duration = Duration::from_nanos(600);

/// A 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Returns this color with every channel multiplied by `factor` (clamped to `0.0..=1.0`).
    #[must_use]
    pub fn scaled(self, factor: f32) -> Self {
        let factor = factor.clamp(0.0, 1.0);
        let scale = |channel: u8| (f32::from(channel) * factor).round() as u8;
        Self::new(scale(self.r), scale(self.g), scale(self.b))
    }
}

/// A single WS2812 LED attached to one GPIO pin.
pub struct Ws2812<'d> {
    channel: TxChannelDriver<'d>,
    encoder: BytesEncoder,
    tx_config: TransmitConfig,
}

impl<'d> Ws2812<'d> {
    /// Claims an RMT transmit channel on `pin` and configures it for WS2812 timing.
    pub fn new(pin: impl OutputPin + 'd) -> Result<Self, EspError> {
        let channel = TxChannelDriver::new(
            pin,
            &TxChannelConfig {
                resolution: RESOLUTION,
                ..Default::default()
            },
        )?;

        // Turns each byte into 8 RMT symbols, most significant bit first.
        let encoder = BytesEncoder::with_config(&BytesEncoderConfig {
            bit0: Symbol::new_with(RESOLUTION, PinState::High, T0H, PinState::Low, T0L)?,
            bit1: Symbol::new_with(RESOLUTION, PinState::High, T1H, PinState::Low, T1L)?,
            msb_first: true,
            ..Default::default()
        })?;

        Ok(Self {
            channel,
            encoder,
            tx_config: TransmitConfig::default(),
        })
    }

    /// Sends `color` to the LED and blocks until the transmission completes (~30 us).
    ///
    /// The LED latches the new color once the data line has been idle for >50 us,
    /// so callers should not call this back-to-back without a short pause.
    pub fn set_color(&mut self, color: Rgb) -> Result<(), EspError> {
        // WS2812 expects green, red, blue byte order.
        let grb = [color.g, color.r, color.b];
        self.channel
            .send_and_wait(&mut self.encoder, &grb, &self.tx_config)
    }
}
