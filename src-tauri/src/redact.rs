//! Keeping identity out of the log file.
//!
//! Logs are written to be attached to a bug report, which means they leave the
//! machine and land in a public issue. Two things carry identity there and
//! neither is obvious at the call site:
//!
//! * **Paths.** Every one of ours begins with the home directory, so
//!   `log dir: /home/jane/.local/share/...` names the account. Windows is worse
//!   -- `C:\Users\Jane Smith\` is often a full name.
//! * **URL queries.** Google puts the signed-in address in `Email`,
//!   `identifier` and `authuser`, and an attachment link carries a bearer token
//!   in its query. The scheme, host and path say which endpoint was involved,
//!   which is the part a report needs; the query is where the identity is.
//!
//! Everything the log prints from a `Path` or a `Url` goes through here first.
//! Message bodies and sender names never reach Rust at all -- `chat.js` redacts
//! notification payloads on its own side, and `commands::show_notification`
//! hands the title and body straight to the OS without logging them.

use std::path::Path;

/// A path with the home directory folded to `~`.
///
/// Falls back to the last component alone when the path is somewhere else
/// entirely -- an unexpected absolute path is not worth printing in full just
/// because it did not match, and the file name is what a report is asking
/// about anyway.
pub fn path(p: &Path) -> String {
    let Some(home) = home_dir() else {
        // No home to strip: keep a relative path whole, reduce anything
        // absolute to its name.
        return if p.is_absolute() {
            tail(p)
        } else {
            p.display().to_string()
        };
    };

    match p.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()).replace('\\', "/"),
        Err(_) if p.is_absolute() => tail(p),
        Err(_) => p.display().to_string(),
    }
}

/// A URL **this app built**, with the query and fragment reduced to a marker.
///
/// The path survives here because we wrote it: the releases endpoint and the
/// sign-in target are `format!`ed from constants, so their paths describe our
/// own code and name nobody. The count is kept because "the link had five
/// parameters" is sometimes the difference between two code paths.
///
/// Do not reach for this for a URL that arrived from the page -- see
/// `foreign_url`, which is the one that decides what a bug report may know
/// about where somebody was and what they clicked.
pub fn url(u: &url::Url) -> String {
    render(u, true)
}

/// A URL **the page handed us**: the origin, and nothing after it.
///
/// A link clicked inside a chat, the address the window is sitting on, the
/// target of a hand-off. The path there is not ours and is not structure -- it
/// is content, and it is specific enough to identify the person and their
/// employer in one line. A real log from a signed-in session read
///
/// ```text
/// link intercepted: https://github.com/<org>/<private repo>/pull/1042
/// ```
///
/// three times over, from the three modules that each log the hand-off.
///
/// The host stays, because the host is the whole of the decision: the
/// allow-list in `urls` routes on it, so it is what a report about a link
/// opening in the wrong place has to say. Everything past it is dropped.
pub fn foreign_url(u: &url::Url) -> String {
    render(u, false)
}

fn render(u: &url::Url, keep_path: bool) -> String {
    let mut out = String::new();
    out.push_str(u.scheme());
    out.push(':');

    if u.has_host() {
        out.push_str("//");
        // Skip userinfo deliberately: `https://user:pw@host/` is a credential.
        out.push_str(u.host_str().unwrap_or(""));
        if let Some(port) = u.port() {
            out.push_str(&format!(":{port}"));
        }
    }

    if keep_path {
        out.push_str(u.path());
    } else if u.path() != "/" && !u.path().is_empty() {
        // Not silence: "there was a path" separates a link into a site from a
        // link to its front page, which is the kind of thing a hand-off bug
        // turns on.
        out.push_str("/<path>");
    }

    if let Some(query) = u.query() {
        let n = if query.is_empty() {
            0
        } else {
            query.split('&').count()
        };
        out.push_str(&format!("?<{n} params>"));
    }
    if u.fragment().is_some() {
        out.push_str("#<fragment>");
    }

    out
}

/// `url`, for one of ours that arrived as text and may not parse.
///
/// An unparseable string is not printed: there is no telling what is in it.
pub fn url_str(s: &str) -> String {
    match url::Url::parse(s) {
        Ok(u) => url(&u),
        Err(_) => "<unparseable url>".to_owned(),
    }
}

/// `foreign_url`, for one that arrived as text -- which is how every URL from
/// the page arrives.
pub fn foreign_url_str(s: &str) -> String {
    match url::Url::parse(s) {
        Ok(u) => foreign_url(&u),
        Err(_) => "<unparseable url>".to_owned(),
    }
}

