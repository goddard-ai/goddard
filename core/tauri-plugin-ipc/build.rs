const COMMANDS: &[&str] = &["send", "subscribe", "unsubscribe"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./api-iife.js")
        .try_build()
        .unwrap();
}
