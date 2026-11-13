//! Follow the UI font chosen in NextUI-CN's settings (`minuisettings.txt`).
//!
//! Mirrors `CFG_init` / `CFG_resolveFontPath` in NextUI-CN `common/config.c`:
//! - `font=<file>` names a font in `.system/res` (`font1.ttf` 梦源黑体 is the default,
//!   `font2.ttf` 缝合像素, `font3.ttf` 寒蝉半圆体);
//! - `font=custom:<file>` loads `<SD>/Fonts/<file>`;
//! - legacy integer ids: with the CN marker `fontOrder=2`, `0/1/2` map to
//!   `font1/font2/font3.ttf`; without it, upstream's `0 -> font2.ttf`, `1 -> font1.ttf`
//!   (and `2 -> font3.ttf`).

use std::path::{Path, PathBuf};

pub const DEFAULT_FONT_FILE: &str = "font1.ttf";
const CUSTOM_FONT_PREFIX: &str = "custom:";
const CUSTOM_FONT_DIR: &str = "Fonts";

/// The font file setting (`font1.ttf`, `custom:foo.ttf`, ...) from the settings text.
pub fn selected_font_file(settings: &str) -> String {
    let mut font_order = 0;
    let mut font: Option<&str> = None;
    for line in settings.lines() {
        if let Some(v) = line.strip_prefix("fontOrder=") {
            font_order = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("font=") {
            font = Some(v.trim_end_matches(['\r', '\n']));
        }
    }
    // Legacy integer ids: CN order (fontOrder=2) vs upstream order
    let legacy: &[&str] = if font_order >= 2 {
        &["font1.ttf", "font2.ttf", "font3.ttf"]
    } else {
        &["font2.ttf", "font1.ttf", "font3.ttf"]
    };
    let file = match font {
        None | Some("") => DEFAULT_FONT_FILE,
        Some(id @ ("0" | "1" | "2")) => legacy[id.parse::<usize>().unwrap_or(0)],
        Some(other) => other,
    };
    file.to_string()
}

/// Full path of a font setting, or `None` if it is not a plain file name.
pub fn resolve_font_path(sd_root: &Path, font_file: &str) -> Option<PathBuf> {
    if font_file.is_empty() {
        return None;
    }
    if let Some(name) = font_file.strip_prefix(CUSTOM_FONT_PREFIX) {
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return None;
        }
        return Some(sd_root.join(CUSTOM_FONT_DIR).join(name));
    }
    if font_file.contains('/') {
        return None;
    }
    Some(sd_root.join(".system/res").join(font_file))
}

/// Can egui (`ab_glyph`) actually use these bytes as a font?
pub fn is_usable_font(bytes: &[u8]) -> bool {
    ab_glyph::FontRef::try_from_slice(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_setting_mapping() {
        assert_eq!(selected_font_file(""), "font1.ttf");
        assert_eq!(selected_font_file("font=font3.ttf\n"), "font3.ttf");
        assert_eq!(
            selected_font_file("font=custom:My Font.ttf\r\n"),
            "custom:My Font.ttf"
        );
        // CN legacy ids
        assert_eq!(selected_font_file("font=0\nfontOrder=2\n"), "font1.ttf");
        assert_eq!(selected_font_file("fontOrder=2\nfont=1\n"), "font2.ttf");
        assert_eq!(selected_font_file("fontOrder=2\nfont=2\n"), "font3.ttf");
        // upstream legacy ids
        assert_eq!(selected_font_file("font=0\n"), "font2.ttf");
        assert_eq!(selected_font_file("font=1\nfontStyle=1\n"), "font1.ttf");
        // other keys that merely contain "font=" are ignored
        assert_eq!(selected_font_file("myfont=font3.ttf\n"), "font1.ttf");
    }

    #[test]
    fn font_path_resolution() {
        let sd = Path::new("/mnt/SDCARD");
        assert_eq!(
            resolve_font_path(sd, "font3.ttf").unwrap(),
            Path::new("/mnt/SDCARD/.system/res/font3.ttf")
        );
        assert_eq!(
            resolve_font_path(sd, "custom:a b.ttf").unwrap(),
            Path::new("/mnt/SDCARD/Fonts/a b.ttf")
        );
        assert!(resolve_font_path(sd, "custom:../x.ttf").is_none());
        assert!(resolve_font_path(sd, "custom:..").is_none());
        assert!(resolve_font_path(sd, "custom:").is_none());
        assert!(resolve_font_path(sd, "../x.ttf").is_none());
    }

    #[test]
    fn garbage_is_not_a_font() {
        assert!(!is_usable_font(b"definitely not a font"));
        assert!(!is_usable_font(&[]));
    }
}
