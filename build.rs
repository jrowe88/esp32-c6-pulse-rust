use std::path::Path;

fn main() {
    const CREDENTIALS: &str = "src/wifi_credentials.rs";

    println!("cargo:rerun-if-changed={CREDENTIALS}");
    if !Path::new(CREDENTIALS).exists() {
        panic!(
            "\n\n{CREDENTIALS} not found.\n\
             Copy src/wifi_credentials.example.rs to {CREDENTIALS} and enter your Wi-Fi details.\n\n"
        );
    }

    // Passes ESP-IDF build settings (linker args, cfgs) from esp-idf-sys to this crate.
    embuild::espidf::sysenv::output();
}
