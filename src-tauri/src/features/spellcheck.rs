//! Linux only: spell checking in the message box.
//!
//! WebKitGTK checks spelling through Enchant, which uses Hunspell (or Aspell,
//! if its Enchant provider is installed). It is off by default, and Tauri has
//! no setting for it, so it is switched on here on the webview's
//! `WebKitWebContext` directly. With it on, misspelled words are underlined and
//! WebKit's own context menu offers corrections; the app does not replace that
//! menu, so nothing more is needed for the suggestions.
//!
//! Enabling it is not enough on its own: WebKit checks against nothing until it
//! is given a language list. The list is the user's locale plus `en_US`.
//! Languages with no installed dictionary are skipped by WebKit, so asking for
//! `en_IN` on a machine that only has `hunspell-en-us` is harmless -- and is
//! why `en_US` is always on the list. What actually loaded is logged, since a
//! missing dictionary is otherwise invisible.
//!
//! macOS and Windows are left to whatever their webviews do by default.

use tauri::WebviewWindow;
use webkit2gtk::{WebContextExt, WebViewExt};

/// Always asked for, after the locale's own languages.
const FALLBACK: &str = "en_US";

pub fn install(window: &WebviewWindow) {
    let asked = window.with_webview(|webview| {
        let Some(context) = webview.inner().context() else {
            log::warn!("spellcheck: the webview has no context, left off");
            return;
        };

        let names = webkit2gtk::glib::language_names();
        let wanted = languages(names.iter().map(|n| n.as_str()));
        let wanted: Vec<&str> = wanted.iter().map(String::as_str).collect();

        context.set_spell_checking_languages(&wanted);
        context.set_spell_checking_enabled(true);

        let loaded = context.spell_checking_languages();
        if loaded.is_empty() {
            log::warn!(
                "spellcheck: no dictionary for any of {wanted:?}; install one, \
                 e.g. hunspell-en-us"
            );
        } else {
            let loaded: Vec<&str> = loaded.iter().map(|l| l.as_str()).collect();
            log::info!("spellcheck: on, using {loaded:?} (asked for {wanted:?})");
        }
    });

    if let Err(e) = asked {
        log::warn!("spellcheck: no webview to enable it on: {e}");
    }
}

/// The dictionaries to ask for, from glib's locale names (`LANGUAGE`, then
/// `LC_ALL` / `LC_MESSAGES` / `LANG`), most preferred first.
///
/// glib lists every variant of each name -- `en_IN.UTF-8`, `en_IN`, `en`, `C`
/// -- so only the plain `ll_CC` form is kept: the encoded and `@modifier`
/// forms are not dictionary names, and a bare `en` would load whichever English
/// dictionary Enchant finds first, on top of the specific one.
fn languages<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names.into_iter().chain([FALLBACK]) {
        let Some((lang, region)) = name.split_once('_') else {
            continue;
        };
        let plain = |s: &str, len: std::ops::RangeInclusive<usize>| {
            len.contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphabetic())
        };
        if !plain(lang, 2..=3) || !plain(region, 2..=2) {
            continue;
        }
        if !out.iter().any(|o| o == name) {
            out.push(name.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_locale_and_adds_en_us() {
        let names = ["en_IN.UTF-8", "en_IN", "en.UTF-8", "en", "C.UTF-8", "C"];
        assert_eq!(languages(names), ["en_IN", "en_US"]);
    }

    #[test]
    fn does_not_ask_for_en_us_twice() {
        assert_eq!(languages(["en_US.UTF-8", "en_US", "en", "C"]), ["en_US"]);
    }

    #[test]
    fn keeps_every_language_in_preference_order() {
        let names = ["de_DE.UTF-8", "de_DE", "de", "fr_FR", "fr", "C"];
        assert_eq!(languages(names), ["de_DE", "fr_FR", "en_US"]);
    }

    #[test]
    fn falls_back_to_en_us_for_the_c_locale() {
        assert_eq!(languages(["C.UTF-8", "C", "POSIX"]), ["en_US"]);
    }

    #[test]
    fn drops_modifiers_and_codesets() {
        assert_eq!(
            languages(["sr_RS@latin", "ca_ES@valencia", "ja_JP.eucJP"]),
            ["en_US"]
        );
    }
}
