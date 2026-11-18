//! Linux only: spell checking in the message box, and Preferences → Spelling.
//!
//! WebKitGTK checks spelling through Enchant, which uses Hunspell (or Aspell,
//! if its Enchant provider is installed). It is off by default, and Tauri has
//! no setting for it, so it is switched on here on the webview's
//! `WebKitWebContext` directly. With it on, misspelled words are underlined and
//! WebKit's own context menu offers corrections; the app does not replace that
//! menu, so nothing more is needed for the suggestions.
//!
//! Enabling it is not enough on its own: WebKit checks against nothing until it
//! is given a language list. On automatic -- the default -- that is the user's
//! locale plus `en_US`; the Spelling menu replaces it with the dictionaries the
//! user ticks. Several can be on at once, and a word passes if any of them
//! accepts it. Languages with no installed dictionary are skipped by WebKit,
//! so the ticks follow what WebKit reports it *loaded*, never what was asked
//! for, and the log says the same.
//!
//! Every change applies at once: the context takes a new list or an on/off
//! while the page is up, so nothing needs a restart. The menu's list of
//! dictionaries is read once at startup, so a dictionary installed meanwhile
//! appears on the next launch.
//!
//! macOS and Windows are left to whatever their webviews do by default.

use std::sync::{Mutex, OnceLock};

use tauri::menu::{CheckMenuItemBuilder, MenuItemBuilder, Submenu, SubmenuBuilder};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow};
use webkit2gtk::{WebContextExt, WebViewExt};

use crate::config::{self, Config, Prefs};
use crate::features::app_menu::nested_check_item;
use crate::features::window::MAIN;

/// Always asked for on automatic, after the locale's own languages.
const FALLBACK: &str = "en_US";

/// Every id this module owns starts with this, so `app_menu::handle` can pass
/// them all along in one arm.
pub const MENU_PREFIX: &str = "spell-";
const ENABLED_ID: &str = "spell-enabled";
const AUTO_ID: &str = "spell-auto";
const LANG_PREFIX: &str = "spell-lang:";

/// The dictionaries the menu offers, settled when it is built.
static CHOICES: OnceLock<Vec<String>> = OnceLock::new();

/// What WebKit last reported loaded. Ticking a language while on automatic
/// starts from this, so the languages already in use stay in use.
static LOADED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn install(window: &WebviewWindow) {
    apply(window.app_handle());
}

/// Hand the stored preferences to WebKit, then make the menu say what loaded.
pub fn apply(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        log::warn!("spellcheck: no window to apply it to");
        return;
    };

    let prefs = app.state::<Config>().get();
    let wanted = wanted(&prefs);
    let app = app.clone();

    let asked = window.with_webview(move |webview| {
        let Some(context) = webview.inner().context() else {
            log::warn!("spellcheck: the webview has no context, left off");
            return;
        };

        let tags: Vec<&str> = wanted.iter().map(String::as_str).collect();
        context.set_spell_checking_languages(&tags);
        context.set_spell_checking_enabled(prefs.spellcheck);

        let loaded: Vec<String> = context
            .spell_checking_languages()
            .iter()
            .map(|l| l.to_string())
            .collect();

        let mode = if prefs.spell_languages.is_none() {
            "automatic"
        } else {
            "chosen"
        };
        if !prefs.spellcheck {
            log::info!("spellcheck: off");
        } else if loaded.is_empty() {
            log::warn!(
                "spellcheck: no dictionary for any of {wanted:?} ({mode}); install one, \
                 e.g. hunspell-en-us"
            );
        } else {
            log::info!("spellcheck: on, using {loaded:?} ({mode}, asked for {wanted:?})");
        }

        *LOADED.lock().unwrap() = loaded.clone();
        sync_menu(
            &app,
            prefs.spellcheck,
            prefs.spell_languages.is_none(),
            &loaded,
        );
    });

    if let Err(e) = asked {
        log::warn!("spellcheck: no webview to apply it to: {e}");
    }
}

