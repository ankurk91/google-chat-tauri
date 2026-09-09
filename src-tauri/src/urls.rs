//! Ported from electron `src/urls.ts` and the allow-lists in
//! `src/main/features/externalLinks.ts`.

pub const APP_URL: &str = "https://mail.google.com/chat/u/0";

/// Used by the "Sign Out" menu item.
pub fn logout_url() -> String {
    format!("https://www.google.com/accounts/Logout?continue={APP_URL}")
}

/// Where to send someone whose session has ended.
///
/// Not `APP_URL`. That is the URL a sign-out has already bounced off, so going
/// back to it only repeats whatever Google decided the first time -- which is
/// sometimes the sign-in form and sometimes a marketing page. This is the form
/// itself, which then follows `continue` into the app.
pub fn sign_in_url() -> String {
    format!("https://accounts.google.com/ServiceLogin?continue={APP_URL}")
}

/// Where the update check asks what has been released.
///
/// The list, not `/releases/latest`: that endpoint is documented as "the most
/// recent non-prerelease, non-draft release", so a repository whose only
/// release is a pre-release has no latest at all and answers 404 -- which is
/// indistinguishable from a repository that has never released anything.
/// Measured against this one on 2026-09-06, with v0.0.1 published as a
/// pre-release: `/releases/latest` 404, `/releases` one entry.
///
/// Derived from `CARGO_PKG_REPOSITORY` for the same reason the issue URL is:
/// the repository is named once, in `Cargo.toml`, and a fork or a rename does
/// not leave a hard-coded owner behind pointing everyone at the original.
pub fn releases_api() -> String {
    let path = env!("CARGO_PKG_REPOSITORY")
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_start_matches("https://github.com/");

    // A handful is plenty: the newest few are the only ones that can be newer
    // than what is running.
    format!("https://api.github.com/repos/{path}/releases?per_page=10")
}

/// How long the pre-filled issue URL is allowed to get.
///
/// GitHub itself accepts far more, but the request travels through whatever
/// browser, launcher and proxy the user has, and 2000 characters is the
/// smallest limit anything in that chain is likely to impose. Overshooting does
/// not truncate the body -- it opens nothing at all, which is the one outcome
/// worse than a short report.
const MAX_ISSUE_URL: usize = 2000;

/// The questions whose answers are missing from almost every first report.
const PROMPTS: &str = "### What happened

### What you expected

### Steps to reproduce

1.
2.
";

/// Where the rest of the answers already are, written down.
const LOGS: &str = "### Logs

Help > Show Logs, then attach the log file. The block at the top of it is the
part that matters.
";

/// Pre-filled "Report an Issue" link for the Help menu.
///
/// The facts come from `diagnostics::facts`, the same function that writes the
/// log header, so a report and its attached log can never disagree about which
/// machine they came from.
pub fn issue_url() -> String {
    let facts: String = crate::features::diagnostics::facts()
        .iter()
        .map(|f| format!("- {f}\n"))
        .collect();

    let environment = format!(
        "### Environment\n\n- app: {} {} ({})\n{facts}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        crate::features::diagnostics::profile(),
    );

    // Longest first, and what goes when it does not fit is the prose: someone
    // can describe their own problem unprompted, but nobody retypes their
    // WebKitGTK version from memory.
    for body in [
        format!("{PROMPTS}\n{environment}\n{LOGS}"),
        format!("{environment}\n{LOGS}"),
        environment.clone(),
    ] {
        let url = new_issue_url(&body);
        if url.len() <= MAX_ISSUE_URL {
            return url;
        }
    }

    // Only reachable if the environment block alone is enormous -- a
    // distribution with a novel for a PRETTY_NAME. Trim the body by characters
    // and re-encode rather than cutting the finished URL, which would leave
    // half a `%E2` escape behind.
    fit(environment)
}

/// Shorten `body` until the URL built from it fits, and return that URL.
fn fit(mut body: String) -> String {
    loop {
        let url = new_issue_url(&body);
        if url.len() <= MAX_ISSUE_URL || body.is_empty() {
            return url;
        }
        let keep = body.chars().count().saturating_sub(64);
        body = body.chars().take(keep).collect();
    }
}

