use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{
    App, Global, Hsla, Pixels, Rems, Window, WindowAppearance, hsla, px, rems, rgb,
    transparent_black,
};

pub use waku_client::theme::{ThemeMode, ThemeName, ThemeSettings};

/// Scaled pixels: a dimension authored at the default 14px UI font size,
/// expressed in rems so the UI font size setting scales it. The window's rem
/// size *is* the UI font size, so at the default setting this resolves to
/// exactly the authored pixel value.
///
/// Chrome text sizes and their line heights go through here. Content surfaces
/// that already derive from a font-size setting — markdown metrics, the file
/// editor, diff rows, tool-output mono — stay in `px` so they never scale
/// twice.
pub fn sp(value: f32) -> Rems {
    rems(value / waku_client::persistence::DEFAULT_UI_FONT_SIZE)
}

/// Whether borders draw a full pixel instead of the default hairline. `Styled`
/// border methods run inside builder chains with no `cx`, so the setting
/// reaches them through this static — written once at startup and again when
/// the toggle flips, read on every frame after.
static THICK_BORDERS: AtomicBool = AtomicBool::new(false);

/// The app-standard border width: a half-pixel hairline, or a full pixel while
/// the "Thick borders" setting is on.
pub fn hairline() -> Pixels {
    px(if THICK_BORDERS.load(Ordering::Relaxed) {
        1.0
    } else {
        0.5
    })
}

/// Store the "Thick borders" preference [`hairline`] reads.
pub fn set_thick_borders(enabled: bool) {
    THICK_BORDERS.store(enabled, Ordering::Relaxed);
}

/// A translucent color wash — selection, etc. `rgb()` yields `Rgba`; this
/// hops through `Hsla` so the alpha can be set.
fn wash(color: u32, alpha: f32) -> Hsla {
    let color: Hsla = rgb(color).into();
    color.opacity(alpha)
}

fn native_override(settings: ThemeSettings) -> Option<bool> {
    match settings.mode {
        ThemeMode::System => None,
        _ => Some(theme_for(settings, false).is_dark),
    }
}

/// Foreground colors for `md::highlight` token classes. `Added`/`Removed` stay
/// bound to `success`/`danger` — diffs are semantic, not stylistic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntaxColors {
    pub keyword: Hsla,
    /// Language-level constant: `true`, `nil`, `None`.
    pub literal: Hsla,
    pub string: Hsla,
    pub comment: Hsla,
    pub number: Hsla,
    pub ty: Hsla,
    pub function: Hsla,
    /// `@decorator`, `#[attribute]`, preprocessor lines, `$variable`.
    pub meta: Hsla,
}

/// Goddard's visual language, take two: neutral graphite surfaces in the spirit
/// of Cursor — color is reserved for meaning. On macOS the sidebar's semantic
/// tint is installed as a native layer above Sidebar vibrancy; keeping this
/// GPUI surface clear avoids incorrectly accumulating the alpha of nested Metal
/// backgrounds. Selected, hovered, and pressed rows remain a 6% neutral layer.
///
/// Named schemes (Gruvbox, Everforest, Kanagawa, Zenburn) keep the same
/// semantic contract — the struct doesn't know which palette it carries.
#[derive(Clone, Copy)]
pub struct Theme {
    pub is_dark: bool,
    pub canvas: Hsla,
    pub sidebar: Hsla,
    pub sidebar_drag_background: Hsla,
    pub sidebar_item_background: Hsla,
    pub surface: Hsla,
    pub raised: Hsla,
    pub composer: Hsla,
    pub inset: Hsla,
    /// Terminal screen surface: paper-white in light mode, near-black in dark.
    pub terminal: Hsla,
    pub overlay: Hsla,
    pub overlay_strong: Hsla,

    pub border: Hsla,
    pub border_strong: Hsla,
    pub sidebar_border: Hsla,

    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,
    pub text_ghost: Hsla,

    /// Accent color. Brand coral in the default palettes; each named scheme
    /// owns its own (Gruvbox orange, Kanagawa blue, …).
    pub accent: Hsla,
    pub resize_handle: Hsla,
    /// Meter fills in the usage panel. Quota-meter blue by convention;
    /// warning/danger take over as a lane fills.
    pub gauge: Hsla,

    /// Text-selection wash. Painted *under* the glyphs, so it stays
    /// translucent. The default palettes keep the familiar browser blue;
    /// named schemes use their own visual-selection color.
    pub selection: Hsla,
    /// Inline `code` foreground and its rounded wash.
    pub code_text: Hsla,
    pub code_wash: Hsla,

    /// Light fill for primary buttons (send, allow), dark glyph on top.
    pub inverse: Hsla,
    pub on_inverse: Hsla,

    /// Informational blue for "settled, not yet seen" markers — the sidebar's
    /// finished-turn dot. Kept apart from `accent` so an unread state never
    /// reads as live work.
    pub info: Hsla,
    pub warning: Hsla,
    pub success: Hsla,
    pub favorite: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,

    pub syntax: SyntaxColors,
    /// The 16-color terminal table (indices 0-15). Indices 16-255 stay on the
    /// standard 6×6×6 cube and grayscale ramp.
    pub ansi: [u32; 16],
}

/// The hand-picked part of a palette. [`Theme::from_spec`] derives the
/// translucent layers — hover washes, borders, code wash, danger soft — from
/// `neutral`, which must be the scheme's own gray so the tints land on-tint
/// even for warm schemes like Gruvbox.
struct ThemeSpec {
    is_dark: bool,
    canvas: u32,
    /// Solid sidebar fill: non-macOS, transparency-off, and the native
    /// vibrancy tint source.
    sidebar_solid: u32,
    surface: u32,
    raised: u32,
    composer: u32,
    inset: u32,
    terminal: u32,
    sidebar_border: Hsla,

    /// The scheme's mid gray; drives every alpha layer.
    neutral: Hsla,

    text: u32,
    text_secondary: u32,
    text_tertiary: u32,
    text_ghost: u32,

    accent: u32,
    selection: Hsla,
    code_text: u32,