/// Preferences → Spelling. Ticks start from what is about to be asked for;
/// `apply` corrects them to what WebKit actually loaded a moment later.
pub fn submenu<R: Runtime>(app: &AppHandle<R>, prefs: &Prefs) -> tauri::Result<Submenu<R>> {
    let choices = CHOICES.get_or_init(choices);
    let wanted = wanted(prefs);
    let on = prefs.spellcheck;

    let mut menu = SubmenuBuilder::new(app, "Spelling")
        .item(
            &CheckMenuItemBuilder::with_id(ENABLED_ID, "Check Spelling")
                .checked(on)
                .build(app)?,
        )
        .separator()
        .item(
            &CheckMenuItemBuilder::with_id(AUTO_ID, "Automatic (System Languages)")
                .checked(prefs.spell_languages.is_none())
                .enabled(on)
                .build(app)?,
        )
        .separator();

    if choices.is_empty() {
        menu = menu.item(
            &MenuItemBuilder::with_id("spell-none", "No Dictionaries Installed")
                .enabled(false)
                .build(app)?,
        );
    }
    for tag in choices {
        menu = menu.item(
            &CheckMenuItemBuilder::with_id(format!("{LANG_PREFIX}{tag}"), label(tag))
                .checked(wanted.contains(tag))
                .enabled(on)
                .build(app)?,
        );
    }

    menu.build()
}

/// A click under Preferences → Spelling.
///
/// A check item flips its own tick before this runs, so every branch ends in
/// `apply`, which sets every tick from what WebKit loaded -- including putting
/// back one that was clicked to no effect.
pub fn handle(app: &AppHandle, id: &str) {
    if id == ENABLED_ID {
        config::update_and_save(app, |p| p.spellcheck = !p.spellcheck);
    } else if id == AUTO_ID {
        config::update_and_save(app, |p| p.spell_languages = None);
    } else if let Some(tag) = id.strip_prefix(LANG_PREFIX) {
        let loaded = LOADED.lock().unwrap().clone();
        let mut refused = false;
        config::update_and_save(app, |p| {
            let current = p.spell_languages.clone().unwrap_or(loaded);
            match toggled(current, tag) {
                Some(next) => p.spell_languages = Some(next),
                None => refused = true,
            }
        });
        if refused {
            log::info!("spellcheck: kept {tag}, the last language; Check Spelling turns it off");
        }
    } else {
        return;
    }

    apply(app);
}

fn sync_menu(app: &AppHandle, on: bool, auto: bool, loaded: &[String]) {
    let set = |id: &str, checked: bool, enabled: bool| {
        if let Some(item) = nested_check_item(app, id) {
            let _ = item.set_checked(checked);
            let _ = item.set_enabled(enabled);
        }
    };

    set(ENABLED_ID, on, true);
    set(AUTO_ID, auto, on);
    for tag in CHOICES.get().map(Vec::as_slice).unwrap_or_default() {
        set(&format!("{LANG_PREFIX}{tag}"), loaded.contains(tag), on);
    }
}

/// The languages to hand WebKit: the user's picks, or automatic.
fn wanted(prefs: &Prefs) -> Vec<String> {
    prefs
        .spell_languages
        .clone()
        .unwrap_or_else(system_languages)
}

fn system_languages() -> Vec<String> {
    let names = webkit2gtk::glib::language_names();
    languages(names.iter().map(|n| n.as_str()))
}

/// The dictionaries to offer in the menu.
fn choices() -> Vec<String> {
    let system = system_languages();
    let Some(enchant) = enchant::Enchant::open() else {
        log::warn!("spellcheck: Enchant not reachable, offering the system languages only");
        return system;
    };
    let offered = offered(&enchant.list(), |tag| enchant.exists(tag), &system);
    log::debug!("spellcheck: dictionaries offered {offered:?}");
    offered
}