fn tail(p: &Path) -> String {
    match p.file_name() {
        Some(name) => format!(".../{}", Path::new(name).display()),
        None => "<path>".to_owned(),
    }
}

/// `$HOME`, or `%USERPROFILE%` on Windows.
///
/// Read from the environment rather than pulled in as a dependency: this is
/// the same variable `dirs` consults first, and a log line is not worth a crate.
fn home_dir() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let key = "USERPROFILE";
    #[cfg(not(windows))]
    let key = "HOME";

    std::env::var_os(key)
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn with_home<T>(home: &str, f: impl FnOnce() -> T) -> T {
        // Serialised by the mutex below: these tests share one process
        // environment, and a parallel test reading HOME mid-swap sees the
        // wrong one.
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

        #[cfg(windows)]
        let key = "USERPROFILE";
        #[cfg(not(windows))]
        let key = "HOME";

        let saved = std::env::var_os(key);
        // SAFETY: single-threaded within the lock above.
        unsafe { std::env::set_var(key, home) };
        let out = f();
        match saved {
            Some(v) => unsafe { std::env::set_var(key, v) },
            None => unsafe { std::env::remove_var(key) },
        }
        out
    }

    #[test]
    fn the_home_directory_becomes_a_tilde() {
        with_home("/home/jane", || {
            assert_eq!(
                path(&PathBuf::from("/home/jane/.local/share/app/logs")),
                "~/.local/share/app/logs"
            );
            assert_eq!(path(&PathBuf::from("/home/jane")), "~");
        });
    }

    #[test]
    fn a_path_outside_home_keeps_only_its_name() {
        with_home("/home/jane", || {
            // /media/jane-usb would name the account just as well.
            assert_eq!(
                path(&PathBuf::from("/media/jane-usb/report.pdf")),
                ".../report.pdf"
            );
            assert_eq!(path(&PathBuf::from("logs/app.log")), "logs/app.log");
        });
    }

    #[test]
    fn no_home_still_hides_absolute_paths() {
        with_home("", || {
            assert_eq!(
                path(&PathBuf::from("/home/jane/notes.txt")),
                ".../notes.txt"
            );
        });
    }

    #[test]
    fn a_query_becomes_a_count() {
        let u = url::Url::parse(
            "https://accounts.google.com/ServiceLogin?Email=jane%40example.com&continue=x",
        )
        .unwrap();
        assert_eq!(
            url(&u),
            "https://accounts.google.com/ServiceLogin?<2 params>"
        );
    }

    #[test]
    fn credentials_and_fragments_do_not_survive() {
        let u = url::Url::parse("https://jane:hunter2@example.com:8443/a/b#token=x").unwrap();
        assert_eq!(url(&u), "https://example.com:8443/a/b#<fragment>");
        assert_eq!(
            foreign_url(&u),
            "https://example.com:8443/<path>#<fragment>"
        );
    }

    #[test]
    fn one_of_ours_keeps_the_path_that_we_wrote() {
        // The common case has to stay readable, or the log stops being worth
        // reading -- and this path came out of a format! over constants.
        let u = url::Url::parse("https://api.github.com/repos/o/r/releases").unwrap();
        assert_eq!(url(&u), "https://api.github.com/repos/o/r/releases");
    }

    #[test]
    fn a_link_from_the_page_keeps_only_its_host() {
        // Measured, not hypothetical: this is the shape of the line that named
        // an employer, a private repository and a pull request in a log meant
        // for a public issue.
        let u = url::Url::parse("https://github.com/acme-corp/secret-api/pull/1042").unwrap();
        assert_eq!(foreign_url(&u), "https://github.com/<path>");

        // Chat's own paths are no safer than anyone else's.
        let room = url::Url::parse("https://chat.google.com/room/AAAAmZ2f").unwrap();
        assert_eq!(foreign_url(&room), "https://chat.google.com/<path>");
    }

    #[test]
    fn a_front_page_is_distinguishable_from_a_deep_link() {
        // Which of the two it was is the kind of thing a hand-off bug turns on.
        let root = url::Url::parse("https://example.com/").unwrap();
        assert_eq!(foreign_url(&root), "https://example.com");
    }

    #[test]
    fn unparseable_text_is_not_printed() {
        assert_eq!(url_str("mail.google.com/chat"), "<unparseable url>");
        assert_eq!(foreign_url_str("mail.google.com/chat"), "<unparseable url>");
        assert_eq!(url_str("https://x/y"), "https://x/y");
        assert_eq!(foreign_url_str("https://x/y"), "https://x/<path>");
    }
}