    inverse: u32,
    on_inverse: u32,

    info: u32,
    warning: u32,
    success: u32,
    favorite: u32,
    danger: u32,

    syntax: SyntaxColors,
    ansi: [u32; 16],
}

impl Theme {
    /// The Big Picture backdrop: one step off the window surface — lifted
    /// toward `raised` in dark themes, sunken toward `inset` in light ones —
    /// painted translucent over the blurred frame snapshot so the workspace
    /// still ghosts through.
    pub fn backdrop(&self) -> Hsla {
        if self.is_dark { self.raised } else { self.inset }
    }

    pub fn current(cx: &App) -> Self {
        if cx.has_global::<ActiveWakuTheme>() {
            cx.global::<ActiveWakuTheme>().0
        } else {
            Self::dark()
        }
    }

    fn from_spec(spec: ThemeSpec) -> Self {
        let neutral = spec.neutral;
        // Wash strengths carried over from the graphite palettes — kept as two
        // polarity sets so light themes darken and dark themes lighten.
        let (border_a, border_strong_a, code_wash_a) = if spec.is_dark {
            (0.07, 0.14, 0.08)
        } else {
            (0.08, 0.15, 0.07)
        };
        let danger: Hsla = rgb(spec.danger).into();
        Self {
            is_dark: spec.is_dark,
            canvas: rgb(spec.canvas).into(),
            sidebar: if cfg!(target_os = "macos") {
                transparent_black()
            } else {
                rgb(spec.sidebar_solid).into()
            },
            sidebar_drag_background: rgb(spec.sidebar_solid).into(),
            sidebar_item_background: neutral.opacity(0.06),
            surface: rgb(spec.surface).into(),
            raised: rgb(spec.raised).into(),
            composer: rgb(spec.composer).into(),
            inset: rgb(spec.inset).into(),
            terminal: rgb(spec.terminal).into(),
            overlay: neutral.opacity(0.05),
            overlay_strong: neutral.opacity(0.09),

            border: neutral.opacity(border_a),
            border_strong: neutral.opacity(border_strong_a),
            sidebar_border: spec.sidebar_border,

            text: rgb(spec.text).into(),
            text_secondary: rgb(spec.text_secondary).into(),
            text_tertiary: rgb(spec.text_tertiary).into(),
            text_ghost: rgb(spec.text_ghost).into(),

            accent: rgb(spec.accent).into(),
            resize_handle: rgb(spec.info).into(),
            gauge: rgb(spec.info).into(),

            selection: spec.selection,
            code_text: rgb(spec.code_text).into(),
            code_wash: neutral.opacity(code_wash_a),

            inverse: rgb(spec.inverse).into(),
            on_inverse: rgb(spec.on_inverse).into(),

            info: rgb(spec.info).into(),
            warning: rgb(spec.warning).into(),
            success: rgb(spec.success).into(),
            favorite: rgb(spec.favorite).into(),
            danger,
            danger_soft: danger.opacity(0.10),

            syntax: spec.syntax,
            ansi: spec.ansi,
        }
    }

