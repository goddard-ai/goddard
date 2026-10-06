use std::borrow::Cow;

use anyhow::Result;
use gpui::{App, AssetSource, SharedString};

/// Icons embedded in the binary so the app stays a single artifact.
pub struct Assets;

macro_rules! icons {
    ($($name:literal),+ $(,)?) => {
        &[$((
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice(),
        )),+]
    };
}

const ICONS: &[(&str, &[u8])] = icons![
    "alert",
    "appearance",
    "archive",
    "asterisk",
    "automations",
    "beaker",
    "arrow-down",
    "arrow-left",
    "arrow-right",
    "arrow-up",
    "arrow-up-right",
    "ban",
    "battery-low",
    "bell",
    "block",
    "book-open",
    "bot",
    "brain",
    "broom",
    "bug",
    "case-sensitive",
    "chart-column",
    "chat",
    "check",
    "changes",
    "circle-alert",
    "circle-check",
    "circle-dot",
    "circle-help",
    "cloud-upload",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "chevrons-up-down",
    "coffee",
    "command",
    "compass",
    "compose",
    "container",
    "contrast",
    "copy",
    "corner-down-right",
    "cursor-spark",
    "database",
    "dock-archive",
    "dock-ind-glyph-left",
    "dock-ind-glyph-mid",
    "dock-ind-glyph-right",
    "dock-keyboard",
    "download",
    "ellipsis",
    "ellipsis-vertical",
    "eye",
    "eye-off",
    "external-link",
    "file",
    "folder",
    "folder-clock",
    "folder-new",
    "folder-open",
    "folder-search",
    "file-bottom-left-arrow",
    "file-diff",
    "file-text",
    "file-types/angular",
    "file-types/audio",
    "file-types/astro",
    "file-types/babel",
    "file-types/biome",
    "file-types/bun",
    "file-types/c",
    "file-types/certificate",
    "file-types/clojure",
    "file-types/cmake",
    "file-types/coffee",
    "file-types/console",
    "file-types/cpp",
    "file-types/crystal",
    "file-types/csharp",
    "file-types/css",
    "file-types/dart",
    "file-types/database",
    "file-types/deno",
    "file-types/diff",
    "file-types/docker",
    "file-types/editorconfig",
    "file-types/elixir",
    "file-types/elm",
    "file-types/erlang",
    "file-types/eslint",
    "file-types/exe",
    "file-types/file",
    "file-types/firebase",
    "file-types/git",
    "file-types/gitlab",
    "file-types/go",
    "file-types/gradle",
    "file-types/graphql",
    "file-types/haskell",
    "file-types/haxe",
    "file-types/helm",
    "file-types/html",
    "file-types/image",
    "file-types/java",
    "file-types/javascript",
    "file-types/jinja",
    "file-types/json",
    "file-types/julia",
    "file-types/kotlin",
    "file-types/kubernetes",
    "file-types/lock",
    "file-types/lua",
    "file-types/makefile",
    "file-types/markdown",
    "file-types/nest",
    "file-types/next",
    "file-types/nginx",
    "file-types/nix",
    "file-types/nodejs",
    "file-types/npm",
    "file-types/nuxt",
    "file-types/ocaml",
    "file-types/pdf",
    "file-types/perl",
    "file-types/php",
    "file-types/pnpm",
    "file-types/powershell",
    "file-types/prettier",
    "file-types/prisma",
    "file-types/proto",
    "file-types/pug",
    "file-types/python",
    "file-types/react",
    "file-types/readme",
    "file-types/rollup",
    "file-types/ruby",
    "file-types/rust",
    "file-types/sass",
    "file-types/scala",
    "file-types/settings",
    "file-types/solidity",
    "file-types/storybook",
    "file-types/stylelint",
    "file-types/supabase",
    "file-types/svelte",
    "file-types/svg",
    "file-types/swift",
    "file-types/tailwindcss",
    "file-types/terraform",
    "file-types/tex",
    "file-types/turborepo",
    "file-types/typescript",
    "file-types/video",
    "file-types/vite",
    "file-types/vitest",
    "file-types/vue",
    "file-types/webassembly",
    "file-types/webpack",
    "file-types/xaml",
    "file-types/xml",
    "file-types/yaml",
    "file-types/yarn",
    "file-types/zig",
    "file-types/zip",
    "fork",
    "friends",
    "gauge",
    "git-branch",
    "git-commit-horizontal",
    "git-merge",
    "git-pull-request",
    "git-pull-request-closed",
    "git-pull-request-draft",
    "goddard-logo",
    "goddard-thinking-ear",
    "goddard-thinking-ear-back",
    "goddard-thinking-eyes",
    "goddard-thinking-face",
    "globe",
    "github",
    "goal",
    "hammer",
    "hand",
    "hat-glasses",
    "headphones",
    "rotate-ccw",
    "fast-forward",
    "hourglass",
    "hexagon",
    "inbox",
    "inbox-sidebar",
    "info",
    "integration-atlassian",
    "integration-figma",
    "integration-github",
    "integration-linear",
    "integration-monday",
    "integration-notion",
    "integration-sentry",
    "integration-stripe",
    "integration-supabase",
    "integration-vercel",
    "key-round",
    "keyboard",
    "languages",
    "laptop",
    "link",
    "list",
    "list-filter",
    "loader-circle",
    "local",
    "lock",
    "lock-open",
    "map",
    "maximize",
    "message-square",
    "mic",
    "minimize",
    "minus",
    "monitor",
    "moon",
    "octagon-alert",
    "octagon-x",
    "package",
    "panel-left",
    "panel-right",
    "paperclip",
    "pause",
    "pencil",
    "pin",
    "pin-filled",
    "pin-off",
    "play",
    "plus",
    "power",
    "provider-amp",
    "provider-antigravity",
    "provider-claude",
    "provider-cloudflare",
    "provider-copilot",
    "provider-cursor",
    "provider-deepseek",
    "provider-devin",
    "provider-droid",
    "provider-fx",
    "provider-goose",
    "provider-grok",
    "provider-goose",
    "provider-kimi",
    "provider-muse",
    "provider-openai",
    "provider-ohmypi",
    "provider-opencode",
    "provider-openrouter",
    "provider-pi",
    "provider-typesafe",
    "provider-typesafe-padded",
    "projects",
    "qr-code",
    "queue",
    "regex",
    "replace",
    "replace-all",
    "rewind",
    "rotate-cw",
    "search",
    "send",
    "send-chrome-arrow",
    "send-chrome-underlay",
    "server",
    "settings",
    "settings-hexagon",
    "sigma",
    "slash",
    "sparkle",
    "star",
    "star-filled",
    "square",
    "stop",
    "stop-filled",
    "sun",
    "target",
    "terminal",
    "terminal-prompt",
    "terminal-square",
    "trash",
    "type",
    "unplug",
    "user-round",
    "volume-2",
    "whole-word",
    "wifi",
    "wrench",
    "window-maximize",
    "window-minimize",
    "window-restore",
    "x",
    "x-bold",
    "zap",
];

