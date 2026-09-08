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

/// `scheme://host/path`, with the query and fragment reduced to a marker.
///
/// The count is kept because "the link had five parameters" is sometimes the
/// difference between two code paths, and a count names nobody.
pub fn url(u: &url::Url) -> String {
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

    out.push_str(u.path());

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

/// The same, for a URL that arrived as text and may not parse.
///
/// An unparseable string is not printed: it came from the page or the webview,
/// so there is no telling what is in it.
pub fn url_str(s: &str) -> String {
    match url::Url::parse(s) {
        Ok(u) => url(&u),
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
    }

    #[test]
    fn a_plain_url_is_left_alone() {
        // The common case has to stay readable, or the log stops being worth
        // reading.
        let u = url::Url::parse("https://mail.google.com/chat/u/0").unwrap();
        assert_eq!(url(&u), "https://mail.google.com/chat/u/0");
    }

    #[test]
    fn unparseable_text_is_not_printed() {
        assert_eq!(url_str("mail.google.com/chat"), "<unparseable url>");
        assert_eq!(url_str("https://x/y"), "https://x/y");
    }
}
