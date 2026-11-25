//! Embedded Studio assets. Everything is original, compiled into the binary,
//! and served from loopback only. No CDN, no external fonts, no network.

pub const INDEX: &str = include_str!("../../../assets/index.html");
pub const CSS: &str = include_str!("../../../assets/app.css");
pub const JS: &str = include_str!("../../../assets/app.js");
pub const FAVICON: &str = include_str!("../../../assets/favicon.svg");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_embedded_and_offline() {
        assert!(INDEX.contains("Anvil"));
        assert!(INDEX.contains("app.js"));
        assert!(CSS.len() > 1000);
        assert!(JS.len() > 5000);
        assert!(FAVICON.contains("svg"));
        for text in [INDEX, CSS, JS] {
            assert!(!text.contains("http://"));
            assert!(!text.contains("https://"));
        }
        // The SVG namespace is an identifier, not a network fetch. Reject any
        // actual linked or embedded remote asset.
        assert!(!FAVICON.contains("href=\"http"));
        assert!(!FAVICON.contains("url(http"));
    }
}
