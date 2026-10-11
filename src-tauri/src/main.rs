#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    // Orbit is also the reviewer's dev container tool (see `boxtool`).
    if orbit_lib::boxtool::serve_if_asked() {
        return;
    }
    orbit_lib::run();
}