/// Raster art embedded the same way — anything `img()` loads that is not a
/// monochrome icon.
const IMAGES: &[(&str, &[u8])] = &[
    (
        "images/dock-button-bkg.webp",
        include_bytes!("../assets/images/dock-button-bkg.webp").as_slice(),
    ),
    (
        "images/dock-ind-orb-left.webp",
        include_bytes!("../assets/images/dock-ind-orb-left.webp").as_slice(),
    ),
    (
        "images/dock-ind-orb-mid.webp",
        include_bytes!("../assets/images/dock-ind-orb-mid.webp").as_slice(),
    ),
    (
        "images/dock-ind-orb-right.webp",
        include_bytes!("../assets/images/dock-ind-orb-right.webp").as_slice(),
    ),
    (
        "images/send-button-chrome.webp",
        include_bytes!("../assets/images/send-button-chrome.webp").as_slice(),
    ),
];

const TEXT_FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
];

/// Symbols-only icon face resolved via CoreText cascade (`FontFallbacks`),
/// never as a primary GPUI family; see `register_fonts_with_coretext`.
const SYMBOLS_FONT: &[u8] = include_bytes!("../assets/fonts/SymbolsNerdFontMono-Regular.ttf");

/// Family name of [`SYMBOLS_FONT`] for `FontFallbacks` lists.
pub const SYMBOLS_FONT_FAMILY: &str = "Symbols Nerd Font Mono";

pub fn register_fonts(cx: &App) -> Result<()> {
    cx.text_system().add_fonts(
        TEXT_FONTS
            .iter()
            .map(|font| Cow::Borrowed(*font))
            .collect::<Vec<_>>(),
    )?;
    crate::platform::register_fonts_with_coretext(&[SYMBOLS_FONT])
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .chain(IMAGES)
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .chain(IMAGES)
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Every `"icons/…svg"` literal in the workspace's source must resolve
    /// through the embedded `AssetSource` — an SVG left out of `icons!`
    /// renders blank with no error anywhere.
    #[test]
    fn every_referenced_icon_is_embedded() {
        use crate::persistence::CustomCommandIcon;

        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        rust_sources(&manifest.join("src"), &mut files);
        // Workspace crates name the same embedded set — e.g. the `icon()`
        // methods on waku-protocol's enums — so their literals count too.
        for entry in std::fs::read_dir(manifest.join("crates")).unwrap() {
            let src = entry.unwrap().path().join("src");
            if src.is_dir() {
                rust_sources(&src, &mut files);
            }
        }
        let mut missing = Vec::new();
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            for (start, _) in source.match_indices("\"icons/") {
                let rest = &source[start + 1..];
                let Some(end) = rest.find('"') else { continue };
                let path = &rest[..end];
                if !path.ends_with(".svg") || path.contains(char::is_whitespace) {
                    continue;
                }
                if Assets.load(path).unwrap().is_none() {
                    missing.push(format!("{path} (referenced by {})", file.display()));
                }
            }
        }

        // Vocabularies that resolve an icon *name* to a path are checked by
        // enumeration too: every `CustomCommandIcon` variant — the
        // persona/employee job icons, whose snake_case wire names map to
        // kebab-case files through `icon_path` — plus every icon the
        // job-title classifier can emit.
        for icon in CustomCommandIcon::ALL {
            let path = crate::custom_commands::icon_path(icon);
            if Assets.load(path).unwrap().is_none() {
                missing.push(format!("{path} (CustomCommandIcon::{icon:?})"));
            }
        }
        for path in crate::app::job_title_icon_paths() {
            if Assets.load(path).unwrap().is_none() {
                missing.push(format!("{path} (job-title icon)"));
            }
        }

        assert!(
            missing.is_empty(),
            "icon paths referenced but not embedded:\n{}",
            missing.join("\n")
        );
    }
}
