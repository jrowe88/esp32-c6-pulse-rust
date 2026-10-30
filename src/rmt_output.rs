//! Sends ledkit frames to NeoPixel-style LEDs (WS2812 and friends) with the RMT peripheral.
//!
//! These "clockless" LEDs are driven over one data wire with a precisely timed pulse
//! train: every color bit is a high pulse followed by a low pulse, and the pulse widths
//! encode `0` or `1`. The ESP32's RMT (Remote Control Transceiver) peripheral generates
//! these pulses in hardware, so the timing is unaffected by Wi-Fi interrupts or task
//! scheduling.
//!
//! [ledkit](https://github.com/jrowe88/ledkit) supplies the rest: the pulse lengths for
//! each chip, the byte order it expects, and the brightness and color processing done by
//! [`Strip`](ledkit_core::Strip). This file is only the bridge from ledkit to the RMT.

use std::marker::PhantomData;

use esp_idf_svc::hal::gpio::OutputPin;
use esp_idf_svc::hal::rmt::config::{TransmitConfig, TxChannelConfig};
use esp_idf_svc::hal::rmt::encoder::{BytesEncoder, BytesEncoderConfig};
use esp_idf_svc::hal::rmt::{PinState, Pulse, PulseTicks, Symbol, TxChannelDriver};
use esp_idf_svc::hal::units::Hertz;
use esp_idf_svc::sys::EspError;
use ledkit_core::{wire, Chip, ClocklessTiming, Output};

/// RMT tick rate: 10 MHz, so one tick is 100 ns.
const RESOLUTION: Hertz = Hertz(10_000_000);

/// The longest single RMT pulse, in ticks.
const MAX_TICKS: u32 = 32_767;

/// A chain of clockless LEDs of type `C` (for example [`ledkit_core::Ws2812b`]) on one
/// GPIO pin.
///
/// Hand it to [`Strip::new`](ledkit_core::Strip::new); the strip calls
/// [`Output::write`] for you on every `show()`.
pub struct RmtOutput<'d, C: Chip> {
    channel: TxChannelDriver<'d>,
    encoder: BytesEncoder,
    tx_config: TransmitConfig,
    /// The frame as bytes, reused between frames so `show()` doesn't allocate.
    bytes: Vec<u8>,
    chip: PhantomData<C>,
}

impl<'d, C: Chip> RmtOutput<'d, C> {
    /// Claims an RMT transmit channel on `pin` and configures it for `C`'s timing.
    pub fn new(pin: impl OutputPin + 'd) -> anyhow::Result<Self> {
        // Evaluated at compile time: a clocked chip (a DotStar, which needs a clock wire)
        // can't be driven this way, and using one here fails the build.
        let timing = const { ClocklessTiming::of::<C>() };
        // ledkit checks each pulse against the chip's datasheet window at this tick rate.
        let ticks = timing.pulse_ticks(RESOLUTION.0, MAX_TICKS)?;

        let channel = TxChannelDriver::new(
            pin,
            &TxChannelConfig {
                resolution: RESOLUTION,
                ..Default::default()
            },
        )?;

        // Turns each byte into 8 RMT symbols, most significant bit first.
        let encoder = BytesEncoder::with_config(&BytesEncoderConfig {
            bit0: symbol(ticks.zero_high, ticks.zero_low)?,
            bit1: symbol(ticks.one_high, ticks.one_low)?,
            msb_first: true,
            ..Default::default()
        })?;

        Ok(Self {
            channel,
            encoder,
            tx_config: TransmitConfig::default(),
            bytes: Vec::new(),
            chip: PhantomData,
        })
    }
}

impl<C: Chip> Output<C> for RmtOutput<'_, C> {
    type Error = EspError;

    /// Sends one frame and blocks until the transmission completes (~30 us per LED).
    ///
    /// The LEDs latch the new colors once the data line has been idle for the chip's reset
    /// time (280 us for the WS2812B), so frames must be at least that far apart.
    fn write<I>(&mut self, pixels: I) -> Result<(), EspError>
    where
        I: Iterator<Item = C::Pixel> + ExactSizeIterator,
    {
        // ledkit puts each pixel's channels in the chip's order (green, red, blue for the
        // WS2812B).
        self.bytes.clear();
        self.bytes.extend(wire::clockless_bytes::<C, _>(pixels));

        self.channel
            .send_and_wait(&mut self.encoder, &self.bytes, &self.tx_config)
    }
}

/// One bit's pulse: high for `high` ticks, then low for `low` ticks.
fn symbol(high: u32, low: u32) -> Result<Symbol, EspError> {
    // `pulse_ticks` was given MAX_TICKS, so these always fit in a u16.
    let pulse = |state, ticks: u32| -> Result<Pulse, EspError> {
        Ok(Pulse::new(state, PulseTicks::new(ticks as u16)?))
    };
    Ok(Symbol::new(
        pulse(PinState::High, high)?,
        pulse(PinState::Low, low)?,
    ))
}
