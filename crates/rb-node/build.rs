//! Links the addon the way Node loads it (on macOS, symbols resolved at load time).
fn main() {
    napi_build::setup();
}
