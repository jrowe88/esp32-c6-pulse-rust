# ESP32-C6 Pulse (Rust)

A small, complete example of writing firmware for the **ESP32-C6** in **Rust**, using the
Rust standard library (`std`) on top of Espressif's ESP-IDF framework.

The firmware connects to Wi-Fi while the board's onboard RGB LED shows what's happening:

| LED | Meaning |
|---|---|
| Pulsing blue | Connecting to Wi-Fi |
| Slow green "breathing" | Connected |
| Fast red pulse | Connection failed (it retries automatically) |

It's meant as a starting point: small enough to read in one sitting, but structured like a
real project, with a hardware driver, a background task, Wi-Fi with automatic reconnection,
and CI.

> [!IMPORTANT]
> **This is _not_ bare-metal Rust.** There are two different ways to run Rust on Espressif
> chips, and tutorials, crates and examples for one generally don't work with the other:
>
> | | **This project: `std` on ESP-IDF** | Bare-metal (`no_std`) |
> |---|---|---|
> | Crates | `esp-idf-svc`, `esp-idf-hal`, `esp-idf-sys` | `esp-hal`, often with `embassy` |
> | Under the hood | Espressif's C SDK (ESP-IDF) and FreeRTOS | Pure Rust, no C SDK and no RTOS |
> | You get | `std::thread`, `std::net`, `std::time`, heap allocation, the full ESP-IDF feature set (Wi-Fi, BLE, HTTP, MQTT, OTA...) | Small binaries, fast builds, full control; you bring only what you need |
> | Telltale signs in code | `fn main()`, `esp_idf_svc::...` | `#![no_std]`, `#![no_main]`, `esp_hal::...`, `#[esp_hal::main]` |
>
> If you're following a tutorial or adding a crate, check which of the two it targets.
> Both are maintained by the [esp-rs](https://github.com/esp-rs) community.

## Why Rust instead of C or C++?

ESP-IDF itself is written in C, and most ESP32 code (including Arduino sketches) is C or C++.
Rust gives you access to the same SDK, with some real advantages:

- **Memory safety without a garbage collector.** Use-after-free, buffer overflows, and
  dangling pointers are rejected at compile time instead of showing up as crashes or
  corrupted data on the device.
- **Concurrency the compiler checks for you.** Sharing data between tasks unsafely
  (a data race) is a compile error. In this project the LED task and the main task share
  state through an `Arc<AtomicU8>`; forgetting the synchronization wouldn't compile.
- **Hardware ownership.** `Peripherals::take()` hands out each pin and peripheral exactly
  once. Passing `gpio8` to the LED driver *moves* it there, so no other code can
  accidentally drive that pin.
- **Errors you can't silently ignore.** Fallible calls return `Result`, and the `?`
  operator passes errors up cleanly, instead of return codes that are easy to forget or
  `ESP_ERROR_CHECK` macros that reboot the chip.
- **Modern tooling.** `cargo` handles dependencies, building, flashing, formatting
  (`cargo fmt`), and linting (`cargo clippy`) in one tool.

There are trade-offs too: the first build is slow (it compiles ESP-IDF from source), it
needs a nightly Rust toolchain, the learning curve is steeper than Arduino's, and there are
fewer ready-made libraries. When a Rust crate doesn't exist, you can still call any ESP-IDF
C function through `esp_idf_svc::sys`.

## Coming from Arduino?

Most concepts map directly:

| Arduino | This project |
|---|---|
| `setup()` | The start of `main()` |
| `loop()` | A `loop { ... }` at the end of `main()` (or in a thread) |
| `delay(ms)` | `std::thread::sleep(Duration::from_millis(ms))` |
| `Serial.println(...)` | `log::info!(...)` |
| `WiFi.begin(ssid, pass)` | `wifi::start(...)` and `wifi::connect(...)` in `src/wifi.rs` |
| Adafruit NeoPixel library | `src/ws2812.rs` |
| Library Manager | Add a crate to `[dependencies]` in `Cargo.toml` |
| "Upload" button | `cargo run` (build, flash, then open the serial monitor) |
| Serial Monitor | Opens automatically after `cargo run`, or run `espflash monitor` |

On ESP32s, Arduino itself runs on top of ESP-IDF and FreeRTOS, so the underlying platform
is the same; only the language and APIs differ.

## Hardware

- An **ESP32-C6 development board** with an onboard RGB LED on **GPIO8**, such as
  Espressif's ESP32-C6-DevKitC-1 or ESP32-C6-DevKitM-1. For other boards, change
  `peripherals.pins.gpio8` in `src/main.rs` to your LED's pin.
- A **USB-C data cable** (some cables only carry power).
- A **2.4 GHz Wi-Fi network**. The ESP32-C6 does not support 5 GHz.

The DevKit boards have two USB-C ports. Use the one labeled **UART**: it's the port ESP-IDF
logs to by default, and it stays connected while the chip resets.

## Getting started

### 1. Install Rust

Install Rust with [rustup](https://rustup.rs/).

- **Windows:** rustup will offer to install the Visual Studio C++ Build Tools. Accept;
  Rust needs them to build tools that run on your PC.
- **macOS / Linux:** run the `curl` command shown on the rustup site.

You do *not* need to install nightly Rust manually. The project's `rust-toolchain.toml`
pins the exact version, and rustup downloads it automatically the first time you build.

### 2. Install the build prerequisites

The ESP-IDF build needs **git**, **Python 3**, and **libclang** (used to generate Rust
bindings to the C SDK).

**Windows** (PowerShell):

```powershell
winget install Git.Git Python.Python.3.13 LLVM.LLVM
[Environment]::SetEnvironmentVariable('LIBCLANG_PATH', 'C:\Program Files\LLVM\bin', 'User')
```

Then open a new terminal so the new variable takes effect.

**macOS:**

```sh
xcode-select --install      # git and clang
brew install python3 llvm
```

**Debian / Ubuntu:**

```sh
sudo apt install git python3 python3-venv libclang-dev
sudo usermod -aG dialout $USER   # lets you access the USB serial port; log out and back in
```

### 3. Install the ESP tools

```sh
cargo install ldproxy espflash
```

- `ldproxy` lets Cargo use ESP-IDF's linker.
- `espflash` flashes the firmware and shows the serial output.

> [!NOTE]
> Many esp-rs guides also mention `espup`. That's only needed for the older **Xtensa**
> chips (ESP32, ESP32-S2, ESP32-S3). The ESP32-C6 is a RISC-V chip, which standard Rust
> nightly supports directly.

### 4. Get the code and add your Wi-Fi details

```sh
git clone https://github.com/jrowe88/esp32-c6-pulse-rust.git
cd esp32-c6-pulse-rust
```

Copy the credentials template and enter your network name and password:

```sh
cp src/wifi_credentials.example.rs src/wifi_credentials.rs     # macOS / Linux
copy src\wifi_credentials.example.rs src\wifi_credentials.rs   # Windows
```

`src/wifi_credentials.rs` is in `.gitignore`, so your password won't be committed.

### 5. Choose the build directory (macOS / Linux only)

On Windows, ESP-IDF fails if the build directory's path is too long, so
`.cargo/config.toml` builds into the short path `C:/t/pulse`. On **macOS and Linux**,
override that with the standard location:

```sh
export CARGO_TARGET_DIR=target    # add to ~/.bashrc or ~/.zshrc to make it permanent
```

Windows users don't need to do anything for this step.

### 6. Build, flash, and run

Plug in the board, then:

```sh
cargo run
```

This builds the firmware, flashes it, resets the board, and opens the serial monitor.
Press **Ctrl+R** to reset the board, or **Ctrl+C** to exit the monitor.

> [!NOTE]
> **The first build takes a while**, often 10 minutes or more. It downloads ESP-IDF and
> its toolchain (to `~/.espressif`) and compiles the whole SDK from source. Later builds
> only recompile your code and take seconds.

You should see something like:

```text
I (726) esp32_c6_pulse_rust: Connecting to Wi-Fi network 'my-network'...
I (4656) esp32_c6_pulse_rust: Wi-Fi connected, IP address 192.168.1.224
```

The LED pulses blue, then switches to a slow green breathe. Try `ping`ing the IP address
from your PC: the network stack answers automatically.

## Project layout

```text
.
├── .cargo/config.toml            # Target chip, build-std, flash runner, ESP-IDF version
├── rust-toolchain.toml           # Pinned nightly Rust toolchain
├── sdkconfig.defaults            # ESP-IDF settings (task stack sizes)
├── build.rs                      # ESP-IDF build integration; checks for Wi-Fi credentials
└── src
    ├── main.rs                   # Startup and the Wi-Fi supervisor loop
    ├── status_led.rs             # Breathing LED animation, running in its own thread
    ├── ws2812.rs                 # Minimal WS2812 RGB LED driver using the RMT peripheral
    ├── wifi.rs                   # Wi-Fi station setup and connection
    └── wifi_credentials.example.rs
```

## How it works

- **Two tasks.** `main()` starts the LED animation in its own thread, which on ESP-IDF is a
  FreeRTOS task, then connects to Wi-Fi. Because the LED has its own task, it keeps
  animating while the Wi-Fi calls block. The main task stays in a supervisor loop that
  checks the connection every 5 seconds and reconnects if it drops.
- **Talking to the LED.** The onboard LED is a WS2812 "smart" LED. Instead of PWM, it reads
  a single-wire serial signal where each bit is a high pulse followed by a low pulse, timed
  to within about 150 ns. The ESP32's **RMT** peripheral generates those pulses in hardware,
  so Wi-Fi activity can't disturb the timing. See `src/ws2812.rs`.
- **The breathing effect.** Brightness follows a raised-cosine curve with simple gamma
  correction, so the fade looks smooth to the eye. The animation advances by the actual
  elapsed time, so its speed doesn't depend on the frame rate. See `brightness_at()` in
  `src/status_led.rs`.

## Customizing

Most knobs are constants at the top of their files:

| What | Where |
|---|---|
| Colors and pulse speeds for each status | `LedStatus::pattern()` in `src/status_led.rs` |
| Peak and minimum brightness | `MAX_BRIGHTNESS`, `MIN_BRIGHTNESS` in `src/status_led.rs` |
| LED pin | `peripherals.pins.gpio8` in `src/main.rs` |
| Reconnect timing | `CONNECTION_CHECK_INTERVAL`, `RETRY_DELAY` in `src/main.rs` |
| WPA3-only networks | Change `AuthMethod::WPA2Personal` to `WPA3Personal` in `src/wifi.rs` |

### Using a library for the LED instead

`src/ws2812.rs` is written out by hand so you can see how the protocol works. For real
projects, especially LED strips, consider an existing crate:

- [`ws2812-esp32-rmt-driver`](https://crates.io/crates/ws2812-esp32-rmt-driver): an
  RMT-based driver for `std`/ESP-IDF projects like this one. It supports multi-LED strips,
  RGBW LEDs, and the [`smart-leds`](https://crates.io/crates/smart-leds) ecosystem of
  effects and color utilities.
- SK6812 LEDs use nearly the same protocol as the WS2812, and the RGB versions usually work
  unchanged. RGBW SK6812s send a fourth (white) byte per LED, which the crate above
  supports.

## Troubleshooting

| Problem | Fix |
|---|---|
| `Too long output directory` (Windows) | The build directory path is too long. Keep the project's `target-dir` setting, or move the project to a short path like `C:\pulse`. |
| `Unable to find libclang` | Install LLVM (step 2) and set `LIBCLANG_PATH` to the folder containing `libclang.dll` / `libclang.so`. Open a new terminal afterward. |
| `src/wifi_credentials.rs not found` | Copy the example file (step 4). |
| Errors inside `std`, such as `no field st_ctime` | A newer nightly broke `std` for ESP-IDF. Keep the nightly pinned in `rust-toolchain.toml`. |
| `Blocking waiting for file lock on build directory` | Another build is running, often rust-analyzer in your editor. It continues when that finishes. |
| `espflash` can't find or connect to the board | Use a data cable and the **UART** port. On Windows, check Device Manager for a missing USB-serial driver (CP210x or CH343). As a last resort, hold **BOOT**, tap **RESET**, release **BOOT**, and try again. |
| LED pulses red | Check the credentials, that the network is 2.4 GHz, and whether it's WPA3-only (see Customizing). The serial log shows the exact error. |

## Useful commands

```sh
cargo build                  # Build only (add --release for an optimized build)
cargo run                    # Build, flash, and open the serial monitor
espflash monitor             # Open the serial monitor without flashing
espflash board-info          # Check the board is connected: chip, flash size, MAC address
espflash erase-flash         # Erase the whole chip, including saved Wi-Fi data
cargo fmt                    # Format the code
cargo clippy                 # Lint the code
```

## Learn more

- [The Rust on ESP Book](https://docs.esp-rs.org/book/): the official esp-rs guide, covering
  both the `std` and bare-metal approaches
- [esp-rs/esp-idf](https://github.com/esp-rs/esp-idf): source and examples for
  `esp-idf-svc`, `esp-idf-hal`, and `esp-idf-sys`
- [esp-idf-template](https://github.com/esp-rs/esp-idf-template): the `cargo generate`
  template this project started from
- [The Rust Book](https://doc.rust-lang.org/book/): the standard introduction to Rust itself

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
