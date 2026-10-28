use dioxus::prelude::*;

/// `app_settings` key that stores the selected theme.
pub const THEME_SETTING_KEY: &str = "ui_theme";

const BASE_STYLES: &str = concat!(
    include_str!("../ui-mockups/styles.css"),
    "\n",
    include_str!("../assets/themes/default.css")
);
const GNOME_STYLES: &str = include_str!("../assets/themes/gnome.css");

/// Available application themes. `Dark` is the look the app shipped with;
/// `Gnome` follows the Adwaita light palette used by current GNOME releases.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Gnome,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Dark, Theme::Gnome];

    pub fn key(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Gnome => "gnome",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Gnome => "GNOME",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Theme::Dark => "The original slate-blue interface with glass panels",
            Theme::Gnome => "Light Adwaita surfaces, GNOME blue accent and typography",
        }
    }

    /// Unknown or missing values fall back to the default theme, so a database
    /// written by a newer build never leaves the window unstyled.
    pub fn from_key(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "gnome" => Theme::Gnome,
            _ => Theme::Dark,
        }
    }
}

/// Injects the shared stylesheet and, when selected, the GNOME light overrides.
///
/// The dark theme needs no extra CSS: its colours live in the base stylesheets.
#[component]
pub fn ThemeStyles(theme: Theme) -> Element {
    let solid_font = asset!("/assets/fontawesome/webfonts/fa-solid-900.woff2");
    let regular_font = asset!("/assets/fontawesome/webfonts/fa-regular-400.woff2");
    let brands_font = asset!("/assets/fontawesome/webfonts/fa-brands-400.woff2");
    let compatibility_font = asset!("/assets/fontawesome/webfonts/fa-v4compatibility.woff2");
    let theme_styles = if theme == Theme::Gnome {
        GNOME_STYLES
    } else {
        ""
    };

    rsx! {
        document::Link {
            rel: "stylesheet",
            href: asset!("/assets/fontawesome/css/all.min.css"),
        }
        document::Link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&display=swap",
        }
        style { "{BASE_STYLES}" }
        style { "{theme_styles}" }
        style {
            "@font-face {{ font-family: 'Font Awesome 6 Free'; font-style: normal; font-weight: 400; font-display: block; src: url('{regular_font}') format('woff2'); }}
            @font-face {{ font-family: 'Font Awesome 6 Free'; font-style: normal; font-weight: 900; font-display: block; src: url('{solid_font}') format('woff2'); }}
            @font-face {{ font-family: 'Font Awesome 6 Brands'; font-style: normal; font-weight: 400; font-display: block; src: url('{brands_font}') format('woff2'); }}
            @font-face {{ font-family: 'FontAwesome'; font-style: normal; font-weight: 400; font-display: block; src: url('{compatibility_font}') format('woff2'); }}"
        }
    }
}