    /// The default dark palette — neutral graphite, color reserved for
    /// meaning. Terminal table is Tomorrow Night, syntax hues are Goddard's
    /// restrained set.
    pub fn dark() -> Self {
        let slate = hsla(220.0 / 360.0, 0.10, 0.90, 1.0);
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x1A1A1A,
            sidebar_solid: 0x181818,
            surface: 0x1A1A1A,
            raised: 0x232323,
            composer: 0x212121,
            inset: 0x151515,
            terminal: 0x151515,
            sidebar_border: hsla(126.93 / 360.0, 0.000_000_1, 0.16077, 1.0),

            neutral: slate,

            text: 0xE2E2E2,
            text_secondary: 0xA3A3A3,
            text_tertiary: 0x7D7D7D,
            text_ghost: 0x575757,

            accent: 0xE2795B,
            selection: hsla(211.0 / 360.0, 1.0, 0.50, 0.55),
            code_text: 0xE0A882,

            inverse: 0xE7E9EC,
            on_inverse: 0x17181C,

            info: 0x3B82F6,
            warning: 0xE0B36A,
            success: 0x62C987,
            favorite: 0xEAB308,
            danger: 0xE2726A,

            syntax: SyntaxColors {
                keyword: rgb(0xC98BC0).into(),
                literal: rgb(0xD9A05B).into(),
                string: rgb(0x94C08A).into(),
                comment: rgb(0x575757).into(),
                number: rgb(0xD9A05B).into(),
                ty: rgb(0x8FB8D9).into(),
                function: rgb(0x8FB8D9).into(),
                meta: rgb(0x7D7D7D).into(),
            },
            // Tomorrow Night.
            ansi: [
                0x1d1f21, 0xcc6666, 0xb5bd68, 0xf0c674, 0x81a2be, 0xb294bb, 0x8abeb7, 0xc5c8c6,
                0x666666, 0xd54e53, 0xb9ca4a, 0xe7c547, 0x7aa6da, 0xc397d8, 0x70c0b1, 0xeaeaea,
            ],
        })
    }

    /// The default light palette — graphite's mirror image, Tomorrow on the
    /// terminal.
    pub fn light() -> Self {
        let slate = hsla(220.0 / 360.0, 0.10, 0.12, 1.0);
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xF6F5F6,
            sidebar_solid: 0xF3F3F3,
            surface: 0xF6F5F6,
            raised: 0xECECEC,
            composer: 0xFFFFFF,
            inset: 0xE6E6E6,
            terminal: 0xFFFFFF,
            sidebar_border: hsla(0.0, 0.0, 0.078, 0.12),

            neutral: slate,

            text: 0x242424,
            text_secondary: 0x666666,
            text_tertiary: 0x858585,
            text_ghost: 0xA4A4A4,

            accent: 0x369BDC,
            selection: hsla(211.0 / 360.0, 1.0, 0.50, 0.35),
            code_text: 0x1E6FB4,

            inverse: 0x202227,
            on_inverse: 0xF8F8F9,

            info: 0x2563EB,
            warning: 0xA66B20,
            success: 0x2F8F52,
            favorite: 0xCA8A04,
            danger: 0xC64A42,

            syntax: SyntaxColors {
                keyword: rgb(0x9A4B92).into(),
                literal: rgb(0x9A6019).into(),
                string: rgb(0x3F7A36).into(),
                comment: rgb(0xA4A4A4).into(),
                number: rgb(0x9A6019).into(),
                ty: rgb(0x2F6690).into(),
                function: rgb(0x2F6690).into(),
                meta: rgb(0x858585).into(),
            },
            // Tomorrow.
            ansi: [
                0x000000, 0xc82829, 0x718c00, 0xeab700, 0x4271ae, 0x8959a8, 0x3e999f, 0xc7c7c7,
                0x8e908c, 0xc82829, 0x718c00, 0xeab700, 0x4271ae, 0x8959a8, 0x3e999f, 0xffffff,
            ],
        })
    }

    /// Gruvbox dark, medium contrast — morhetz/gruvbox `colors/gruvbox.vim`.
    /// Statements are red, functions green, constants purple, types yellow;
    /// the terminal table is the file's own `g:terminal_color_*` mapping.
    pub fn gruvbox_dark() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x282828,
            sidebar_solid: 0x32302F,
            surface: 0x282828,
            raised: 0x3C3836,
            composer: 0x32302F,
            inset: 0x1D2021,
            terminal: 0x282828,
            sidebar_border: rgb(0x3C3836).into(),

            neutral: rgb(0x928374).into(),

            text: 0xEBDBB2,
            text_secondary: 0xD5C4A1,
            text_tertiary: 0xA89984,
            text_ghost: 0x928374,

            accent: 0xFE8019,
            selection: wash(0x83A598, 0.40),
            code_text: 0xFE8019,

            inverse: 0xEBDBB2,
            on_inverse: 0x282828,

            info: 0x83A598,
            warning: 0xFABD2F,
            success: 0xB8BB26,
            favorite: 0xFABD2F,
            danger: 0xFB4934,

            syntax: SyntaxColors {
                keyword: rgb(0xFB4934).into(),  // Statement
                literal: rgb(0xD3869B).into(),  // Constant
                string: rgb(0xB8BB26).into(),   // String
                comment: rgb(0x928374).into(),  // Comment
                number: rgb(0xD3869B).into(),   // Number
                ty: rgb(0xFABD2F).into(),       // Type
                function: rgb(0xB8BB26).into(), // Function (green, bold upstream)
                meta: rgb(0x8EC07C).into(),     // PreProc
            },
            ansi: [
                0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984,
                0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
            ],
        })
    }

    /// Gruvbox light, hard contrast — `#f9f5d7` paper. Bright terminal slots
    /// are the faded set, per upstream's light-mode mapping.
    pub fn gruvbox_light_hard() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xF9F5D7,
            sidebar_solid: 0xF2E5BC,
            surface: 0xF9F5D7,
            raised: 0xF2E5BC,
            composer: 0xFBF1C7,
            inset: 0xEBDBB2,
            terminal: 0xFBF1C7,
            sidebar_border: rgb(0xD5C4A1).into(),

            neutral: rgb(0x928374).into(),

            text: 0x3C3836,
            text_secondary: 0x665C54,
            text_tertiary: 0x7C6F64,
            text_ghost: 0x928374,

            accent: 0xD65D0E,
            selection: wash(0x458588, 0.30),
            code_text: 0xAF3A03,

            inverse: 0x3C3836,
            on_inverse: 0xF9F5D7,

            info: 0x076678,
            warning: 0xB57614,
            success: 0x79740E,
            favorite: 0xD79921,
            danger: 0x9D0006,

            syntax: SyntaxColors {
                keyword: rgb(0x9D0006).into(),
                literal: rgb(0x8F3F71).into(),
                string: rgb(0x79740E).into(),
                comment: rgb(0x928374).into(),
                number: rgb(0x8F3F71).into(),
                ty: rgb(0xB57614).into(),
                function: rgb(0x79740E).into(),
                meta: rgb(0x427B58).into(),
            },
            ansi: [
                0xf9f5d7, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0x7c6f64,
                0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0x3c3836,
            ],
        })
    }

    /// Everforest dark, medium contrast — sainnhe/everforest `palette.md`.
    /// Keywords red, functions and strings green, constants aqua, numbers
    /// purple, types yellow; ANSI is the official alacritty port.
    pub fn everforest_dark() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x2D353B,
            sidebar_solid: 0x232A2E,
            surface: 0x2D353B,
            raised: 0x3D484D,
            composer: 0x343F44,
            inset: 0x232A2E,
            terminal: 0x2D353B,
            sidebar_border: rgb(0x3D484D).into(),

            neutral: rgb(0x859289).into(),

            text: 0xD3C6AA,
            text_secondary: 0x9DA9A0,
            text_tertiary: 0x859289,
            text_ghost: 0x7A8478,

            accent: 0xA7C080,
            selection: wash(0x543A48, 0.75),
            code_text: 0xE69875,

            inverse: 0xD3C6AA,
            on_inverse: 0x2D353B,

            info: 0x7FBBB3,
            warning: 0xDBBC7F,
            success: 0xA7C080,
            favorite: 0xDBBC7F,
            danger: 0xE67E80,

            syntax: SyntaxColors {
                keyword: rgb(0xE67E80).into(),  // red
                literal: rgb(0x83C092).into(),  // aqua — constants
                string: rgb(0xA7C080).into(),   // green
                comment: rgb(0x859289).into(),  // grey1
                number: rgb(0xD699B6).into(),   // purple
                ty: rgb(0xDBBC7F).into(),       // yellow
                function: rgb(0xA7C080).into(), // green — same as string upstream
                meta: rgb(0xD699B6).into(),     // purple — preprocessors
            },
            ansi: [
                0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
                0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
            ],
        })
    }

    /// Everforest light, medium contrast — `#fdf6e3` paper.
    pub fn everforest_light() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xFDF6E3,
            sidebar_solid: 0xEFEBD4,
            surface: 0xFDF6E3,
            raised: 0xF4F0D9,
            composer: 0xFFFBEF,
            inset: 0xEFEBD4,
            terminal: 0xFDF6E3,
            sidebar_border: rgb(0xE0DCC7).into(),

            neutral: rgb(0x939F91).into(),

            text: 0x5C6A72,
            text_secondary: 0x829181,
            text_tertiary: 0x939F91,
            text_ghost: 0xA6B0A0,

            accent: 0x8DA101,
            selection: wash(0xEAEDC8, 0.70),
            code_text: 0xF57D26,

            inverse: 0x5C6A72,
            on_inverse: 0xFDF6E3,

            info: 0x3A94C5,
            warning: 0xDFA000,
            success: 0x8DA101,
            favorite: 0xDFA000,
            danger: 0xF85552,

            syntax: SyntaxColors {
                keyword: rgb(0xF85552).into(),
                literal: rgb(0x35A77C).into(),
                string: rgb(0x8DA101).into(),
                comment: rgb(0x939F91).into(),
                number: rgb(0xDF69BA).into(),
                ty: rgb(0xDFA000).into(),
                function: rgb(0x8DA101).into(),
                meta: rgb(0xDF69BA).into(),
            },
            ansi: [
                0x5c6a72, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xe0dcc7,
                0x5c6a72, 0xf85552, 0x8da101, 0xdfa000, 0x3a94c5, 0xdf69ba, 0x35a77c, 0xe0dcc7,
            ],
        })
    }

    /// Kanagawa Lotus — rebelot/kanagawa.nvim `colors.lua` + `themes.lua`.
    /// Keywords are violet, functions blue, constants orange, strings green;
    /// the ANSI table is the theme's own `term` list.
    pub fn kanagawa_light() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xF2ECBC,
            sidebar_solid: 0xE5DDB0,
            surface: 0xF2ECBC,
            raised: 0xE7DBA0,
            // Lotus has nothing lighter than its bg; the composer is a small
            // derivation toward white.
            composer: 0xF7F2CE,
            inset: 0xE5DDB0,
            terminal: 0xF2ECBC,
            sidebar_border: rgb(0xE4D794).into(),

            neutral: rgb(0x8A8980).into(),

            text: 0x545464,
            text_secondary: 0x716E61,
            text_tertiary: 0x8A8980,
            text_ghost: 0xA09CAC,

            accent: 0x4D699B,
            selection: wash(0xC9CBD1, 0.55),
            code_text: 0xCC6D00,

            inverse: 0x545464,
            on_inverse: 0xF2ECBC,

            info: 0x5A7785,
            warning: 0xE98A00,
            success: 0x6F894E,
            favorite: 0xDE9800,
            danger: 0xC84053,

            syntax: SyntaxColors {
                keyword: rgb(0x624C83).into(),  // lotusViolet4 — statement/keyword
                literal: rgb(0xCC6D00).into(),  // lotusOrange — constant
                string: rgb(0x6F894E).into(),   // lotusGreen
                comment: rgb(0x8A8980).into(),  // lotusGray3
                number: rgb(0xB35B79).into(),   // lotusPink
                ty: rgb(0x597B75).into(),       // lotusAqua
                function: rgb(0x4D699B).into(), // lotusBlue4
                meta: rgb(0xC84053).into(),     // lotusRed — preproc
            },
            ansi: [
                0x1f1f28, 0xc84053, 0x6f894e, 0x77713f, 0x4d699b, 0xb35b79, 0x597b75, 0x545464,
                0x8a8980, 0xd7474b, 0x6e915f, 0x836f4a, 0x6693bf, 0x624c83, 0x5e857a, 0x43436c,
            ],
        })
    }

    /// Zenburn — bbatsov/zenburn-emacs `zenburn-theme.el`. Low contrast:
    /// keywords yellow, strings red, functions cyan, types blue-1, comments
    /// the scheme's signature green. ANSI is the alacritty-theme port.
    pub fn zenburn() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x3F3F3F,
            sidebar_solid: 0x383838,
            surface: 0x3F3F3F,
            raised: 0x4F4F4F,
            composer: 0x494949,
            inset: 0x383838,
            terminal: 0x3A3A3A,
            sidebar_border: rgb(0x494949).into(),

            neutral: rgb(0x989890).into(),

            text: 0xDCDCCC,
            text_secondary: 0x989890,
            // Midpoint of fg-05 and fg-1 — the ramp has no fourth stop.
            text_tertiary: 0x7E7F73,
            text_ghost: 0x656555,

            accent: 0xF0DFAF,
            selection: wash(0x8CD0D3, 0.28),
            code_text: 0xDFAF8F,

            inverse: 0xDCDCCC,
            on_inverse: 0x3F3F3F,

            info: 0x8CD0D3,
            warning: 0xDFAF8F,
            success: 0x7F9F7F,
            favorite: 0xF0DFAF,
            danger: 0xCC9393,

            syntax: SyntaxColors {
                keyword: rgb(0xF0DFAF).into(),  // zenburn-yellow
                literal: rgb(0xBFEBBF).into(),  // zenburn-green+4 — constant
                string: rgb(0xCC9393).into(),   // zenburn-red
                comment: rgb(0x7F9F7F).into(),  // zenburn-green
                number: rgb(0xDC8CC3).into(),   // zenburn-magenta — no upstream number face
                ty: rgb(0x7CB8BB).into(),       // zenburn-blue-1
                function: rgb(0x93E0E3).into(), // zenburn-cyan
                meta: rgb(0x94BFF3).into(),     // zenburn-blue+1 — preprocessor
            },
            ansi: [
                0x1e2320, 0xd78787, 0x60b48a, 0xdfaf8f, 0x506070, 0xdc8cc3, 0x8cd0d3, 0xdcdccc,
                0x709080, 0xdca3a3, 0xc3bf9f, 0xf0dfaf, 0x94bff3, 0xec93d3, 0x93e0e3, 0xffffff,
            ],
        })
    }

    /// Poimandres — drcmda/poimandres-theme `src/theme.js`. A deep blue-gray
    /// canvas where almost everything cool-toned is the signature mint:
    /// strings, numbers, and control flow share it, while functions and
    /// types are light blue. ANSI is the theme's own terminal table.
    pub fn poimandres() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x1B1E28,
            // Derived one step darker than the canvas; the scheme has no
            // darker surface of its own.
            sidebar_solid: 0x171A24,
            surface: 0x1B1E28,
            raised: 0x303340,
            composer: 0x232733,
            inset: 0x15171F,
            terminal: 0x1B1E28,
            sidebar_border: rgb(0x303340).into(),

            neutral: rgb(0x7390AA).into(),

            text: 0xE4F0FB,
            text_secondary: 0xA6ACCD,
            text_tertiary: 0x767C9D,
            text_ghost: 0x506477,

            accent: 0x5DE4C7,
            selection: wash(0x717CB4, 0.25),
            code_text: 0xADD7FF,

            inverse: 0xE4F0FB,
            on_inverse: 0x1B1E28,

            info: 0xADD7FF,
            warning: 0xFFFAC2,
            success: 0x5DE4C7,
            favorite: 0xFFFAC2,
            danger: 0xD0679D,

            syntax: SyntaxColors {
                keyword: rgb(0x5DE4C7).into(),  // brightMint — control flow
                literal: rgb(0x5DE4C7).into(),  // constant.language — same mint
                string: rgb(0x5DE4C7).into(),   // strings are mint upstream
                comment: rgb(0x767C9D).into(),  // darkerGray
                number: rgb(0x5DE4C7).into(),   // constant.numeric — same mint
                ty: rgb(0xADD7FF).into(),       // lightBlue — types and classes
                function: rgb(0xADD7FF).into(), // lightBlue — function decls
                meta: rgb(0x91B4D5).into(),     // desaturatedBlue — attributes
            },
            ansi: [
                0x1B1E28, 0xD0679D, 0x5DE4C7, 0xFFFAC2, 0x89DDFF, 0xF087BD, 0x89DDFF, 0xFFFFFF,
                0xA6ACCD, 0xD0679D, 0x5DE4C7, 0xFFFAC2, 0xADD7FF, 0xF087BD, 0xADD7FF, 0xFFFFFF,
            ],
        })
    }

    /// GitHub Light Default — primer/github-vscode-theme. Paper-white canvas
    /// on `#f6f8fa` chrome; syntax is Pretty Lights: keywords red, strings
    /// deep blue, constants and numbers blue, functions purple, types
    /// orange. ANSI is the theme's own terminal table.
    pub fn github_light() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xFFFFFF,
            sidebar_solid: 0xF6F8FA,
            surface: 0xFFFFFF,
            raised: 0xF6F8FA,
            composer: 0xF6F8FA,
            inset: 0xF6F8FA,
            terminal: 0xFFFFFF,
            sidebar_border: rgb(0xD0D7DE).into(),

            neutral: rgb(0x8C959F).into(),

            text: 0x1F2328,
            text_secondary: 0x656D76,
            text_tertiary: 0x6E7781,
            text_ghost: 0xAFB8C1,

            accent: 0x0969DA,
            selection: wash(0x0366D6, 0.20),
            code_text: 0x0550AE,

            inverse: 0x1F2328,
            on_inverse: 0xFFFFFF,

            info: 0x0969DA,
            warning: 0x9A6700,
            success: 0x1A7F37,
            favorite: 0xBF8700,
            danger: 0xCF222E,

            syntax: SyntaxColors {
                keyword: rgb(0xCF222E).into(),  // red — keyword/storage
                literal: rgb(0x0550AE).into(),  // blue — constant
                string: rgb(0x0A3069).into(),   // deep blue
                comment: rgb(0x6E7781).into(),  // gray
                number: rgb(0x0550AE).into(),   // constant.numeric → blue
                ty: rgb(0x953800).into(),       // orange — entity.name
                function: rgb(0x8250DF).into(), // purple — entity.name.function
                meta: rgb(0x8250DF).into(),     // purple — meta.diff.range
            },
            ansi: [
                0x24292f, 0xcf222e, 0x116329, 0x4d2d00, 0x0969da, 0x8250df, 0x1b7c83, 0x6e7781,
                0x57606a, 0xa40e26, 0x1a7f37, 0x633c01, 0x218bff, 0xa475f9, 0x3192aa, 0x8c959f,
            ],
        })
    }

    /// GitHub Dark Default — primer/github-vscode-theme. `#0d1117` canvas on
    /// `#010409` chrome; the same Pretty Lights hue roles as the light side.
    pub fn github_dark() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x0D1117,
            sidebar_solid: 0x010409,
            surface: 0x0D1117,
            raised: 0x161B22,
            composer: 0x161B22,
            inset: 0x010409,
            terminal: 0x0D1117,
            sidebar_border: rgb(0x30363D).into(),

            neutral: rgb(0x8B949E).into(),

            text: 0xE6EDF3,
            text_secondary: 0x7D8590,
            text_tertiary: 0x6E7681,
            text_ghost: 0x484F58,

            accent: 0x2F81F7,
            selection: wash(0x3392FF, 0.27),
            code_text: 0x79C0FF,

            inverse: 0xE6EDF3,
            on_inverse: 0x0D1117,

            info: 0x2F81F7,
            warning: 0xD29922,
            success: 0x3FB950,
            favorite: 0xE3B341,
            danger: 0xF85149,

            syntax: SyntaxColors {
                keyword: rgb(0xFF7B72).into(),  // red — keyword/storage
                literal: rgb(0x79C0FF).into(),  // blue — constant
                string: rgb(0xA5D6FF).into(),   // light blue
                comment: rgb(0x8B949E).into(),  // gray
                number: rgb(0x79C0FF).into(),   // constant.numeric → blue
                ty: rgb(0xFFA657).into(),       // orange — entity.name
                function: rgb(0xD2A8FF).into(), // purple — entity.name.function
                meta: rgb(0xD2A8FF).into(),     // purple — meta.diff.range
            },
            ansi: [
                0x484f58, 0xff7b72, 0x3fb950, 0xd29922, 0x58a6ff, 0xbc8cff, 0x39c5cf, 0xb1bac4,
                0x6e7681, 0xffa198, 0x56d364, 0xe3b341, 0x79c0ff, 0xd2a8ff, 0x56d4dd, 0xffffff,
            ],
        })
    }

    /// Dracula — spec.draculatheme.com. The `#282a36` canvas and the classic
    /// candy set: keywords pink, functions green, strings yellow, types cyan,
    /// constants and numbers purple. Secondary text ramps are interpolated
    /// between the spec's foreground and comment — the palette has no grays.
    /// ANSI is the official terminal spec.
    pub fn dracula() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x282A36,
            sidebar_solid: 0x21222C,
            surface: 0x282A36,
            raised: 0x44475A,
            composer: 0x343746,
            inset: 0x21222C,
            terminal: 0x282A36,
            sidebar_border: rgb(0x44475A).into(),

            neutral: rgb(0x6272A4).into(),

            text: 0xF8F8F2,
            text_secondary: 0xADB5CB,
            text_tertiary: 0x8893B7,
            text_ghost: 0x6272A4,

            accent: 0xBD93F9,
            selection: wash(0x44475A, 0.80),
            code_text: 0x8BE9FD,

            inverse: 0xF8F8F2,
            on_inverse: 0x282A36,

            info: 0x8BE9FD,
            warning: 0xFFB86C,
            success: 0x50FA7B,
            favorite: 0xF1FA8C,
            danger: 0xFF5555,

            syntax: SyntaxColors {
                keyword: rgb(0xFF79C6).into(), // pink
                literal: rgb(0xBD93F9).into(), // purple — constant.language
                string: rgb(0xF1FA8C).into(),  // yellow
                comment: rgb(0x6272A4).into(),
                number: rgb(0xBD93F9).into(), // purple — constant.numeric
                ty: rgb(0x8BE9FD).into(),     // cyan
                function: rgb(0x50FA7B).into(), // green
                meta: rgb(0xFFB86C).into(),   // orange — parameters/attributes
            },
            ansi: [
                0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
                0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
            ],
        })
    }

    /// Rosé Pine Dawn — rose-pine/neovim `palette.lua` + highlight groups.
    /// Warm paper canvas; keywords are pine, strings gold, functions rose,
    /// types foam, attributes iris. The ANSI table is the official alacritty
    /// port (pine fills the green slot, rose the cyan slot).
    pub fn rose_pine_dawn() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xFAF4ED,
            sidebar_solid: 0xF8F0E7,
            surface: 0xFAF4ED,
            raised: 0xFFFAF3,
            composer: 0xFFFAF3,
            inset: 0xF2E9E1,
            terminal: 0xFFFAF3,
            sidebar_border: rgb(0xDFDAD9).into(),

            neutral: rgb(0x9893A5).into(),

            text: 0x464261,
            text_secondary: 0x797593,
            text_tertiary: 0x9893A5,
            text_ghost: 0xCECACD,

            accent: 0x907AA9,
            selection: wash(0x907AA9, 0.20),
            code_text: 0x56949F,

            inverse: 0x464261,
            on_inverse: 0xFAF4ED,

            info: 0x56949F,
            warning: 0xEA9D34,
            success: 0x286983,
            favorite: 0xEA9D34,
            danger: 0xB4637A,

            syntax: SyntaxColors {
                keyword: rgb(0x286983).into(),  // pine
                literal: rgb(0xD7827E).into(),  // rose — booleans
                string: rgb(0xEA9D34).into(),   // gold
                comment: rgb(0x797593).into(),  // subtle
                number: rgb(0xEA9D34).into(),   // gold
                ty: rgb(0x56949F).into(),       // foam
                function: rgb(0xD7827E).into(), // rose
                meta: rgb(0x907AA9).into(),     // iris — preproc/attributes
            },
            ansi: [
                0xf2e9e1, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
                0x9893a5, 0xb4637a, 0x286983, 0xea9d34, 0x56949f, 0x907aa9, 0xd7827e, 0x575279,
            ],
        })
    }

    /// Rosé Pine Moon — the dark side of the same palette. `#232136` base
    /// under `#1f1d30` chrome, same hue roles as Dawn.
    pub fn rose_pine_moon() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x232136,
            sidebar_solid: 0x1F1D30,
            surface: 0x232136,
            raised: 0x393552,
            composer: 0x2A273F,
            inset: 0x1F1D30,
            terminal: 0x232136,
            sidebar_border: rgb(0x44415A).into(),

            neutral: rgb(0x6E6A86).into(),

            text: 0xE0DEF4,
            text_secondary: 0x908CAA,
            text_tertiary: 0x6E6A86,
            text_ghost: 0x56526E,

            accent: 0xC4A7E7,
            selection: wash(0xC4A7E7, 0.20),
            code_text: 0x9CCFD8,

            inverse: 0xE0DEF4,
            on_inverse: 0x232136,

            info: 0x9CCFD8,
            warning: 0xF6C177,
            success: 0x3E8FB0,
            favorite: 0xF6C177,
            danger: 0xEB6F92,

            syntax: SyntaxColors {
                keyword: rgb(0x3E8FB0).into(),  // pine
                literal: rgb(0xEA9A97).into(),  // rose — booleans
                string: rgb(0xF6C177).into(),   // gold
                comment: rgb(0x908CAA).into(),  // subtle
                number: rgb(0xF6C177).into(),   // gold
                ty: rgb(0x9CCFD8).into(),       // foam
                function: rgb(0xEA9A97).into(), // rose
                meta: rgb(0xC4A7E7).into(),     // iris — preproc/attributes
            },
            ansi: [
                0x393552, 0xeb6f92, 0x3e8fb0, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xe0def4,
                0x6e6a86, 0xeb6f92, 0x3e8fb0, 0xf6c177, 0x9ccfd8, 0xc4a7e7, 0xea9a97, 0xe0def4,
            ],
        })
    }

    /// Kansō Zen — webhooked/kanso.nvim `colors.lua` + `themes.lua`. A
    /// near-black `#090e13` canvas and a deliberately desaturated ramp:
    /// keywords violet-gray, functions slate blue, strings sage, constants
    /// clay, types aqua. ANSI is the theme's own `term` list.
    pub fn kanso_zen() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x090E13,
            // The scheme's own bg_dim is a step *lighter* than its canvas.
            sidebar_solid: 0x1C1E25,
            surface: 0x090E13,
            raised: 0x22262D,
            composer: 0x1C1E25,
            inset: 0x070B0F,
            terminal: 0x090E13,
            sidebar_border: rgb(0x22262D).into(),

            neutral: rgb(0x75797F).into(),

            text: 0xC5C9C7,
            text_secondary: 0xA4A7A4,
            text_tertiary: 0x909398,
            text_ghost: 0x5C6066,

            accent: 0x7FB4CA,
            selection: wash(0x393B44, 0.80),
            code_text: 0x7FB4CA,

            inverse: 0xC5C9C7,
            on_inverse: 0x090E13,

            info: 0x658594,
            warning: 0xDCA561,
            success: 0x98BB6C,
            favorite: 0xE6C384,
            danger: 0xC34043,

            syntax: SyntaxColors {
                keyword: rgb(0x8992A7).into(),  // violet2 — keyword/statement
                literal: rgb(0xB6927B).into(),  // orange — constant
                string: rgb(0x8A9A7B).into(),   // green3
                comment: rgb(0x75797F).into(),  // gray4
                number: rgb(0xA292A3).into(),   // pink
                ty: rgb(0x8EA4A2).into(),       // aqua
                function: rgb(0x8BA4B0).into(), // blue3
                meta: rgb(0x909398).into(),     // gray3 — preproc/parameters
            },
            ansi: [
                0x090e13, 0xc4746e, 0x8a9a7b, 0xc4b28a, 0x8ba4b0, 0xa292a3, 0xc5c9c7, 0xa4a7a4,
                0x909398, 0xe46876, 0x87a987, 0xe6c384, 0x7fb4ca, 0x938aa9, 0x7aa89f, 0xc5c9c7,
            ],
        })
    }

    /// Kansō Pearl — the light side of kanso.nvim, on `#f2f1ef` paper.
    /// Same hue roles as Zen; the palette is the repo's `pearl*` ramp.
    pub fn kanso_pearl() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xF2F1EF,
            sidebar_solid: 0xE2E1DF,
            surface: 0xF2F1EF,
            raised: 0xE2E1DF,
            // Pearl has nothing lighter than its bg; the composer is a small
            // derivation toward white.
            composer: 0xF8F7F5,
            inset: 0xE2E1DF,
            terminal: 0xF2F1EF,
            sidebar_border: rgb(0xDDDDDB).into(),

            neutral: rgb(0x6D6D69).into(),

            text: 0x22262D,
            text_secondary: 0x545464,
            text_tertiary: 0x6D6D69,
            text_ghost: 0x9F9F99,

            accent: 0x4D699B,
            selection: wash(0xB5CBD2, 0.60),
            code_text: 0xCC6D00,

            inverse: 0x22262D,
            on_inverse: 0xF2F1EF,

            info: 0x5A7785,
            warning: 0xE98A00,
            success: 0x6E915F,
            favorite: 0xDE9800,
            danger: 0xD7474B,

            syntax: SyntaxColors {
                keyword: rgb(0x624C83).into(),  // pearlViolet4 — keyword/statement
                literal: rgb(0xCC6D00).into(),  // pearlOrange — constant
                string: rgb(0x6F894E).into(),   // pearlGreen
                comment: rgb(0x6D6D69).into(),  // pearlGray3
                number: rgb(0xB35B79).into(),   // pearlPink
                ty: rgb(0x597B75).into(),       // pearlAqua
                function: rgb(0x4D699B).into(), // pearlBlue4
                meta: rgb(0x5C6068).into(),     // pearlGray2 — preproc
            },
            ansi: [
                0x22262d, 0xc84053, 0x6f894e, 0x77713f, 0x4d699b, 0xb35b79, 0x597b75, 0x22262d,
                0x6d6d69, 0xd7474b, 0x6e915f, 0x836f4a, 0x6693bf, 0x624c83, 0x5e857a, 0x43436c,
            ],
        })
    }

    /// Warm Burnout Dark — felipefdl/warm-burnout (nvim `palette.lua`, Zed
    /// port). Ember-toned on a `#1a1510` canvas: keywords orange, functions
    /// amber, strings olive, types steel blue, constants dusty rose; the
    /// accent is the signature burnt orange. ANSI is the ghostty port's
    /// table.
    pub fn warm_burnout_dark() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: true,
            canvas: 0x1A1510,
            sidebar_solid: 0x14120F,
            surface: 0x1A1510,
            raised: 0x222018,
            composer: 0x1F1D17,
            inset: 0x14120F,
            terminal: 0x1A1510,
            sidebar_border: rgb(0x222018).into(),

            neutral: rgb(0xADA69C).into(),

            text: 0xBFBDB6,
            text_secondary: 0xADA69C,
            text_tertiary: 0xA59F96,
            // fg_gutter (`#a59f96a6`) blended over the canvas — the ramp has
            // no dimmer solid stop.
            text_ghost: 0x746E67,

            accent: 0xB8522E,
            selection: wash(0x8AA8B8, 0.25),
            code_text: 0xF29668,

            inverse: 0xBFBDB6,
            on_inverse: 0x1A1510,

            info: 0x90AEC0,
            warning: 0xFFB454,
            success: 0x70BF56,
            favorite: 0xF5C56E,
            danger: 0xF49090,

            syntax: SyntaxColors {
                keyword: rgb(0xFF8F40).into(),  // orange
                literal: rgb(0xD4A8B8).into(),  // dusty rose — constant/boolean
                string: rgb(0xB4BC78).into(),   // olive
                comment: rgb(0xB4A89C).into(),  // warm gray
                number: rgb(0xD4A8B8).into(),   // dusty rose
                ty: rgb(0x90AEC0).into(),       // steel blue
                function: rgb(0xFFB454).into(), // amber
                meta: rgb(0xE6C08A).into(),     // decorator sand
            },
            ansi: [
                0x23211b, 0xf06b73, 0x70bf56, 0xfdb04c, 0x4fbfff, 0xd0a1ff, 0x93e2c8, 0xc7c7c7,
                0x686868, 0xf07178, 0xaad94c, 0xffb454, 0x59c2ff, 0xd2a6ff, 0x95e6cb, 0xffffff,
            ],
        })
    }

    /// Warm Burnout Light — the `#f5ede0` paper side of the same palette.
    /// Same hue roles deepened for contrast; the burnt-orange accent is
    /// unchanged. ANSI is the ghostty port's light table.
    pub fn warm_burnout_light() -> Self {
        Self::from_spec(ThemeSpec {
            is_dark: false,
            canvas: 0xF5EDE0,
            sidebar_solid: 0xEDE6DA,
            surface: 0xF5EDE0,
            raised: 0xF0E8DC,
            composer: 0xFAF6F0,
            inset: 0xEDE6DA,
            terminal: 0xF5EDE0,
            sidebar_border: rgb(0xDDD6CA).into(),

            neutral: rgb(0x8A8070).into(),

            text: 0x3A3630,
            text_secondary: 0x5C5750,
            text_tertiary: 0x8A8070,
            text_ghost: 0xA09888,

            accent: 0xB8522E,
            selection: wash(0x8AA8B8, 0.25),
            code_text: 0x8F4418,

            inverse: 0x3A3630,
            on_inverse: 0xF5EDE0,

            info: 0x285464,
            warning: 0x855700,
            success: 0x226414,
            favorite: 0x8A6000,
            danger: 0xB03434,

            syntax: SyntaxColors {
                keyword: rgb(0x924800).into(),
                literal: rgb(0x7E4060).into(),
                string: rgb(0x4D5C1A).into(),
                comment: rgb(0x544C40).into(),
                number: rgb(0x7E4060).into(),
                ty: rgb(0x285464).into(),
                function: rgb(0x855700).into(),
                meta: rgb(0x7A5A1C).into(),
            },
            ansi: [
                0x3a3630, 0xb82820, 0x2d6a14, 0x8a6000, 0x2060a0, 0x8a3090, 0x146858, 0xc0b8aa,
                0x686868, 0xc83028, 0x3a7a20, 0x9a7008, 0x2870b0, 0x9a38a0, 0x208870, 0xfaf6f0,
            ],
        })
    }
}

