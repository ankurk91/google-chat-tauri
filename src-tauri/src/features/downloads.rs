//! Save downloads to the user's Downloads folder.
//!
//! The electron app had no download handling at all -- it detected attachment
//! URLs and handed them to the system browser, which meant you had to be signed
//! into Google there too. This covers everything the webview starts itself,
//! such as "Save image as" from the context menu.
//!
//! Attachment links still go to the browser for now; see `urls::is_in_app`.

use std::path::{Path, PathBuf};

use tauri::{Manager, Runtime, Webview};

pub fn handle<R: Runtime>(webview: Webview<R>, event: tauri::webview::DownloadEvent<'_>) -> bool {
    match event {
        tauri::webview::DownloadEvent::Requested { url, destination } => {
            let Ok(dir) = webview.app_handle().path().download_dir() else {
                log::warn!("no download directory; letting the webview decide");
                return true;
            };

            let name = file_name_for(destination, url.as_str());
            *destination = unique_path(&dir, &name);
            log::info!(
                "downloading {} -> {}",
                crate::redact::foreign_url_str(url.as_str()),
                crate::redact::path(destination)
            );
        }
        tauri::webview::DownloadEvent::Finished { url, path, success } => match (success, path) {
            (true, Some(p)) => log::info!("downloaded {}", crate::redact::path(&p)),
            (true, None) => log::info!(
                "downloaded {}",
                crate::redact::foreign_url_str(url.as_str())
            ),
            (false, _) => log::warn!(
                "download failed: {}",
                crate::redact::foreign_url_str(url.as_str())
            ),
        },
        _ => {}
    }
    true
}

/// The webview usually proposes a name; fall back to the URL's last segment,
/// then to something generic rather than writing to a directory path.
fn file_name_for(proposed: &Path, url: &str) -> String {
    if let Some(name) = proposed
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(sanitize)
    {
        return name;
    }

    // Parse rather than split on '/': "https://" has no path at all, and
    // naive splitting hands back the scheme as the filename.
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            let segment = u.path_segments()?.rfind(|s| !s.is_empty())?;
            // A path segment is percent-encoded by definition, so without this
            // a shared "quarterly report.pdf" lands as "quarterly%20report.pdf".
            let decoded = percent_encoding::percent_decode_str(segment).decode_utf8_lossy();
            sanitize(&decoded)
        })
        .unwrap_or_else(|| "download".to_owned())
}

/// Characters not to hand a filesystem: the separators, and the rest of the set
/// Windows refuses outright.
const UNSAFE: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Reduce a proposed name to something that can only name a file in the
/// directory we picked.
///
/// The webview's own suggestion arrives through `Path::file_name`, which has
/// already dropped anything that is not a plain name. A name taken from the URL
/// has had no such treatment: a segment can be `..`, which joins to the parent
/// directory rather than to a file; it can hold a separator once decoded; and
/// it can carry control characters straight out of a remote page.
fn sanitize(name: &str) -> Option<String> {
    let name = name.trim();

    // "." and ".." name directories, not files, and a run of dots is the same
    // idea with more of them.
    if name.is_empty() || name.chars().all(|c| c == '.') {
        return None;
    }

    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || UNSAFE.contains(&c) {
                '_'
            } else {
                c
            }
        })
        // Long enough for any real name, short enough to leave room for the
        // " (1)" that `unique_path` may add without passing what a filesystem
        // will take for one component.
        .take(200)
        .collect();

    (!cleaned.trim().is_empty()).then_some(cleaned)
}

/// Never silently overwrite an existing file: "report.pdf" becomes
/// "report (1).pdf".
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }

    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|s| s.to_str());

    for n in 1..1000 {
        let attempt = match ext {
            Some(ext) => dir.join(format!("{stem} ({n}).{ext}")),
            None => dir.join(format!("{stem} ({n})")),
        };
        if !attempt.exists() {
            return attempt;
        }
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_proposed_name() {
        assert_eq!(
            file_name_for(Path::new("/tmp/report.pdf"), "https://x/y"),
            "report.pdf"
        );
    }

    #[test]
    fn falls_back_to_the_url_and_strips_the_query() {
        assert_eq!(
            file_name_for(Path::new(""), "https://x/a/photo.png?sz=2"),
            "photo.png"
        );
        assert_eq!(file_name_for(Path::new(""), "https://x/files/"), "files");
    }

    #[test]
    fn a_url_segment_cannot_name_the_parent_directory() {
        // `dir.join("..")` is the downloads folder's parent, not a file in it.
        //
        // A literal `.` or `..` never actually reaches us: the url crate
        // resolves both away while parsing, so "https://x/a/.." is "https://x/"
        // with no segment left to take, and "https://x/a/." is "https://x/a/",
        // whose last real segment is an ordinary name.
        assert_eq!(file_name_for(Path::new(""), "https://x/a/.."), "download");
        assert_eq!(file_name_for(Path::new(""), "https://x/a/."), "a");

        // A longer run of dots is not a component the parser rewrites, so that
        // one does arrive, and `sanitize` is what turns it away.
        assert_eq!(file_name_for(Path::new(""), "https://x/a/..."), "download");
    }

    #[test]
    fn a_separator_that_survives_decoding_is_not_a_separator() {
        // %2F decodes to '/', which would otherwise reach into a subdirectory
        // -- or, with enough of them, out of the download folder entirely.
        assert_eq!(
            file_name_for(Path::new(""), "https://x/a/%2E%2E%2F%2E%2E%2Fetc%2Fpasswd"),
            ".._.._etc_passwd"
        );
    }

    #[test]
    fn a_percent_encoded_name_arrives_readable() {
        assert_eq!(
            file_name_for(Path::new(""), "https://x/a/quarterly%20report.pdf"),
            "quarterly report.pdf"
        );
    }

    #[test]
    fn control_characters_do_not_reach_the_filesystem() {
        assert_eq!(
            file_name_for(Path::new("/tmp/re\nport\u{7}.pdf"), "https://x/y"),
            "re_port_.pdf"
        );
    }

    #[test]
    fn an_enormous_name_is_cut_to_something_a_filesystem_takes() {
        let name = file_name_for(Path::new(""), &format!("https://x/{}", "a".repeat(500)));
        assert_eq!(name.chars().count(), 200);
    }

    #[test]
    fn never_returns_a_nonsense_name() {
        // A URL with no path used to yield the scheme, "https:".
        assert_eq!(file_name_for(Path::new(""), "https://"), "download");
        assert_eq!(
            file_name_for(Path::new(""), "https://example.com"),
            "download"
        );
        assert_eq!(file_name_for(Path::new(""), "not a url"), "download");
    }

    #[test]
    fn does_not_overwrite_an_existing_file() {
        let dir = std::env::temp_dir().join(format!("gchat-dl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"x").unwrap();

        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (1).txt"));
        assert_eq!(unique_path(&dir, "b.txt"), dir.join("b.txt"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
