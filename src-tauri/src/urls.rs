//! Ported from electron `src/urls.ts` and the allow-lists in
//! `src/main/features/externalLinks.ts`.

pub const APP_URL: &str = "https://mail.google.com/chat/u/0";

/// Used by the "Sign Out" menu item (P1).
#[allow(dead_code)]
pub fn logout_url() -> String {
    format!("https://www.google.com/accounts/Logout?continue={APP_URL}")
}

/// Hosts a *new window / target=_blank* request may open in-app. Strict, and
/// carried over verbatim from the electron app.
const POPUP_HOSTS: [&str; 4] = [
    "accounts.google.com",
    "accounts.youtube.com",
    "chat.google.com",
    "mail.google.com",
];

/// Attachment downloads. The electron app handed these to the system browser.
const ATTACHMENT_URL: &str = "https://chat.google.com/u/0/api/get_attachment_url";

/// Any Google-owned host, *including country ccTLDs*.
///
/// The ccTLD part is not hypothetical: signing in round-trips through
/// `accounts.google.co.in/accounts/SetSID` (or the local equivalent) to set
/// cookies. A `*.google.com`-only list punts that to the system browser, which
/// then follows the `continue=` parameter and finishes the login *there*,
/// leaving the app sitting on the sign-in page.
pub fn is_google_host(host: &str) -> bool {
    // "accounts.google.co.in" -> ["accounts", "google", "co", "in"]
    let labels: Vec<&str> = host.split('.').collect();

    // Use the *last* "google" so a prefix like "google.attacker.net" cannot
    // shadow the real registrable domain.
    let Some(i) = labels.iter().rposition(|l| *l == "google") else {
        return false;
    };

    // Everything after "google" must look like a public suffix: one or two
    // short alphabetic labels ("com", "co.in", "com.au", "de"). Without a real
    // public-suffix list this is the discriminator that keeps "google.evil.com"
    // out while letting every ccTLD Google actually signs in through pass.
    let suffix = &labels[i + 1..];
    matches!(suffix.len(), 1 | 2)
        && suffix
            .iter()
            .all(|l| (1..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Should this new-window request be handed to the system browser instead of
/// being opened in the app?
pub fn should_open_externally(url: &url::Url, current_host: &str) -> bool {
    let Some(host) = url.host_str() else {
        return true;
    };

    // Gmail proper -- only /chat belongs to us.
    if host == "mail.google.com" && !url.as_str().starts_with("https://mail.google.com/chat") {
        return true;
    }

    if url.as_str().contains(ATTACHMENT_URL) {
        return true;
    }

    // Google's own hosts (any ccTLD) stay in-app: these are auth hops, not
    // links the user asked to open elsewhere.
    !(POPUP_HOSTS.contains(&host) || host == current_host || is_google_host(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_google_hosts_including_cctlds() {
        for host in [
            "google.com",
            "mail.google.com",
            "accounts.google.com",
            "accounts.google.co.in",
            "google.co.uk",
            "google.de",
            "google.com.au",
        ] {
            assert!(is_google_host(host), "{host} should be a Google host");
        }

        for host in [
            "google.evil.com",
            "google.com.attacker.net",
            "www.google.co.uk.evil.com",
            "notgoogle.com",
            "example.com",
            "google",
        ] {
            assert!(!is_google_host(host), "{host} should NOT be a Google host");
        }
    }

    #[test]
    fn gmail_proper_opens_externally_but_chat_does_not() {
        let chat = url::Url::parse("https://mail.google.com/chat/u/0/").unwrap();
        let gmail = url::Url::parse("https://mail.google.com/mail/u/0/").unwrap();
        assert!(!should_open_externally(&chat, "chat.google.com"));
        assert!(should_open_externally(&gmail, "chat.google.com"));
    }

    #[test]
    fn attachments_and_third_parties_open_externally() {
        let attachment =
            url::Url::parse("https://chat.google.com/u/0/api/get_attachment_url?x=1").unwrap();
        let third_party = url::Url::parse("https://example.com/thing").unwrap();
        assert!(should_open_externally(&attachment, "chat.google.com"));
        assert!(should_open_externally(&third_party, "chat.google.com"));
    }

    #[test]
    fn the_login_cctld_hop_stays_in_app() {
        // The exact shape that broke sign-in before is_google_host existed.
        let hop = url::Url::parse(
            "https://accounts.google.co.in/accounts/SetSID?ssdc=1&continue=https://mail.google.com/chat/u/0/",
        )
        .unwrap();
        assert!(!should_open_externally(&hop, "accounts.google.com"));
    }
}