/// Resolve settings to a palette. `System` picks the slot matching the OS
/// appearance; `Light`/`Dark` pin their slot regardless of it.
fn theme_for(settings: ThemeSettings, system_dark: bool) -> Theme {
    let name = match settings.mode {
        ThemeMode::System if system_dark => settings.dark,
        ThemeMode::System | ThemeMode::Light => settings.light,
        ThemeMode::Dark => settings.dark,
    };
    theme_named(name)
}

fn theme_named(name: ThemeName) -> Theme {
    match name {
        ThemeName::DefaultLight => Theme::light(),
        ThemeName::DefaultDark => Theme::dark(),
        ThemeName::GruvboxLightHard => Theme::gruvbox_light_hard(),
        ThemeName::GruvboxDark => Theme::gruvbox_dark(),
        ThemeName::EverforestDark => Theme::everforest_dark(),
        ThemeName::EverforestLight => Theme::everforest_light(),
        ThemeName::KanagawaLight => Theme::kanagawa_light(),
        ThemeName::ZenburnDark => Theme::zenburn(),
        ThemeName::PoimandresDark => Theme::poimandres(),
        ThemeName::GithubLight => Theme::github_light(),
        ThemeName::GithubDark => Theme::github_dark(),
        ThemeName::DraculaDark => Theme::dracula(),
        ThemeName::RosePineDawn => Theme::rose_pine_dawn(),
        ThemeName::RosePineMoon => Theme::rose_pine_moon(),
        ThemeName::KansoZen => Theme::kanso_zen(),
        ThemeName::KansoPearl => Theme::kanso_pearl(),
        ThemeName::WarmBurnoutLight => Theme::warm_burnout_light(),
        ThemeName::WarmBurnoutDark => Theme::warm_burnout_dark(),
    }
}