/// Automatic: dictionary names from glib's locale names (`LANGUAGE`, then
/// `LC_ALL` / `LC_MESSAGES` / `LANG`), most preferred first, then `en_US`.
///
/// glib lists every variant of each name -- `en_IN.UTF-8`, `en_IN`, `en`, `C`
/// -- so the encoded and `@modifier` forms are dropped: they are not dictionary
/// names. A bare language (`de`) is kept only when no `ll_CC` of the same
/// language is on the list, so `en_IN:en` does not load whichever English
/// dictionary Enchant finds first on top of the specific one, while a locale of
/// plain `de` still gets German.
fn languages<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let names: Vec<&str> = names.into_iter().chain([FALLBACK]).collect();
    let has_region = |lang: &str| {
        names
            .iter()
            .any(|n| matches!(split(n), Some((l, Some(_))) if l == lang))
    };

    let mut out: Vec<String> = Vec::new();
    for name in &names {
        let keep = match split(name) {
            Some((_, Some(_))) => true,
            Some((lang, None)) => !has_region(lang),
            None => false,
        };
        if keep && !out.iter().any(|o| o == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// What the menu lists: every installed dictionary, plus any of the system
/// languages Enchant can serve. The second half is for a locale such as
/// `en_IN`, which no dictionary is named after but which Enchant answers from
/// its generic `en`. A bare language is hidden when a regional one of it is
/// listed, as in `languages`. Sorted, so related languages sit together.
fn offered(installed: &[String], exists: impl Fn(&str) -> bool, system: &[String]) -> Vec<String> {
    let mut out: Vec<String> = installed
        .iter()
        .filter(|t| split(t).is_some())
        .cloned()
        .collect();
    for tag in system {
        if split(tag).is_some() && !out.contains(tag) && exists(tag) {
            out.push(tag.clone());
        }
    }

    let regional: Vec<String> = out
        .iter()
        .filter_map(|t| match split(t) {
            Some((lang, Some(_))) => Some(lang.to_string()),
            _ => None,
        })
        .collect();
    out.retain(|t| !matches!(split(t), Some((lang, None)) if regional.iter().any(|r| r == lang)));

    out.sort();
    out.dedup();
    out
}

/// Turn one language on or off in a chosen list. `None` when that would leave
/// nothing to check against -- Check Spelling is the way to turn it off.
fn toggled(mut list: Vec<String>, tag: &str) -> Option<Vec<String>> {
    if let Some(at) = list.iter().position(|t| t == tag) {
        list.remove(at);
        if list.is_empty() {
            return None;
        }
    } else {
        list.push(tag.to_string());
    }
    Some(list)
}

/// `ll` or `ll_CC`, split; `None` for anything else (`C`, `en_IN.UTF-8`,
/// `sr_RS@latin`, `en_GB-ise`).
fn split(tag: &str) -> Option<(&str, Option<&str>)> {
    let alpha = |s: &str, len: std::ops::RangeInclusive<usize>| {
        len.contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphabetic())
    };
    match tag.split_once('_') {
        Some((lang, region)) if alpha(lang, 2..=3) && alpha(region, 2..=2) => {
            Some((lang, Some(region)))
        }
        None if alpha(tag, 2..=3) && tag != "C" => Some((tag, None)),
        _ => None,
    }
}

/// `English (en_US)`. The code stays in the label: it is what tells two
/// Englishes apart, and what the user will find in a package name.
fn label(tag: &str) -> String {
    let name = split(tag).and_then(|(lang, _)| language_name(lang));
    match name {
        Some(name) => format!("{name} ({tag})"),
        None => tag.to_string(),
    }
}

fn language_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "af" => "Afrikaans",
        "ar" => "Arabic",
        "bg" => "Bulgarian",
        "bn" => "Bengali",
        "ca" => "Catalan",
        "cs" => "Czech",
        "cy" => "Welsh",
        "da" => "Danish",
        "de" => "German",
        "el" => "Greek",
        "en" => "English",
        "eo" => "Esperanto",
        "es" => "Spanish",
        "et" => "Estonian",
        "eu" => "Basque",
        "fa" => "Persian",
        "fi" => "Finnish",
        "fr" => "French",
        "ga" => "Irish",
        "gl" => "Galician",
        "gu" => "Gujarati",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "id" => "Indonesian",
        "is" => "Icelandic",
        "it" => "Italian",
        "kn" => "Kannada",
        "lt" => "Lithuanian",
        "lv" => "Latvian",
        "ml" => "Malayalam",
        "mr" => "Marathi",
        "ms" => "Malay",
        "nb" => "Norwegian Bokmål",
        "nl" => "Dutch",
        "nn" => "Norwegian Nynorsk",
        "no" => "Norwegian",
        "pa" => "Punjabi",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ro" => "Romanian",
        "ru" => "Russian",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sr" => "Serbian",
        "sv" => "Swedish",
        "sw" => "Swahili",
        "ta" => "Tamil",
        "te" => "Telugu",
        "th" => "Thai",
        "tr" => "Turkish",
        "uk" => "Ukrainian",
        "ur" => "Urdu",
        "vi" => "Vietnamese",
        _ => return None,
    })
}

