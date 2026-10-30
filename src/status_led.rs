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
use ledkit_core::prelude::*;

use ledkit_esp_idf::RmtOutput;

/// The type of LED on the board. The DevKits' onboard LED is a WS2812B-style NeoPixel.
type LedType = Ws2812b;

/// How many LEDs are chained on the pin: the DevKit has one onboard.
const LED_COUNT: usize = 1;

/// Peak brightness, `0.0..=1.0`. The LED is very bright at full power.
///
/// ledkit's brightness is perceptual: `0.5` looks about half as bright as `1.0`.
const MAX_BRIGHTNESS: f32 = 0.75;

/// Bottom of the fade, as a fraction of [`MAX_BRIGHTNESS`].
///
/// Keeps the LED from switching fully off at the bottom of each breath, so the fade
/// looks continuous. Because brightness is perceptual, low values fall off fast: below
/// about 4% the LED receives level 0 and goes dark. 0.3 of 0.75 lands at about level 9.
const MIN_BRIGHTNESS: f32 = 0.3;

/// Time between LED updates.
///
/// FreeRTOS delays round up to whole ticks (10 ms at the default `CONFIG_FREERTOS_HZ=100`),
/// so values below 10 ms still sleep for 10 ms.
const FRAME_INTERVAL: Duration = Duration::from_millis(10);

/// Stack size for the LED thread, set explicitly so it doesn't depend on
/// `CONFIG_PTHREAD_TASK_STACK_SIZE_DEFAULT`. The animation itself needs very little;
/// most of this is headroom for log formatting.
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
    /// Steady brightness, cycling through the color wheel. Handy for testing the LED.
    Rainbow = 3,
}

/// How a status is drawn.
enum Pattern {
    /// Fades `color` in and out once per `period`.
    Breathe { color: Rgb, period: Duration },
    /// Steps through every hue once per `period`, at constant brightness.
    Rainbow { period: Duration },
}

impl Pattern {
    const fn period(&self) -> Duration {
        match self {
            Self::Breathe { period, .. } | Self::Rainbow { period } => *period,
        }
    }

    /// The color and brightness to show at `phase` (`0.0..1.0`) through the pattern's cycle.
    fn frame_at(&self, phase: f32) -> (Rgb, f32) {
        match self {
            Self::Breathe { color, .. } => (*color, breathe_brightness(phase)),
            Self::Rainbow { .. } => (Rgb::from_hue(Hue::from_fraction(phase)), MAX_BRIGHTNESS),
        }
    }
}

impl LedStatus {
    const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Connected,
            2 => Self::Failed,
            3 => Self::Rainbow,
            _ => Self::Connecting,
        }
    }

    const fn pattern(self) -> Pattern {
        match self {
            Self::Connecting => Pattern::Breathe {
                color: Rgb::new(0, 80, 255),
                period: Duration::from_millis(1000),
            },
            Self::Connected => Pattern::Breathe {
                color: Rgb::new(40, 255, 0),
                period: Duration::from_millis(3000),
            },
            Self::Failed => Pattern::Breathe {
                color: Rgb::new(255, 0, 0),
                period: Duration::from_millis(500),
            },
            Self::Rainbow => Pattern::Rainbow {
                period: Duration::from_millis(6000),
            },
        }
    }
}

/// Handle to the running LED animation thread.
pub struct StatusLed {
    status: Arc<AtomicU8>,
}

impl StatusLed {
    /// Spawns the animation thread, driving the LED on `pin`.
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
                    log::error!("Status LED could not start: {e:?}");
                }
            })?;

        Ok(Self { status })
    }

    pub fn set_status(&self, status: LedStatus) {
        self.status.store(status as u8, Ordering::Relaxed);
    }
}

/// Body of the LED thread: renders the current status's pattern forever.
///
/// Only returns if the LED can't be set up. Errors while sending a frame are logged and
/// the animation carries on, so one glitch doesn't freeze the status display.
fn animate(pin: impl OutputPin, status: &AtomicU8) -> anyhow::Result<()> {
    // The strip holds the pixel colors; `show()` applies brightness (with dithering, which
    // smooths the dim end of the fade) and sends the frame through the RMT.
    let mut strip = Strip::<LedType, LED_COUNT, _>::new(RmtOutput::new(pin)?);

    // Position within the current pattern's cycle, `0.0..1.0`.
    let mut phase = 0.0_f32;
    let mut last_frame = Instant::now();
    // Logs only the first error of a run of failures, instead of 100 per second.
    let mut send_failing = false;

    loop {
        let pattern = LedStatus::from_u8(status.load(Ordering::Relaxed)).pattern();

        let (color, brightness) = pattern.frame_at(phase);
        strip.fill(color);
        strip.set_brightness(brightness);

        match strip.show() {
            Ok(()) => send_failing = false,
            Err(e) if !send_failing => {
                log::warn!("Status LED update failed: {e}");
                send_failing = true;
            }
            Err(_) => {}
        }
        FreeRtos::delay_ms(FRAME_INTERVAL.as_millis() as u32);

        // Advance by the real elapsed time rather than FRAME_INTERVAL: the delay rounds up
        // to whole RTOS ticks, and sending takes time too. This keeps the animation speed
        // independent of the frame rate.
        let now = Instant::now();
        let elapsed = (now - last_frame).as_secs_f32();
        phase = (phase + elapsed / pattern.period().as_secs_f32()).fract();
        last_frame = now;
    }
}

/// Brightness for a point in the breathe cycle (`phase` in `0.0..1.0`).
fn breathe_brightness(phase: f32) -> f32 {
    // Raised cosine: a smooth 0 -> 1 -> 0 curve with gentle ease-in and ease-out.
    // No gamma correction needed here: ledkit's brightness is already perceptual.
    let level = (1.0 - (2.0 * PI * phase).cos()) / 2.0;

    (MIN_BRIGHTNESS + (1.0 - MIN_BRIGHTNESS) * level) * MAX_BRIGHTNESS
}