fn new_issue_url(body: &str) -> String {
    format!(
        "{}/issues/new?body={}",
        env!("CARGO_PKG_REPOSITORY"),
        urlencoding_lite(body)
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

/// The marketing paths on `www.google.com`, which is otherwise off limits here.
///
/// Matched anywhere in the path so the localised forms are covered too:
/// `/gmail/about/` and `/intl/en-GB/gmail/about/` are the same page.
const LANDING_PATHS: [&str; 2] = ["/gmail/about", "/chat/about"];

/// Has Google parked the window on one of its own marketing pages?
///
/// Signing out sends the browser to `accounts/Logout?continue=<APP_URL>`, and
/// Google then decides -- not always the same way, which is why this is hard to
/// reproduce -- whether a session-less visit to Chat gets the sign-in form or an
/// advertisement for Workspace. The advertisement is a dead end: it is not the
/// app, so "Go to Chat" only bounces off the same redirect, and it is not an
/// origin the IPC capability covers, so until `chat.js` learned to fall back it
/// could not even follow its own "Sign in" link. Reported in the wild as
/// `https://workspace.google.com/intl/en-US/gmail/`.
pub fn is_signed_out_landing(url: &url::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    match host {
        // The Workspace marketing site in full. Nothing under it is the app.
        "workspace.google.com" => true,
        // Only the product corners of www.google.com: the rest of that host
        // carries the sign-out endpoint this arrives through.
        "www.google.com" => LANDING_PATHS.iter().any(|p| url.path().contains(p)),
        _ => false,
    }
}

/// Attachment downloads. The electron app handed these to the system browser.
///
/// The path only, matched against the path: the whole URL used to be looked for
/// as a substring of the whole URL, which said yes to any Chat link merely
/// carrying that text in its query and sent a perfectly ordinary page to the
/// browser. It also hard-coded `/u/0/`, so the same endpoint under a second
/// signed-in account -- `/u/1/`, `/u/2/` -- was not recognised at all and the
/// download opened in this window.
const ATTACHMENT_PATH: &str = "/api/get_attachment_url";

/// Is this the attachment endpoint, under whichever account slot?
fn is_attachment(url: &url::Url) -> bool {
    url.host_str() == Some("chat.google.com") && url.path().ends_with(ATTACHMENT_PATH)
}

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
    if is_attachment(url) {
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
    fn attachments_under_a_second_account_go_there_too() {
        // The slot is the signed-in account, and it is not always 0. Matching
        // the whole URL against a `/u/0/` literal missed every other one.
        for slot in ["u/1", "u/2", "u/17"] {
            let url = format!("https://chat.google.com/{slot}/api/get_attachment_url?x=1");
            assert!(external(&url), "{url} should open externally");
        }
    }

    #[test]
    fn an_ordinary_chat_link_is_not_an_attachment() {
        // The endpoint used to be looked for anywhere in the URL, so a link
        // that merely mentioned it -- in a query parameter, say -- was shipped
        // off to the browser instead of opening in the app.
        assert!(!external(
            "https://chat.google.com/room/AAAA?continue=/u/0/api/get_attachment_url"
        ));
        assert!(!external(
            "https://chat.google.com/u/0/api/get_attachment_urls"
        ));
    }

    #[test]
    fn third_parties_go_to_the_browser() {
        assert!(external("https://example.com/thing"));
        assert!(external("https://github.com/ankurk91"));
    }

    fn landing(u: &str) -> bool {
        is_signed_out_landing(&url::Url::parse(u).unwrap())
    }

    #[test]
    fn the_workspace_advertisement_is_a_dead_end() {
        // The one seen in the wild after Sign Out, and its neighbours.
        assert!(landing("https://workspace.google.com/intl/en-US/gmail/"));
        assert!(landing("https://workspace.google.com/"));
        assert!(landing("https://workspace.google.com/products/chat/"));
        assert!(landing("https://www.google.com/gmail/about/"));
        assert!(landing("https://www.google.com/intl/en-GB/gmail/about/"));
    }

    #[test]
    fn the_sign_out_endpoint_is_not_a_dead_end() {
        // It lives on www.google.com and is the very hop that leads here, so
        // catching it would redirect the user off their own sign-out.
        assert!(!landing(&logout_url()));
        assert!(!landing("https://www.google.com/accounts/Logout"));
    }

    #[test]
    fn the_app_and_its_sign_in_are_never_a_dead_end() {
        for u in [
            APP_URL,
            "https://chat.google.com/u/0/app/home",
            "https://accounts.google.com/ServiceLogin",
            "https://accounts.google.co.in/accounts/SetSID",
            &sign_in_url(),
        ] {
            assert!(!landing(u), "{u} must not be treated as a dead end");
        }
    }

    #[test]
    fn the_sign_in_url_comes_back_to_the_app() {
        let url = sign_in_url();
        assert!(url.starts_with("https://accounts.google.com/"), "{url}");
        // Without the continue parameter the user signs in and stays on
        // Google's account page, which is the dead end all over again.
        assert!(url.contains(&format!("continue={APP_URL}")), "{url}");
        // And it must be somewhere the link policy keeps in-app, or the sign-in
        // finishes in the system browser.
        assert!(!external(&url), "the sign-in page must stay in-app");
    }

    #[test]
    fn the_injected_script_knows_where_chat_is() {
        // chat.js has to carry the address itself: its offline page cannot ask
        // Rust for it, because a failed-load document has an opaque origin and
        // Tauri rejects every invoke from one. This is what keeps the copy from
        // drifting away from the constant above.
        assert!(
            crate::inject::SCRIPT.contains(APP_URL),
            "chat.js no longer contains {APP_URL}"
        );
    }

    #[test]
    fn the_release_api_url_points_at_this_repository() {
        let url = releases_api();
        assert!(
            url.starts_with("https://api.github.com/repos/"),
            "not an api.github.com URL: {url}"
        );
        // The list. `/releases/latest` cannot see a pre-release.
        assert!(url.contains("/releases?"), "wrong endpoint: {url}");
        // No scheme left in the middle: the owner/name pair, and nothing else.
        assert_eq!(url.matches("https://").count(), 1, "{url}");
        assert!(!url.contains(".git/"), "{url}");
    }

    #[test]
    fn the_issue_url_fits_in_a_url_bar() {
        let url = issue_url();
        assert!(url.len() <= MAX_ISSUE_URL, "{} characters", url.len());
    }

    #[test]
    fn the_issue_url_carries_the_environment() {
        let url = issue_url();
        assert!(url.starts_with("https://github.com/"));
        // Percent-encoding leaves alphanumerics alone, so the labels survive
        // literally -- which is what makes asserting on them worth anything.
        for label in ["app", "platform", "webview"] {
            assert!(url.contains(label), "{label} missing from {url}");
        }
    }

    #[test]
    fn an_over_long_body_is_trimmed_without_cutting_an_escape() {
        // Multibyte on purpose: trimming the finished URL rather than the body
        // it came from would leave half a `%E2` behind.
        let url = fit("→".repeat(4000));
        assert!(url.len() <= MAX_ISSUE_URL, "{} characters", url.len());
        assert!(
            !url[url.len() - 2..].contains('%'),
            "url ends mid-escape: {url}"
        );
    }
}