/// Just enough of Enchant to list its dictionaries, which WebKit cannot do.
///
/// Opened at run time rather than linked: WebKitGTK has already mapped
/// `libenchant-2.so.2` into the process, so this takes one more reference to a
/// library that is there anyway, and the build needs no Enchant headers. A
/// broker of our own sees the same providers and dictionaries as WebKit's,
/// which is the point -- the menu offers exactly what WebKit can load.
mod enchant {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};

    use libloading::Library;

    type Broker = c_void;
    type Describe = unsafe extern "C" fn(
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        *mut c_void,
    );

    pub struct Enchant {
        lib: Library,
        broker: *mut Broker,
    }

    impl Enchant {
        pub fn open() -> Option<Self> {
            // SAFETY: Enchant's initialisers are safe to run more than once in
            // a process; WebKitGTK has already run them.
            let lib = match unsafe { Library::new("libenchant-2.so.2") } {
                Ok(lib) => lib,
                Err(e) => {
                    log::warn!("spellcheck: could not open Enchant: {e}");
                    return None;
                }
            };
            // SAFETY: the signature is Enchant 2's `enchant_broker_init`.
            let broker = unsafe {
                let init = lib
                    .get::<unsafe extern "C" fn() -> *mut Broker>(b"enchant_broker_init\0")
                    .ok()?;
                init()
            };
            if broker.is_null() {
                return None;
            }
            Some(Self { lib, broker })
        }

        /// Every dictionary tag Enchant can serve, as its providers name them.
        pub fn list(&self) -> Vec<String> {
            unsafe extern "C" fn collect(
                tag: *const c_char,
                _provider_name: *const c_char,
                _provider_desc: *const c_char,
                _provider_file: *const c_char,
                out: *mut c_void,
            ) {
                if tag.is_null() || out.is_null() {
                    return;
                }
                // SAFETY: `out` is the `Vec` passed below, alive for the call,
                // and `tag` is a NUL-terminated string Enchant owns.
                unsafe {
                    let out = &mut *(out as *mut Vec<String>);
                    out.push(CStr::from_ptr(tag).to_string_lossy().into_owned());
                }
            }

            let mut out: Vec<String> = Vec::new();
            // SAFETY: the signature is Enchant 2's `enchant_broker_list_dicts`;
            // the callback runs synchronously, before `out` goes out of scope.
            unsafe {
                let Ok(list) = self
                    .lib
                    .get::<unsafe extern "C" fn(*mut Broker, Describe, *mut c_void)>(
                        b"enchant_broker_list_dicts\0",
                    )
                else {
                    return out;
                };
                list(
                    self.broker,
                    collect,
                    &mut out as *mut Vec<String> as *mut c_void,
                );
            }
            out
        }

        /// Whether Enchant can serve `tag`, falling back as it does for WebKit
        /// (`en_IN` is answered from `en`).
        pub fn exists(&self, tag: &str) -> bool {
            let Ok(tag) = CString::new(tag) else {
                return false;
            };
            // SAFETY: the signature is Enchant 2's `enchant_broker_dict_exists`.
            unsafe {
                let Ok(exists) = self
                    .lib
                    .get::<unsafe extern "C" fn(*mut Broker, *const c_char) -> c_int>(
                        b"enchant_broker_dict_exists\0",
                    )
                else {
                    return false;
                };
                exists(self.broker, tag.as_ptr()) != 0
            }
        }
    }

    impl Drop for Enchant {
        fn drop(&mut self) {
            // SAFETY: the broker came from `enchant_broker_init` and is freed
            // once, here.
            unsafe {
                if let Ok(free) = self
                    .lib
                    .get::<unsafe extern "C" fn(*mut Broker)>(b"enchant_broker_free\0")
                {
                    free(self.broker);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

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

    #[test]
    fn keeps_a_bare_language_with_no_regional_form() {
        assert_eq!(languages(["de", "C"]), ["de", "en_US"]);
        // ...but not a bare English, which en_US already covers.
        assert_eq!(languages(["en", "C"]), ["en_US"]);
    }

    #[test]
    fn offers_installed_dictionaries_and_servable_system_languages() {
        let installed = strings(&["en", "en_US", "en_GB", "de_DE", "en_GB-ise"]);
        let system = strings(&["en_IN", "fr_FR", "en_US"]);
        let exists = |t: &str| t == "en_IN";
        assert_eq!(
            offered(&installed, exists, &system),
            ["de_DE", "en_GB", "en_IN", "en_US"]
        );
    }

    #[test]
    fn offers_a_bare_language_with_no_regional_form() {
        let installed = strings(&["he", "en_US"]);
        assert_eq!(offered(&installed, |_| false, &[]), ["en_US", "he"]);
    }

    #[test]
    fn toggling_adds_and_removes_in_pick_order() {
        let list = strings(&["en_US"]);
        let list = toggled(list, "de_DE").unwrap();
        assert_eq!(list, ["en_US", "de_DE"]);
        assert_eq!(toggled(list, "en_US").unwrap(), ["de_DE"]);
    }

    #[test]
    fn toggling_will_not_empty_the_list() {
        assert_eq!(toggled(strings(&["de_DE"]), "de_DE"), None);
    }

    #[test]
    fn labels_name_the_language_and_keep_the_code() {
        assert_eq!(label("en_GB"), "English (en_GB)");
        assert_eq!(label("he"), "Hebrew (he)");
        assert_eq!(label("xx_YY"), "xx_YY");
    }
}
