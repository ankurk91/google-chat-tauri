//! Ported from electron `src/urls.ts` and the allow-lists in
//! `src/main/features/externalLinks.ts`.

pub const APP_URL: &str = "https://mail.google.com/chat/u/0";

/// Used by the "Sign Out" menu item.
pub fn logout_url() -> String {
    format!("https://www.google.com/accounts/Logout?continue={APP_URL}")
}

/// Pre-filled "Report an Issue" link for the Help menu.
pub fn issue_url() -> String {
    format!(
        "{}/issues/new?body={}",
        env!("CARGO_PKG_REPOSITORY"),
        urlencoding_lite(&format!(
            "### Platform\n\n- App: {} {}\n- OS: {} {}\n",
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        ))
    )
}

/// Minimal percent-encoding for the query string above. Not a general-purpose
/// encoder -- it only has to survive our own fixed template.
fn urlencoding_lite(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Google's sign-in hosts, across every country domain.
///
/// This is deliberately narrow -- only `accounts.google.<tld>` -- and it exists
/// because signing in round-trips through the user's country domain
/// (`accounts.google.co.in/accounts/SetSID` in India, the local equivalent
/// elsewhere). Treating those as external hands the tail of the login flow to
/// the system browser, which then follows the `continue=` parameter and
/// finishes signing in *there*, leaving the app on the sign-in page.
pub fn is_accounts_host(host: &str) -> bool {
    let Some(rest) = host.strip_prefix("accounts.google.") else {
        // accounts.youtube.com participates in the same flow.
        return host == "accounts.youtube.com";
    };

    // What follows must look like a public suffix: one or two short alphabetic
    // labels ("com", "co.in", "com.au", "de"). Without a public-suffix list this
    // is what keeps "accounts.google.evil.com" out.
    let labels: Vec<&str> = rest.split('.').collect();
    matches!(labels.len(), 1 | 2)
        && labels
            .iter()
            .all(|l| (1..=3).contains(&l.len()) && l.chars().all(|c| c.is_ascii_alphabetic()))
}

/// Attachment downloads. The electron app handed these to the system browser.
const ATTACHMENT_URL: &str = "https://chat.google.com/u/0/api/get_attachment_url";

/// Is this URL part of the Chat app itself, rather than something Chat merely
/// links to?
///
/// The allow-list is short on purpose. Anything else -- including every other
/// Google property, so Docs, Sheets, Drive, Calendar and Meet links shared in a
/// conversation -- belongs in the user's real browser, where their extensions,
/// profiles and other tabs are.
fn is_in_app(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    // Attachments are a download, not a page, even though they live on the
    // Chat host.
    if url.as_str().contains(ATTACHMENT_URL) {
        return false;
    }

    match host {
        "chat.google.com" => true,
        // Chat-in-Gmail lives under /chat; the rest of that host is Gmail proper.
        "mail.google.com" => url.path().starts_with("/chat"),
        _ => is_accounts_host(host),
    }
}

/// Should this link be handed to the system browser instead of opened in-app?
pub fn should_open_externally(url: &url::Url) -> bool {
    !is_in_app(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn external(u: &str) -> bool {
        should_open_externally(&url::Url::parse(u).unwrap())
    }

    #[test]
    fn chat_itself_stays_in_app() {
        assert!(!external("https://chat.google.com/u/0/app/home"));
        assert!(!external("https://chat.google.com/room/AAAA"));
        assert!(!external("https://mail.google.com/chat/u/0/"));
    }

    #[test]
    fn other_google_products_open_in_the_browser() {
        // The whole point: a Docs link someone pastes into a conversation
        // belongs in the user's real browser, not in this window.
        for u in [
            "https://docs.google.com/document/d/abc/edit",
            "https://docs.google.com/spreadsheets/d/abc/edit",
            "https://drive.google.com/file/d/abc/view",
            "https://calendar.google.com/calendar/u/0/r",
            "https://meet.google.com/abc-defg-hij",
            "https://groups.google.com/g/some-group",
            "https://mail.google.com/mail/u/0/",
            "https://www.google.com/search?q=x",
        ] {
            assert!(external(u), "{u} should open externally");
        }
    }

    #[test]
    fn sign_in_works_in_every_country() {
        // The login hop goes through the user's country domain; sending it to
        // the browser strands the app on the sign-in page.
        for host in [
            "accounts.google.com",
            "accounts.google.co.in",
            "accounts.google.co.uk",
            "accounts.google.com.au",
            "accounts.google.de",
            "accounts.google.fr",
            "accounts.youtube.com",
        ] {
            let u = format!(
                "https://{host}/accounts/SetSID?continue=https://mail.google.com/chat/u/0/"
            );
            assert!(!external(&u), "{host} must stay in-app for sign-in");
        }
    }

    #[test]
    fn lookalike_auth_hosts_are_not_trusted() {
        for host in [
            "accounts.google.evil.com",
            "accounts.google.com.attacker.net",
            "notaccounts.google.com",
            "accounts.googleXcom",
        ] {
            assert!(
                external(&format!("https://{host}/")),
                "{host} must be external"
            );
        }
    }

    #[test]
    fn attachments_go_to_the_browser() {
        assert!(external(
            "https://chat.google.com/u/0/api/get_attachment_url?url_type=DOWNLOAD_URL"
        ));
    }

    #[test]
    fn third_parties_go_to_the_browser() {
        assert!(external("https://example.com/thing"));
        assert!(external("https://github.com/ankurk91"));
    }
}
