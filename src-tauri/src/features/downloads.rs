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
            log::info!("downloading {url} -> {}", destination.display());
        }
        tauri::webview::DownloadEvent::Finished { url, path, success } => match (success, path) {
            (true, Some(p)) => log::info!("downloaded {}", p.display()),
            (true, None) => log::info!("downloaded {url}"),
            (false, _) => log::warn!("download failed: {url}"),
        },
        _ => {}
    }
    true
}

/// The webview usually proposes a name; fall back to the URL's last segment,
/// then to something generic rather than writing to a directory path.
fn file_name_for(proposed: &Path, url: &str) -> String {
    if let Some(name) = proposed.file_name().and_then(|n| n.to_str()) {
        if !name.is_empty() {
            return name.to_owned();
        }
    }

    // Parse rather than split on '/': "https://" has no path at all, and
    // naive splitting hands back the scheme as the filename.
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()?
                .rfind(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "download".to_owned())
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