#[derive(Clone, Copy)]
struct ActiveWakuTheme(Theme);

impl Global for ActiveWakuTheme {}

/// Publish the resolved palette. [`Theme::current`] reads it back from the
/// global, which is how every view gets its colors.
fn set_active_theme(theme: Theme, cx: &mut App) {
    cx.set_global(ActiveWakuTheme(theme));
}

/// Resolve and publish the startup palette, before any window exists.
/// Persisted settings are loaded later; the default pair is close enough
/// for the first frame.
pub fn init(cx: &mut App) {
    let system_dark = matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    set_active_theme(
        if system_dark {
            Theme::dark()
        } else {
            Theme::light()
        },
        cx,
    );
}

pub fn apply_theme_preference(
    settings: ThemeSettings,
    sidebar_transparent: bool,
    window: &mut Window,
    cx: &mut App,
) {
    crate::platform::set_window_appearance(window, native_override(settings));
    let system_dark = matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    let mut theme = theme_for(settings, system_dark);
    let is_dark = theme.is_dark;
    if !sidebar_transparent {
        // The vibrancy stack is switched off natively, so the sidebar needs
        // its own fill — the same solid it already uses while resizing.
        theme.sidebar = theme.sidebar_drag_background;
    }
    set_active_theme(theme, cx);
    crate::platform::configure_sidebar_material(
        window,
        theme.sidebar_drag_background,
        is_dark,
        sidebar_transparent,
    );
    window.refresh();
}
