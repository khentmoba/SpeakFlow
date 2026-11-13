// Prevents an extra console window from appearing in Windows release builds. Do not remove!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    speak_flow_lib::run()
}
