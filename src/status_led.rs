//! A "breathing" status indicator on the onboard RGB LED.
//!
//! The animation runs in its own thread (a FreeRTOS task under the hood), so it keeps
//! animating while other code blocks, for example while connecting to Wi-Fi. Other code
//! changes the pattern through [`StatusLed::set_status`], which stores into a shared atomic.

use std::f32::consts::PI;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::OutputPin;

use crate::ws2812::{Rgb, Ws2812};

/// Peak brightness, `0.0..=1.0`. The LED is very bright at full power.
const MAX_BRIGHTNESS: f32 = 0.75;

/// Bottom of the fade, as a fraction of [`MAX_BRIGHTNESS`].
///
/// Fading all the way to zero looks steppy at the dim end, because the lowest 8-bit
/// levels are large relative jumps (1 -> 2 doubles the brightness).
const MIN_BRIGHTNESS: f32 = 0.05;

/// Time between LED updates.
///
/// FreeRTOS delays round up to whole ticks (10 ms at the default `CONFIG_FREERTOS_HZ=100`),
/// so values below 10 ms still sleep for 10 ms.
const FRAME_INTERVAL: Duration = Duration::from_millis(10);

/// Stack size for the LED thread. The animation needs very little.
const TASK_STACK_SIZE: usize = 4096;

/// What the LED is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LedStatus {
    /// Quick blue pulse.
    Connecting = 0,
    /// Slow green breathe.
    Connected = 1,
    /// Fast red pulse.
    Failed = 2,
}

impl LedStatus {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Connected,
            2 => Self::Failed,
            _ => Self::Connecting,
        }
    }

    /// Base color and the length of one full breathe in/out cycle.
    const fn pattern(self) -> (Rgb, Duration) {
        match self {
            Self::Connecting => (Rgb::new(0, 80, 255), Duration::from_millis(1000)),
            Self::Connected => (Rgb::new(40, 255, 0), Duration::from_millis(3000)),
            Self::Failed => (Rgb::new(255, 0, 0), Duration::from_millis(500)),
        }
    }
}

/// Handle to the running LED animation thread.
pub struct StatusLed {
    status: Arc<AtomicU8>,
}

impl StatusLed {
    /// Spawns the animation thread, driving a WS2812 LED on `pin`.
    ///
    /// The thread owns the LED for the rest of the program; the returned handle is
    /// only used to change what it shows.
    pub fn start(pin: impl OutputPin + Send + 'static) -> anyhow::Result<Self> {
        let status = Arc::new(AtomicU8::new(LedStatus::Connecting as u8));
        let thread_status = Arc::clone(&status);

        thread::Builder::new()
            .name("status_led".into())
            .stack_size(TASK_STACK_SIZE)
            .spawn(move || {
                if let Err(e) = animate(pin, &thread_status) {
                    log::error!("Status LED stopped: {e:?}");
                }
            })?;

        Ok(Self { status })
    }

    pub fn set_status(&self, status: LedStatus) {
        self.status.store(status as u8, Ordering::Relaxed);
    }
}

/// Body of the LED thread: renders the current status's pattern forever.
fn animate(pin: impl OutputPin, status: &AtomicU8) -> anyhow::Result<()> {
    let mut led = Ws2812::new(pin)?;

    // Position within the current breathe cycle, `0.0..1.0`.
    let mut phase = 0.0_f32;
    let mut last_frame = Instant::now();

    loop {
        let (color, period) = LedStatus::from_u8(status.load(Ordering::Relaxed)).pattern();

        led.set_color(color.scaled(brightness_at(phase)))?;
        FreeRtos::delay_ms(FRAME_INTERVAL.as_millis() as u32);

        // Advance by the real elapsed time rather than FRAME_INTERVAL: the delay rounds up
        // to whole RTOS ticks, and sending takes time too. This keeps the animation speed
        // independent of the frame rate.
        let now = Instant::now();
        phase = (phase + (now - last_frame).as_secs_f32() / period.as_secs_f32()).fract();
        last_frame = now;
    }
}

/// Brightness for a point in the breathe cycle (`phase` in `0.0..1.0`).
fn brightness_at(phase: f32) -> f32 {
    // Raised cosine: a smooth 0 -> 1 -> 0 curve with gentle ease-in and ease-out.
    let level = (1.0 - (2.0 * PI * phase).cos()) / 2.0;
    // Squaring is a cheap gamma correction: the eye is more sensitive to changes at low
    // brightness, so this makes the fade look even.
    let perceived = level * level;

    (MIN_BRIGHTNESS + (1.0 - MIN_BRIGHTNESS) * perceived) * MAX_BRIGHTNESS
}
