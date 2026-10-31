//! Orbit without a window (see `orbit_lib::server`).
fn main() {
    // Orbit is also the reviewer's dev container tool (see `boxtool`), which
    // runs this same program again.
    if orbit_lib::boxtool::serve_if_asked() {
        return;
    }
    std::process::exit(orbit_lib::server::main(
        std::env::args_os().skip(1).collect(),
    ));
}
