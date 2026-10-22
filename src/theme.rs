use dioxus::prelude::*;

const STYLES: &str = concat!(
    include_str!("../ui-mockups/styles.css"),
    "\n",
    include_str!("../assets/themes/default.css")
);

#[component]
pub fn ThemeStyles() -> Element {
    let solid_font = asset!("/assets/fontawesome/webfonts/fa-solid-900.woff2");
    let regular_font = asset!("/assets/fontawesome/webfonts/fa-regular-400.woff2");
    let brands_font = asset!("/assets/fontawesome/webfonts/fa-brands-400.woff2");
    let compatibility_font = asset!("/assets/fontawesome/webfonts/fa-v4compatibility.woff2");

    rsx! {
        document::Link {
            rel: "stylesheet",
            href: asset!("/assets/fontawesome/css/all.min.css"),
        }
        document::Link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500;600;700&display=swap",
        }
        style { "{STYLES}" }
        style {
            "@font-face {{ font-family: 'Font Awesome 6 Free'; font-style: normal; font-weight: 400; font-display: block; src: url('{regular_font}') format('woff2'); }}
            @font-face {{ font-family: 'Font Awesome 6 Free'; font-style: normal; font-weight: 900; font-display: block; src: url('{solid_font}') format('woff2'); }}
            @font-face {{ font-family: 'Font Awesome 6 Brands'; font-style: normal; font-weight: 400; font-display: block; src: url('{brands_font}') format('woff2'); }}
            @font-face {{ font-family: 'FontAwesome'; font-style: normal; font-weight: 400; font-display: block; src: url('{compatibility_font}') format('woff2'); }}"
        }
    }
}
