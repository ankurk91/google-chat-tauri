/// The entire JS half of the app, compiled into the binary.
///
/// `include_str!` also registers a cargo rebuild dependency, so editing
/// `chat.js` is picked up by `tauri dev`.
pub const SCRIPT: &str = include_str!("chat.js");

#[cfg(test)]
mod tests {
    use super::SCRIPT;

    /// The capability that decides which origins the IPC will answer on.
    const CAPABILITY: &str = include_str!("../../capabilities/remote-chat.json");

    /// The origins `chat.js` names in its own `CHAT_ORIGINS`.
    fn script_origins() -> Vec<String> {
        let (_, rest) = SCRIPT
            .split_once("const CHAT_ORIGINS = [")
            .expect("chat.js no longer declares CHAT_ORIGINS");
        let (list, _) = rest
            .split_once(']')
            .expect("CHAT_ORIGINS is no longer an array literal");

        // Every second piece of a split on the quote character is a quoted
        // string; the rest is the punctuation between them.
        list.split('\'')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    }

    /// `chat.js` decides how much of itself to install from `location.origin`,
    /// and the ACL decides whether to answer from the same thing. They are one
    /// set spelled in two files, so let them drift and either the script boots
    /// the whole bridge onto a page that will refuse every call, or -- far
    /// harder to notice -- it stays quiet on a page where it would have worked.
    #[test]
    fn the_injected_script_and_the_capability_agree_on_where_chat_is() {
        let capability: serde_json::Value =
            serde_json::from_str(CAPABILITY).expect("remote-chat.json is not valid JSON");

        let allowed: Vec<String> = capability["remote"]["urls"]
            .as_array()
            .expect("remote.urls is not an array")
            .iter()
            .map(|url| {
                let url = url.as_str().expect("remote.urls holds a non-string");
                // The capability matches paths as well; an origin has none.
                url.trim_end_matches("/*").to_string()
            })
            .collect();

        assert_eq!(
            script_origins(),
            allowed,
            "chat.js and remote-chat.json disagree about which origins are Chat"
        );
    }
}
