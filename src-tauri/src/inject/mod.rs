/// The entire JS half of the app, compiled into the binary.
///
/// `include_str!` also registers a cargo rebuild dependency, so editing
/// `chat.js` is picked up by `tauri dev`.
pub const SCRIPT: &str = include_str!("chat.js");
