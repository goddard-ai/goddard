use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, OnceLock};
use std::time::Duration;

use crate::input::TextInput;
use crate::ui::menu::{ContextMenuHandle, MenuItem, context_menu};
use crate::ui::shortcut::ShortcutHint;
use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{BidirectionalIterator, Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point as TerminalPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty::{self, Shell};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, Sender, unbounded};
use gpui::{
    AnyElement, App, AppContext, Bounds, ClipboardItem, Context, Entity, EventEmitter, FocusHandle,
    Focusable, FontFallbacks, FontStyle, FontWeight, Global, Hsla, InteractiveElement, IntoElement,
    KeyBinding, KeyDownEvent, Keystroke, Modifiers, ModifiersChangedEvent, MouseButton,
    MouseDownEvent, MouseExitEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point,
    Render, ScrollDelta, ScrollWheelEvent, SharedString, StrikethroughStyle, Styled, StyledText,
    Subscription, Task, TextRun, UnderlineStyle, Window, actions, canvas, div, fill, font, point,
    px, rgb, size,
};
use parking_lot::Mutex;

use gpui::prelude::FluentBuilder;

use crate::persistence::DEFAULT_RIGHT_PANEL_WIDTH;
use crate::theme::{Theme, hairline, sp};
use crate::ui::scrollbar::{self, ScrollbarState};

/// Fallback advance width, used only until the font has been measured.
const TERMINAL_CELL_WIDTH: f32 = 7.8;
/// Startup row height — render replaces it with the scaled value below.
const TERMINAL_CELL_HEIGHT: f32 = 18.0;
/// Rows lead the font at the ratio the shipped 12.5px / 18px pairing set.
const TERMINAL_FONT_LEADING: f32 = TERMINAL_CELL_HEIGHT / 12.5;

fn terminal_cell_height(font_size: f32) -> f32 {
    font_size * TERMINAL_FONT_LEADING
}

/// The resolved terminal font size, published by the app when settings load
/// or change.
struct ActiveTerminalFontSize(f32);
impl Global for ActiveTerminalFontSize {}

/// The size terminal views draw at — the code font's default until
/// `install_font_size` runs.
pub fn font_size(cx: &App) -> f32 {
    cx.try_global::<ActiveTerminalFontSize>()
        .map_or(crate::persistence::DEFAULT_CODE_FONT_SIZE, |size| size.0)
}

/// Publish the resolved terminal font size so every terminal view tracks it.
pub fn install_font_size(size: f32, cx: &mut App) {
    cx.set_global(ActiveTerminalFontSize(size));
}

/// Whether the link modifier still opens links while the running program is
/// reporting mouse events; published by the app when settings load or change.
struct ActiveOpenLinksInMouseMode(bool);
impl Global for ActiveOpenLinksInMouseMode {}

/// The resolved preference — on until the app publishes the stored value.
fn open_links_in_mouse_mode(cx: &App) -> bool {
    cx.try_global::<ActiveOpenLinksInMouseMode>()
        .map_or(true, |enabled| enabled.0)
}

/// Publish the resolved preference so every terminal view tracks it.
pub fn install_open_links_in_mouse_mode(enabled: bool, cx: &mut App) {
    cx.set_global(ActiveOpenLinksInMouseMode(enabled));
}

/// The key that opens links and paths in the integrated terminal; published
/// by the app when settings load or change.
struct ActiveTerminalLinkModifier(crate::persistence::TerminalLinkModifier);
impl Global for ActiveTerminalLinkModifier {}

/// The resolved link modifier — the platform key until the app publishes
/// the stored pick.
fn link_modifier(cx: &App) -> crate::persistence::TerminalLinkModifier {
    cx.try_global::<ActiveTerminalLinkModifier>()
        .map_or(Default::default(), |modifier| modifier.0)
}

/// Publish the resolved pick so every terminal view tracks it.
pub fn install_link_modifier(modifier: crate::persistence::TerminalLinkModifier, cx: &mut App) {
    cx.set_global(ActiveTerminalLinkModifier(modifier));
}

/// Whether finishing a selection copies it to the clipboard; published by
/// the app when settings load or change.
struct ActiveTerminalCopyOnSelect(bool);
impl Global for ActiveTerminalCopyOnSelect {}

/// The resolved copy-on-select preference — on until the app publishes the
/// stored value.
fn copy_on_select(cx: &App) -> bool {
    cx.try_global::<ActiveTerminalCopyOnSelect>()
        .map_or(true, |enabled| enabled.0)
}

/// Publish the resolved preference so every terminal view tracks it.
pub fn install_copy_on_select(enabled: bool, cx: &mut App) {
    cx.set_global(ActiveTerminalCopyOnSelect(enabled));
}

/// Whether `modifiers` holds the key that owns the link gesture — the
/// platform's primary key (⌘/Ctrl) for `CmdOrCtrl`, ⌥/Alt for `Alt`. The
/// other key keeps its plain-click meaning, so the two never compete.
#[inline]
fn link_modifier_pressed(modifiers: &Modifiers, cx: &App) -> bool {
    match link_modifier(cx) {
        crate::persistence::TerminalLinkModifier::CmdOrCtrl => modifiers.secondary(),
        crate::persistence::TerminalLinkModifier::Alt => modifiers.alt,
    }
}

#[inline]
fn terminal_clipboard_modifier_pressed(modifiers: &Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        modifiers.secondary() && !modifiers.control && !modifiers.alt
    } else {
        // Preserve Ctrl+C for SIGINT and follow Linux terminal convention.
        modifiers.control && modifiers.shift && !modifiers.alt && !modifiers.platform
    }
}
const TERMINAL_PADDING_X: f32 = 10.0;
const TERMINAL_PADDING_Y: f32 = 8.0;
const TERMINAL_TOOLBAR_HEIGHT: f32 = 34.0;
const TERMINAL_MIN_COLUMNS: usize = 20;
const TERMINAL_MIN_ROWS: usize = 8;
const TERMINAL_SCROLLBACK_LINES: usize = 10_000;
/// Grid lines sent with a command-bar request — enough for "that failed"
/// or "retry it with sudo" to resolve.
const COMMAND_BAR_CONTEXT_LINES: usize = 60;
const COMMAND_BAR_CONTEXT_MAX_BYTES: usize = 12 * 1024;
const TERMINAL_CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const TERMINAL_CURSOR_BLINK_PAUSE: Duration = Duration::from_millis(300);

/// Alacritty's default URL hint plus local file paths containing a slash.
/// Requiring a slash keeps ordinary dotted words from becoming links.
#[rustfmt::skip]
const TERMINAL_LINK_REGEX: &str = "((ipfs:|ipns:|magnet:|mailto:|gemini://|gopher://|https://|http://|news:|file:|git://|ssh:|ftp://)|\
                                    (/|~/|\\./|\\.\\./|[A-Za-z0-9._@%+~-]+/))\
                                   [^\u{0000}-\u{001F}\u{007F}-\u{009F}<>\"\\s{-}\\^⟨⟩`\\\\]+";
const MAX_TERMINAL_LINK_SEARCH_LINES: i32 = 100;

/// Matches a localhost URL in raw terminal output: an explicit
/// `http(s)://host` where the host is `localhost`, a `*.localhost` subdomain
/// (Portless's `https://<id>.localhost` form), a loopback address, or a bare
/// `host:port` with no scheme. The leading boundary keeps `xlocalhost` or a
/// dotted run like `foo.127.0.0.1` from matching. The URL itself is captured
/// in `url` so the consumed boundary character stays out of the result.
static LOCALHOST_URL_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r#"(?i)(?:^|[^A-Za-z0-9_.-])(?P<url>https?://(?:(?:[A-Za-z0-9-]+\.)*localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::[0-9]{1,5})?(?:/[^\s"'<>`\\]*)?|(?:(?:[A-Za-z0-9-]+\.)*localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):[0-9]{1,5}(?:/[^\s"'<>`\\]*)?)"#,
    )
    .expect("localhost URL regex compiles")
});
/// Lines re-scanned on every output batch so a match landing partially inside
/// the previous batch is still seen once the rest of it arrives.
const LOCALHOST_SCAN_OVERLAP: usize = 8;
/// Upper bound on one scan: a larger burst is trimmed to its newest lines.
const LOCALHOST_SCAN_MAX_LINES: usize = 512;
/// Reported URLs remembered per terminal view. The cap only bounds memory;
/// a terminal this chatty can afford to re-report its oldest URLs.
const MAX_REPORTED_LOCALHOST_URLS: usize = 64;

/// Icon glyphs (nerd-font private-use codepoints) resolve through CoreText's
/// cascade rather than run splitting: JetBrains Mono itself keeps the
/// Powerline range it covers, everything else falls through to the bundled
/// symbols face registered in [`crate::assets::register_fonts`].
static TERMINAL_FONT_FALLBACKS: LazyLock<FontFallbacks> = LazyLock::new(|| {
    FontFallbacks::from_fonts(vec![crate::assets::SYMBOLS_FONT_FAMILY.to_owned()])
});

fn terminal_font(family: &SharedString) -> gpui::Font {
    let mut terminal_font = font(family.clone());
    terminal_font.fallbacks = Some(TERMINAL_FONT_FALLBACKS.clone());
    terminal_font
}

enum TerminalUiEvent {
    Title(String),
    ResetTitle,
    ClipboardStore(String),
    ClipboardLoad(Arc<dyn Fn(&str) -> String + Send + Sync>),
    /// A custom command's launch line reported the script's exit code
    /// through its title sentinel.
    CommandExit(i32),
    /// Shell integration reported the start of an interactive command.
    CommandBegan,
    /// Shell integration reported a command's exit status.
    CommandEnded(i32),
    /// Shell integration reported the shell's working directory.
    Cwd(PathBuf),
    /// The shell process is gone. The code rides along when the OS reported
    /// one, which doubles as a completion signal for scripts that exit the
    /// shell themselves instead of reaching the sentinel.
    Exited(Option<i32>),
}

/// Emitted on the view when the PTY child exits — the shell itself is gone,
/// not just the foreground job.
pub enum TerminalViewEvent {
    Exited,
    /// A command finished, carrying its exit code when one was reported.
    /// Raised by a custom command's launch-line sentinel, by shell
    /// integration's command-end report for interactive runs, by the
    /// child's own exit status as a fallback for scripts that exit the
    /// shell themselves, and with `None` when the PTY ended — or never
    /// started — without a status, so a pending run always resolves.
    CommandFinished(Option<i32>),
    /// A localhost URL appeared in freshly printed output — a dev server
    /// announcing its port. Carries the normalized, openable URL.
    LocalhostUrl(String),
    /// The shell's command state or working directory changed — sidebar
    /// rows showing status or location need a repaint.
    ActivityChanged,
    /// The command bar asked for a generated shell command. Resolving the
    /// provider invocation and the daemon round-trip belong to the app;
    /// the answer comes back through `apply_command_generation`.
    GenerateCommand {
        generation: u64,
        request: String,
        scrollback: String,
        cwd: PathBuf,
        shell: String,
    },
}

actions!(
    terminal_command_bar,
    [
        ConfirmTerminalCommand,
        RunTerminalCommand,
        DismissTerminalCommand,
    ]
);

/// The command bar's bindings — the field itself keeps TextInput's editing
/// chords, so only the three commit gestures need this context.
pub fn init_command_bar_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(
            "enter",
            ConfirmTerminalCommand,
            Some("TerminalCommandBar > TextInput"),
        ),
        KeyBinding::new(
            "secondary-enter",
            RunTerminalCommand,
            Some("TerminalCommandBar > TextInput"),
        ),
        KeyBinding::new(
            "escape",
            DismissTerminalCommand,
            Some("TerminalCommandBar > TextInput"),
        ),
        KeyBinding::new("escape", DismissTerminalCommand, Some("TerminalCommandBar")),
    ]);
}

/// What a new terminal's PTY runs. `Shell` is the plain interactive shell;
/// `CustomCommand` feeds the command's script to an interactive shell of
/// the command's choosing — typed verbatim when the shell's integration
/// can report the run, sourced from a materialized file otherwise.
/// `Program` execs a binary directly — no shell beneath it — for a program
/// that owns the whole surface, like a terminal-backed agent's TUI.
#[derive(Clone)]
pub enum TerminalLaunch {
    Shell,
    CustomCommand(crate::persistence::CustomCommand),
    Program { program: PathBuf, args: Vec<String> },
}

#[derive(Clone)]
struct TerminalEventProxy {
    dirty: Arc<AtomicBool>,
    sender: Arc<OnceLock<EventLoopSender>>,
    ui_events: Sender<TerminalUiEvent>,
    window_size: Arc<Mutex<WindowSize>>,
    /// Palette OSC replies run on the PTY thread; `snapshot` refreshes this
    /// so they track the active theme.
    palette: Arc<Mutex<Theme>>,
}

impl TerminalEventProxy {
    fn write_pty(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        if let Some(sender) = self.sender.get() {
            let _ = sender.send(Msg::Input(bytes.into()));
        }
    }
}

impl EventListener for TerminalEventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {
                self.dirty.store(true, Ordering::Release);
            }
            Event::Title(title) => {
                // Sentinels ride the OSC 2 title channel: a custom
                // command's exit code and the shell-integration reports
                // are swallowed so they can never rename the surface.
                use crate::shell_integration::ShellReport;
                let event = match crate::custom_commands::parse_command_exit(&title) {
                    Some(code) => TerminalUiEvent::CommandExit(code),
                    None => match crate::shell_integration::parse_report(&title) {
                        Some(ShellReport::CommandBegin) => TerminalUiEvent::CommandBegan,
                        Some(ShellReport::CommandEnd(code)) => TerminalUiEvent::CommandEnded(code),
                        Some(ShellReport::Cwd(cwd)) => TerminalUiEvent::Cwd(cwd),
                        None => TerminalUiEvent::Title(title),
                    },
                };
                let _ = self.ui_events.send(event);
            }
            Event::ResetTitle => {
                let _ = self.ui_events.send(TerminalUiEvent::ResetTitle);
            }
            Event::ClipboardStore(_, text) => {
                let _ = self.ui_events.send(TerminalUiEvent::ClipboardStore(text));
            }
            Event::ClipboardLoad(_, formatter) => {
                let _ = self
                    .ui_events
                    .send(TerminalUiEvent::ClipboardLoad(formatter));
            }
            Event::PtyWrite(text) => self.write_pty(text.into_bytes()),
            Event::ColorRequest(index, formatter) => {
                let theme = *self.palette.lock();
                self.write_pty(formatter(terminal_rgb(index, theme)).into_bytes());
            }
            Event::TextAreaSizeRequest(formatter) => {
                self.write_pty(formatter(*self.window_size.lock()).into_bytes());
            }
            Event::Bell => {}
            Event::Exit => {
                let _ = self.ui_events.send(TerminalUiEvent::Exited(None));
                self.dirty.store(true, Ordering::Release);
            }
            Event::ChildExit(status) => {
                let _ = self.ui_events.send(TerminalUiEvent::Exited(status.code()));
                self.dirty.store(true, Ordering::Release);
            }
        }
    }
}

struct TerminalDimensions {
    columns: usize,
    rows: usize,
}

impl Dimensions for TerminalDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

struct TerminalSession {
    term: Arc<FairMutex<Term<TerminalEventProxy>>>,
    sender: EventLoopSender,
    dirty: Arc<AtomicBool>,
    ui_events: Receiver<TerminalUiEvent>,
    window_size: Arc<Mutex<WindowSize>>,
    grid_size: (usize, usize),
    /// Last pixel cell size reported to the PTY — drives pointer and scroll
    /// math so it stays in step with the drawn grid.
    cell_size: (f32, f32),
    palette: Arc<Mutex<Theme>>,
    url_regex: RegexSearch,
    /// Grid lines — scrollback plus screen — already scanned for a localhost
    /// URL, so each output line is considered once.
    localhost_scan_watermark: usize,
}

impl TerminalSession {
    fn new(
        working_directory: &Path,
        launch: &TerminalLaunch,
        columns: usize,
        rows: usize,
    ) -> Result<Self> {
        let columns = columns.max(TERMINAL_MIN_COLUMNS);
        let rows = rows.max(TERMINAL_MIN_ROWS);
        let window_size = WindowSize {
            num_lines: rows.min(u16::MAX as usize) as u16,
            num_cols: columns.min(u16::MAX as usize) as u16,
            cell_width: TERMINAL_CELL_WIDTH.round() as u16,
            cell_height: TERMINAL_CELL_HEIGHT.round() as u16,
        };
        let shared_window_size = Arc::new(Mutex::new(window_size));
        let dirty = Arc::new(AtomicBool::new(true));
        let sender_slot = Arc::new(OnceLock::new());
        let palette = Arc::new(Mutex::new(Theme::dark()));
        let (ui_event_tx, ui_events) = unbounded();
        let url_regex = RegexSearch::new(TERMINAL_LINK_REGEX)
            .map_err(|error| anyhow::anyhow!("compile terminal link regex: {error}"))?;
        let proxy = TerminalEventProxy {
            dirty: dirty.clone(),
            sender: sender_slot.clone(),
            ui_events: ui_event_tx,
            window_size: shared_window_size.clone(),
            palette: palette.clone(),
        };

        let config = Config {
            scrolling_history: TERMINAL_SCROLLBACK_LINES,
            ..Default::default()
        };
        let dimensions = TerminalDimensions { columns, rows };
        let term = Arc::new(FairMutex::new(Term::new(
            config,
            &dimensions,
            proxy.clone(),
        )));

        let (shell, startup_line, shell_integration, program_args) = match launch {
            TerminalLaunch::Shell => {
                let shell = crate::command_env::default_terminal_shell();
                let installed = crate::shell_integration::install(&shell);
                (shell, None, installed, None)
            }
            TerminalLaunch::CustomCommand(command) => {
                let shell = crate::custom_commands::command_shell(command);
                // A shell the integration hooks reports command
                // boundaries on its own: a single-line script runs as
                // plain typed input — no materialized file, no exit
                // sentinel. Multi-line scripts would report each line
                // as its own command and end the run early, and shells
                // with no hook surface (or no `begin` report, like
                // pre-4.4 bash) can't resolve a run, so all of them
                // keep the sourced file.
                let script = command.script.trim();
                let typed = (!script.is_empty()
                    && !script.contains(['\r', '\n'])
                    && crate::shell_integration::install(&shell)
                    && crate::shell_integration::reports_command_begins(&shell))
                .then(|| script.to_owned());
                match typed {
                    Some(line) => (shell, Some(line), true, None),
                    None => {
                        let script_path = crate::custom_commands::ensure_script(&command.script)
                            .context("materialize custom command script")?;
                        let line = crate::custom_commands::source_line(
                            &shell,
                            &script_path,
                            command.close_on_success,
                        );
                        (shell, Some(line), false, None)
                    }
                }
            }
            TerminalLaunch::Program { program, args } => {
                (program.clone(), None, false, Some(args.clone()))
            }
        };
        let shell_args =
            program_args.unwrap_or_else(|| crate::command_env::default_terminal_shell_args(&shell));
        let mut options = tty::Options {
            shell: Some(Shell::new(shell.to_string_lossy().into_owned(), shell_args)),
            working_directory: Some(working_directory.to_path_buf()),
            drain_on_exit: false,
            ..Default::default()
        };
        options.env.insert("TERM".into(), "xterm-256color".into());
        options.env.insert("COLORTERM".into(), "truecolor".into());
        if shell_integration {
            // The rc block the integration installs keys on $GODDARD so
            // only Goddard-spawned shells source the script. $WAKU keeps
            // blocks a pre-Goddard install left behind working until the
            // installer rewrites them.
            options.env.insert("GODDARD".into(), "1".into());
            options.env.insert("WAKU".into(), "1".into());
        }
        if let Some(path) = crate::command_env::executable_search_path() {
            options
                .env
                .insert("PATH".into(), path.to_string_lossy().into_owned());
        }

        let pty = tty::new(&options, window_size, 0)
            .with_context(|| format!("spawn terminal in {}", working_directory.display()))?;
        let event_loop = EventLoop::new(term.clone(), proxy, pty, false, false)
            .context("create Alacritty PTY event loop")?;
        let sender = event_loop.channel();
        sender_slot
            .set(sender.clone())
            .map_err(|_| anyhow::anyhow!("initialize Alacritty PTY sender"))?;
        event_loop.spawn();

        // The command line lands in the PTY's input queue ahead of whatever
        // the shell prints while starting up, so it runs as the first input
        // the interactive shell reads — the same effect as typing it.
        if let Some(startup_line) = startup_line {
            let _ = sender.send(Msg::Input(format!("{startup_line}\n").into_bytes().into()));
        }

        Ok(Self {
            term,
            sender,
            dirty,
            ui_events,
            window_size: shared_window_size,
            grid_size: (columns, rows),
            cell_size: (TERMINAL_CELL_WIDTH, TERMINAL_CELL_HEIGHT),
            palette,
            url_regex,
            localhost_scan_watermark: 0,
        })
    }

    fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        if !bytes.is_empty() {
            let _ = self.sender.send(Msg::Input(bytes));
        }
    }

    fn resize(&mut self, columns: usize, rows: usize, cell_width: f32, cell_height: f32) {
        let columns = columns.max(TERMINAL_MIN_COLUMNS);
        let rows = rows.max(TERMINAL_MIN_ROWS);
        if self.grid_size == (columns, rows) && self.cell_size == (cell_width, cell_height) {
            return;
        }

        self.grid_size = (columns, rows);
        self.cell_size = (cell_width, cell_height);
        let dimensions = TerminalDimensions { columns, rows };
        self.term.lock().resize(dimensions);
        let size = WindowSize {
            num_lines: rows.min(u16::MAX as usize) as u16,
            num_cols: columns.min(u16::MAX as usize) as u16,
            cell_width: cell_width.round() as u16,
            cell_height: cell_height.round() as u16,
        };
        *self.window_size.lock() = size;
        let _ = self.sender.send(Msg::Resize(size));
        self.dirty.store(true, Ordering::Release);
    }

    fn mode(&self) -> TermMode {
        *self.term.lock().mode()
    }

    /// True while the running program asked for mouse input and Shift isn't
    /// held — Shift is the escape hatch that keeps local clicks and
    /// selection working over a mouse-aware program.
    fn mouse_mode(&self, shift: bool) -> bool {
        self.mode().intersects(TermMode::MOUSE_MODE) && !shift
    }

    fn scroll(&self, lines: i32) {
        if lines == 0 {
            return;
        }
        self.term.lock().scroll_display(Scroll::Delta(lines));
        self.dirty.store(true, Ordering::Release);
    }

    fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
        self.dirty.store(true, Ordering::Release);
    }

    fn clear_scrollback(&self) {
        clear_scrollback(&mut self.term.lock());
        self.dirty.store(true, Ordering::Release);
    }

    fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// The bottom `count` non-blank rows of the live screen — what a custom
    /// command's toast mirrors while it runs. Reads the grid, not the
    /// viewport, so scroll position can't skew it.
    fn tail_lines(&self, count: usize) -> Vec<String> {
        tail_lines(&self.term.lock(), count)
    }

    fn link_at(&mut self, point: TerminalPoint) -> Option<TerminalLink> {
        let TerminalSession {
            term, url_regex, ..
        } = self;
        let term = term.lock();
        let (value, bounds) =
            hyperlink_at(&term, point).or_else(|| plain_link_at(&term, url_regex, point))?;
        Some(TerminalLink { value, bounds })
    }

    fn snapshot(
        &self,
        theme: Theme,
        selection_color: Hsla,
        cursor_style: TerminalCursorStyle,
        hovered_link: Option<&Match>,
    ) -> TerminalSnapshot {
        *self.palette.lock() = theme;
        let term = self.term.lock();
        let content = term.renderable_content();
        let columns = self.grid_size.0;
        let rows = self.grid_size.1;
        let selection = content.selection;
        let cursor_row = content.cursor.point.line.0 + content.display_offset as i32;
        let cursor_column = content.cursor.point.column.0;
        let cursor_style = if matches!(
            content.cursor.shape,
            alacritty_terminal::vte::ansi::CursorShape::Hidden
        ) {
            TerminalCursorStyle::Hidden
        } else {
            cursor_style
        };
        let outline_cursor = (cursor_style == TerminalCursorStyle::Outline
            && (0..rows as i32).contains(&cursor_row)
            && cursor_column < columns)
            .then_some((cursor_row as usize, cursor_column));
        let mut cells = vec![TerminalCell::blank(theme); columns * rows];
        let mut mosaics = Vec::new();

        for indexed in content.display_iter {
            let row = indexed.point.line.0 + content.display_offset as i32;
            let column = indexed.point.column.0;
            if row < 0 || row as usize >= rows || column >= columns {
                continue;
            }
            let cell = indexed.cell;
            let spacer = cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::HIDDEN);
            // Mosaic glyphs keep a space in the text so the run still advances
            // the cell; the glyph itself is painted as quads.
            let mosaic = if spacer { None } else { mosaic_glyph(cell.c) };
            let mut text = if spacer || mosaic.is_some() {
                " ".to_owned()
            } else {
                cell.c.to_string()
            };
            if let Some(zerowidth) = cell.zerowidth() {
                text.extend(zerowidth);
            }

            let mut foreground = resolve_color(cell.fg, content.colors, theme, true);
            let mut background = resolve_color(cell.bg, content.colors, theme, false);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut foreground, &mut background);
            }
            // `Flags::DIM_BOLD` and `Flags::BOLD_ITALIC` are unions of the
            // individual bits, so testing them with `intersects` would also
            // match plain bold or italic cells.
            if cell.flags.contains(Flags::DIM) {
                if theme.is_dark {
                    foreground.l *= 0.7;
                } else {
                    foreground.l = 1.0 - (1.0 - foreground.l) * 0.7;
                }
            }
            if selection.is_some_and(|selection| selection.contains(indexed.point)) {
                background = selection_color;
                foreground = theme.text;
            }
            if cursor_style == TerminalCursorStyle::Solid
                && row == cursor_row
                && column == cursor_column
            {
                background = theme.text;
                foreground = theme.terminal;
            }

            if let Some(glyph) = mosaic {
                mosaics.push(MosaicInstance {
                    row: row as usize,
                    column,
                    glyph,
                    foreground,
                });
            }
            cells[row as usize * columns + column] = TerminalCell {
                text,
                foreground,
                background,
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                underline: cell.flags.intersects(Flags::ALL_UNDERLINES)
                    || hovered_link.is_some_and(|bounds| bounds.contains(&indexed.point)),
                strikeout: cell.flags.contains(Flags::STRIKEOUT),
            };
        }

        let mut rendered_rows = Vec::with_capacity(rows);
        for row in cells.chunks(columns) {
            let mut text = String::new();
            let mut runs: Vec<TerminalRun> = Vec::new();
            for cell in row {
                let len = cell.text.len();
                text.push_str(&cell.text);
                let style = TerminalRunStyle {
                    foreground: cell.foreground,
                    background: cell.background,
                    bold: cell.bold,
                    italic: cell.italic,
                    underline: cell.underline,
                    strikeout: cell.strikeout,
                };
                if let Some(run) = runs.last_mut().filter(|run| run.style == style) {
                    run.len += len;
                } else {
                    runs.push(TerminalRun { len, style });
                }
            }
            rendered_rows.push(TerminalRow { text, runs });
        }

        TerminalSnapshot {
            rows: rendered_rows,
            mosaics,
            outline_cursor,
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

/// Adapts the alacritty grid to the overlay scrollbar: the scrollback history
/// is the content above the viewport, and `display_offset` is how far back up
/// into it the view currently sits (0 = pinned to the live bottom).
#[derive(Clone)]
struct TerminalScrollbarTarget {
    term: Arc<FairMutex<Term<TerminalEventProxy>>>,
    dirty: Arc<AtomicBool>,
    viewport_rows: usize,
    cell_height: f32,
}

impl scrollbar::Scrollable for TerminalScrollbarTarget {
    fn viewport_height(&self) -> Pixels {
        px(self.viewport_rows as f32 * self.cell_height)
    }

    fn max_offset(&self) -> Pixels {
        px(self.term.lock().grid().history_size() as f32 * self.cell_height)
    }

    fn scrolled(&self) -> Pixels {
        let term = self.term.lock();
        let grid = term.grid();
        let lines_above = grid.history_size().saturating_sub(grid.display_offset());
        px(lines_above as f32 * self.cell_height)
    }

    fn scroll_to(&self, offset: Pixels) {
        let mut term = self.term.lock();
        let target_offset = (term.grid().history_size() as f32
            - f32::from(offset) / self.cell_height)
            .round()
            .max(0.0) as usize;
        let delta = target_offset as i32 - term.grid().display_offset() as i32;
        if delta != 0 {
            term.scroll_display(Scroll::Delta(delta));
            self.dirty.store(true, Ordering::Release);
        }
    }
}

/// Cell-filling mosaic glyphs (block elements, quadrant and sextant pieces)
/// the bundled fonts don't cover — painted as quads so their geometry is
/// exact and never depends on fallback-font metrics.
#[derive(Clone, Copy)]
enum MosaicGlyph {
    /// 2×3 sextant mask — bit i covers column i % 2, row i / 2.
    Sextant(u8),
    /// 2×2 quadrant mask — bit i covers column i % 2, row i / 2.
    Quadrant(u8),
    /// Filled rect in eighths of the cell.
    Rect { x0: u8, y0: u8, x1: u8, y1: u8 },
    /// Full-cell stipple approximated with translucency.
    Shade(f32),
}

/// Bit i covers column i % 2, row i / 2 of the 2×3 cell grid, indexed by
/// codepoint - U+1FB00.
const SEXTANT_MASKS: [u8; 60] = [
    0b000001, 0b000010, 0b000011, 0b000100, 0b000101, 0b000110, 0b000111, 0b001000, 0b001001,
    0b001010, 0b001011, 0b001100, 0b001101, 0b001110, 0b001111, 0b010000, 0b010001, 0b010010,
    0b010011, 0b010100, 0b010110, 0b010111, 0b011000, 0b011001, 0b011010, 0b011011, 0b011100,
    0b011101, 0b011110, 0b011111, 0b100000, 0b100001, 0b100010, 0b100011, 0b100100, 0b100101,
    0b100110, 0b100111, 0b101000, 0b101001, 0b101011, 0b101100, 0b101101, 0b101110, 0b101111,
    0b110000, 0b110001, 0b110010, 0b110011, 0b110100, 0b110101, 0b110110, 0b110111, 0b111000,
    0b111001, 0b111010, 0b111011, 0b111100, 0b111101, 0b111110,
];

fn mosaic_glyph(c: char) -> Option<MosaicGlyph> {
    const fn rect(x0: u8, y0: u8, x1: u8, y1: u8) -> MosaicGlyph {
        MosaicGlyph::Rect { x0, y0, x1, y1 }
    }
    Some(match c {
        '\u{2580}' => rect(0, 0, 8, 4),
        '\u{2581}'..='\u{2587}' => rect(0, (8 - (c as u32 - 0x2580)) as u8, 8, 8),
        '\u{2588}' => rect(0, 0, 8, 8),
        '\u{2589}'..='\u{258F}' => rect(0, 0, (8 - (c as u32 - 0x2588)) as u8, 8),
        '\u{2590}' => rect(4, 0, 8, 8),
        '\u{2591}' => MosaicGlyph::Shade(0.25),
        '\u{2592}' => MosaicGlyph::Shade(0.5),
        '\u{2593}' => MosaicGlyph::Shade(0.75),
        '\u{2594}' => rect(0, 0, 8, 1),
        '\u{2595}' => rect(7, 0, 8, 8),
        // Quadrant bits: 0 = upper left, 1 = upper right, 2 = lower left,
        // 3 = lower right.
        '\u{2596}' => MosaicGlyph::Quadrant(0b0100),
        '\u{2597}' => MosaicGlyph::Quadrant(0b1000),
        '\u{2598}' => MosaicGlyph::Quadrant(0b0001),
        '\u{2599}' => MosaicGlyph::Quadrant(0b1101),
        '\u{259A}' => MosaicGlyph::Quadrant(0b1001),
        '\u{259B}' => MosaicGlyph::Quadrant(0b0111),
        '\u{259C}' => MosaicGlyph::Quadrant(0b1011),
        '\u{259D}' => MosaicGlyph::Quadrant(0b0010),
        '\u{259E}' => MosaicGlyph::Quadrant(0b0110),
        '\u{259F}' => MosaicGlyph::Quadrant(0b1110),
        '\u{1FB00}'..='\u{1FB3B}' => MosaicGlyph::Sextant(SEXTANT_MASKS[c as usize - 0x1FB00]),
        '\u{1FB70}'..='\u{1FB75}' => {
            let x0 = (c as u32 - 0x1FB70 + 1) as u8;
            rect(x0, 0, x0 + 1, 8)
        }
        '\u{1FB76}'..='\u{1FB7B}' => {
            let y0 = (c as u32 - 0x1FB76 + 1) as u8;
            rect(0, y0, 8, y0 + 1)
        }
        '\u{1FB82}' => rect(0, 0, 8, 2),
        '\u{1FB83}' => rect(0, 0, 8, 3),
        '\u{1FB84}' => rect(0, 0, 8, 5),
        '\u{1FB85}' => rect(0, 0, 8, 6),
        '\u{1FB86}' => rect(0, 0, 8, 7),
        '\u{1FB87}' => rect(6, 0, 8, 8),
        '\u{1FB88}' => rect(5, 0, 8, 8),
        '\u{1FB89}' => rect(3, 0, 8, 8),
        '\u{1FB8A}' => rect(2, 0, 8, 8),
        '\u{1FB8B}' => rect(1, 0, 8, 8),
        _ => return None,
    })
}

struct MosaicInstance {
    row: usize,
    column: usize,
    glyph: MosaicGlyph,
    foreground: Hsla,
}

fn paint_mosaic(
    window: &mut Window,
    origin: Point<Pixels>,
    cell_width: f32,
    cell_height: f32,
    mosaic: &MosaicInstance,
) {
    // Sub-rect edges that sit inside the cell overshoot a hair so rasterizing
    // adjacent fills can't leave bright seams (the same trick the settings QR
    // uses between modules); edges on the cell boundary stay put so nothing
    // bleeds into the neighbouring cell.
    const OVERLAP: f32 = 0.5;
    let paint = |window: &mut Window, x0: f32, y0: f32, x1: f32, y1: f32, color: Hsla| {
        let right = x1 * cell_width + if x1 < 1.0 { OVERLAP } else { 0.0 };
        let bottom = y1 * cell_height + if y1 < 1.0 { OVERLAP } else { 0.0 };
        window.paint_quad(fill(
            Bounds::new(
                point(
                    origin.x + px(x0 * cell_width),
                    origin.y + px(y0 * cell_height),
                ),
                size(px(right - x0 * cell_width), px(bottom - y0 * cell_height)),
            ),
            color,
        ));
    };
    match mosaic.glyph {
        MosaicGlyph::Sextant(mask) => {
            for bit in 0..6 {
                if mask & (1 << bit) == 0 {
                    continue;
                }
                let x0 = (bit % 2) as f32 * 0.5;
                let y0 = (bit / 2) as f32 / 3.0;
                paint(window, x0, y0, x0 + 0.5, y0 + 1.0 / 3.0, mosaic.foreground);
            }
        }
        MosaicGlyph::Quadrant(mask) => {
            for bit in 0..4 {
                if mask & (1 << bit) == 0 {
                    continue;
                }
                let x0 = (bit % 2) as f32 * 0.5;
                let y0 = (bit / 2) as f32 * 0.5;
                paint(window, x0, y0, x0 + 0.5, y0 + 0.5, mosaic.foreground);
            }
        }
        MosaicGlyph::Rect { x0, y0, x1, y1 } => {
            paint(
                window,
                x0 as f32 / 8.0,
                y0 as f32 / 8.0,
                x1 as f32 / 8.0,
                y1 as f32 / 8.0,
                mosaic.foreground,
            );
        }
        MosaicGlyph::Shade(alpha) => {
            paint(window, 0.0, 0.0, 1.0, 1.0, mosaic.foreground.opacity(alpha));
        }
    }
}

#[derive(Clone)]
struct TerminalCell {
    text: String,
    foreground: Hsla,
    background: Hsla,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
}

impl TerminalCell {
    fn blank(theme: Theme) -> Self {
        Self {
            text: " ".into(),
            foreground: theme.text,
            background: theme.terminal,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct TerminalRunStyle {
    foreground: Hsla,
    background: Hsla,
    bold: bool,
    italic: bool,
    underline: bool,
    strikeout: bool,
}

struct TerminalRun {
    len: usize,
    style: TerminalRunStyle,
}

struct TerminalRow {
    text: String,
    runs: Vec<TerminalRun>,
}

struct TerminalSnapshot {
    rows: Vec<TerminalRow>,
    mosaics: Vec<MosaicInstance>,
    outline_cursor: Option<(usize, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TerminalLink {
    value: String,
    bounds: Match,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TerminalLinkTarget {
    Url(String),
    File(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TerminalCursorStyle {
    Solid,
    Outline,
    Hidden,
}

struct TerminalCursorBlink {
    visible: bool,
    enabled: bool,
    epoch: usize,
    _task: Task<()>,
}

impl TerminalCursorBlink {
    fn new() -> Self {
        Self {
            visible: true,
            enabled: false,
            epoch: 0,
            _task: Task::ready(()),
        }
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.enabled {
            return;
        }

        self.enabled = true;
        self.visible = true;
        let epoch = self.next_epoch();
        self.schedule_blink(epoch, TERMINAL_CURSOR_BLINK_INTERVAL, cx);
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if !self.enabled && self.visible {
            return;
        }

        self.enabled = false;
        self.visible = true;
        self.next_epoch();
        cx.notify();
    }

    fn pause(&mut self, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }

        self.visible = true;
        let epoch = self.next_epoch();
        self.schedule_blink(epoch, TERMINAL_CURSOR_BLINK_PAUSE, cx);
        cx.notify();
    }

    fn blink(&mut self, epoch: usize, cx: &mut Context<Self>) {
        if !self.enabled || epoch != self.epoch {
            return;
        }

        self.visible = !self.visible;
        let epoch = self.next_epoch();
        self.schedule_blink(epoch, TERMINAL_CURSOR_BLINK_INTERVAL, cx);
        cx.notify();
    }

    fn schedule_blink(&mut self, epoch: usize, delay: Duration, cx: &mut Context<Self>) {
        self._task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            if let Some(this) = this.upgrade() {
                this.update(cx, |this, cx| this.blink(epoch, cx));
            }
        });
    }

    fn next_epoch(&mut self) -> usize {
        self.epoch = self.epoch.wrapping_add(1);
        self.epoch
    }

    fn visible(&self) -> bool {
        self.visible
    }
}

/// The ⌘I command bar's phase: describe the command in words, wait on the
/// agent, review — and edit — what it wrote before it touches the prompt.
#[derive(Clone, Copy, Eq, PartialEq)]
enum CommandBarPhase {
    Describe,
    Generating,
    Review,
}

struct TerminalCommandBar {
    input: Entity<TextInput>,
    phase: CommandBarPhase,
    error: Option<SharedString>,
    /// The generating provider's display name — the "via" line in review.
    provider: Option<SharedString>,
    /// Bumps per request; a stale daemon reply cannot land on a bar that
    /// submitted again or reopened.
    generation: u64,
}

pub struct TerminalView {
    session: Option<TerminalSession>,
    command_bar: Option<TerminalCommandBar>,
    error: Option<String>,
    focus_handle: FocusHandle,
    working_directory: PathBuf,
    /// The directory the PTY launched in — `working_directory` drifts
    /// from it as soon as the shell reports a `cd`, so "where this
    /// terminal was spawned" reads here instead.
    spawn_directory: PathBuf,
    /// Basename of the PTY's shell — "zsh", "bash" — what a sidebar row
    /// reports when the terminal sits outside any repository.
    shell_name: String,
    /// An interactive command is in flight — reported by shell
    /// integration, or assumed while a custom command's launch line runs.
    command_running: bool,
    /// A shell-integration `begin` has arrived — the marker a matching
    /// `end` counts against. A command launch seeds `command_running`
    /// itself, and bash emits an `end` at the first prompt before the
    /// launch line runs, so the seed alone can't stand in for a begin.
    command_began_seen: bool,
    /// Exit status of the most recent command — `None` until one reports.
    last_command_exit: Option<i32>,
    /// When the most recent command started (unix seconds) — the row's
    /// "…ago" label while a run is in flight.
    last_command_started_at: Option<u64>,
    title: String,
    /// What `ResetTitle` restores — the localized "Terminal" for a plain
    /// shell, the command's display name for a custom command.
    default_title: String,
    /// A name the user set from the sidebar — wins over the OSC-set title
    /// until cleared.
    custom_title: Option<String>,
    exited: bool,
    scroll_accumulator: f32,
    panel_width: f32,
    /// Terminals rendered inside another surface — the provider setup block
    /// in Settings — size their grid to the painted bounds instead of the
    /// right panel's viewport math.
    embedded: bool,
    /// Advance width of one grid cell, measured from the terminal font on
    /// first render so grid math matches what `StyledText` actually lays out.
    /// Keyed by family and size so a font change re-measures instead of
    /// wrapping the grid at the old face's advance.
    measured_cell_width: Option<(SharedString, f32, f32)>,
    scrollbar_state: Rc<ScrollbarState>,
    grid_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    selecting: bool,
    /// The link a link-modifier press landed on, awaiting release. While it
    /// lives the press is swallowed — the program never sees it and no
    /// selection starts.
    link_gesture: Option<TerminalLink>,
    /// The grid cell the last forwarded motion report used — reports only
    /// repeat while the cell actually changes.
    last_mouse_cell: Option<TerminalPoint>,
    hovered_link: Option<TerminalLink>,
    /// Localhost URLs this view has already surfaced, so the overlap between
    /// one output scan and the next cannot re-report them.
    reported_localhost_urls: HashSet<String>,
    cursor_blink: gpui::Entity<TerminalCursorBlink>,
    cursor_focus_tracking_started: bool,
    context_menu: ContextMenuHandle,
    _subscriptions: Vec<Subscription>,
}

impl TerminalView {
    pub fn with_launch(
        working_directory: PathBuf,
        launch: TerminalLaunch,
        cx: &mut Context<Self>,
    ) -> Self {
        let default_title = match &launch {
            TerminalLaunch::Shell => tr!("right_panel.terminal"),
            TerminalLaunch::CustomCommand(command) => command.display_name().to_owned(),
            TerminalLaunch::Program { program, .. } => program
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned(),
        };
        let shell = match &launch {
            TerminalLaunch::Shell => crate::command_env::default_terminal_shell(),
            TerminalLaunch::CustomCommand(command) => {
                crate::custom_commands::command_shell(command)
            }
            TerminalLaunch::Program { program, .. } => program.clone(),
        };
        let shell_name = shell
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let runs_a_command = matches!(launch, TerminalLaunch::CustomCommand(_));
        let terminal_cwd = working_directory.clone();
        cx.spawn(async move |this, cx| {
            let started = cx
                .background_executor()
                .spawn(async move { TerminalSession::new(&terminal_cwd, &launch, 52, 36) })
                .await;
            if this
                .update(cx, |this, cx| {
                    match started {
                        Ok(session) => this.session = Some(session),
                        Err(error) => {
                            this.error = Some(error.to_string());
                            // A command that never launched still owns a
                            // pending run — resolve it so its toast isn't
                            // pinned forever.
                            cx.emit(TerminalViewEvent::CommandFinished(None));
                        }
                    }
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(24))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if this.poll(cx) {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let context_menu = ContextMenuHandle::new(cx);
        let cursor_blink = cx.new(|_| TerminalCursorBlink::new());
        let subscriptions = vec![cx.observe(&cursor_blink, |_, _, cx| cx.notify())];

        Self {
            session: None,
            command_bar: None,
            error: None,
            focus_handle: cx.focus_handle(),
            title: default_title.clone(),
            default_title,
            custom_title: None,
            spawn_directory: working_directory.clone(),
            working_directory,
            shell_name,
            // A custom command's launch line is the terminal's first
            // command — running from spawn until its report lands.
            command_running: runs_a_command,
            command_began_seen: false,
            last_command_exit: None,
            last_command_started_at: runs_a_command.then(crate::model::unix_time),
            exited: false,
            scroll_accumulator: 0.0,
            panel_width: DEFAULT_RIGHT_PANEL_WIDTH,
            embedded: false,
            measured_cell_width: None,
            scrollbar_state: ScrollbarState::new(),
            grid_bounds: Rc::new(Cell::new(None)),
            selecting: false,
            link_gesture: None,
            last_mouse_cell: None,
            hovered_link: None,
            reported_localhost_urls: HashSet::new(),
            cursor_blink,
            cursor_focus_tracking_started: false,
            context_menu,
            _subscriptions: subscriptions,
        }
    }

    /// A terminal embedded in another surface rather than filling the right
    /// panel: the grid derives its columns and rows from the painted bounds,
    /// so the parent can give it any fixed height.
    pub fn embedded(
        working_directory: PathBuf,
        launch: TerminalLaunch,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::with_launch(working_directory, launch, cx);
        view.embedded = true;
        view
    }

    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    /// The directory the PTY spawned in — unlike `working_directory`,
    /// which follows the shell's cwd reports, this never moves.
    pub fn spawn_directory(&self) -> &Path {
        &self.spawn_directory
    }

    /// Basename of the shell the PTY runs — the row label for a terminal
    /// outside any repository.
    pub fn shell_name(&self) -> &str {
        &self.shell_name
    }

    /// Whether an interactive command is in flight — shell-integration
    /// begin without its end, or a custom command's launch line.
    pub fn command_running(&self) -> bool {
        self.command_running
    }

    /// Exit status of the most recent command — `None` until one reports
    /// (a shell without integration reports nothing at all).
    pub fn last_command_exit(&self) -> Option<i32> {
        self.last_command_exit
    }

    /// When the most recent command started (unix seconds) — `None` until
    /// a command runs.
    pub fn last_command_started_at(&self) -> Option<u64> {
        self.last_command_started_at
    }

    /// The last `count` non-blank lines on the terminal's screen — empty
    /// until the PTY session has spawned and printed something.
    pub fn output_tail(&self, count: usize) -> Vec<String> {
        self.session
            .as_ref()
            .map(|session| session.tail_lines(count))
            .unwrap_or_default()
    }

    /// The surface's current title — a sidebar rename while one is set,
    /// else the shell's OSC-set name while the program running in it
    /// controls it, the localized default otherwise.
    pub fn title(&self) -> &str {
        self.custom_title.as_deref().unwrap_or(&self.title)
    }

    /// Set or clear the user-assigned name shown wherever the terminal's
    /// title appears.
    pub fn set_custom_title(&mut self, title: Option<String>, cx: &mut Context<Self>) {
        self.custom_title = title;
        cx.notify();
    }

    pub fn panel_width(&self) -> f32 {
        self.panel_width
    }

    pub fn set_panel_width(&mut self, width: f32) {
        self.panel_width = width;
    }

    pub fn refresh_localized_text(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.default_title.as_str(),
            "Terminal" | "终端" | "ターミナル"
        ) {
            self.default_title = tr!("right_panel.terminal");
        }
        if matches!(self.title.as_str(), "Terminal" | "终端" | "ターミナル") {
            self.title = tr!("right_panel.terminal");
            cx.notify();
        }
    }

    fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = self
            .session
            .as_ref()
            .is_some_and(|session| session.take_dirty());
        if changed && let Some(url) = self.detect_localhost_url() {
            cx.emit(TerminalViewEvent::LocalhostUrl(url));
        }
        let Some(session) = &self.session else {
            return changed;
        };
        while let Ok(event) = session.ui_events.try_recv() {
            changed = true;
            match event {
                TerminalUiEvent::Title(title) => self.title = title,
                TerminalUiEvent::ResetTitle => self.title = self.default_title.clone(),
                TerminalUiEvent::ClipboardStore(text) => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
                TerminalUiEvent::ClipboardLoad(formatter) => {
                    let text = cx
                        .read_from_clipboard()
                        .and_then(|item| item.text())
                        .unwrap_or_default();
                    session.write(formatter(&text).into_bytes());
                }
                TerminalUiEvent::CommandExit(code) => {
                    self.command_running = false;
                    self.last_command_exit = Some(code);
                    cx.emit(TerminalViewEvent::ActivityChanged);
                    cx.emit(TerminalViewEvent::CommandFinished(Some(code)));
                }
                TerminalUiEvent::CommandBegan => {
                    self.command_running = true;
                    self.command_began_seen = true;
                    self.last_command_started_at = Some(crate::model::unix_time());
                    cx.emit(TerminalViewEvent::ActivityChanged);
                }
                TerminalUiEvent::CommandEnded(code) => {
                    // An end only counts when a begin announced a run —
                    // bash emits one at every prompt regardless, and the
                    // first prompt's status is the rc file's, not a
                    // command's. A command launch's seeded
                    // `command_running` can't stand in for that begin.
                    if self.command_running && self.command_began_seen {
                        self.command_running = false;
                        self.last_command_exit = Some(code);
                        cx.emit(TerminalViewEvent::ActivityChanged);
                        cx.emit(TerminalViewEvent::CommandFinished(Some(code)));
                    }
                }
                TerminalUiEvent::Cwd(path) => {
                    if path != self.working_directory {
                        self.working_directory = path;
                        cx.emit(TerminalViewEvent::ActivityChanged);
                    }
                }
                TerminalUiEvent::Exited(code) => {
                    self.exited = true;
                    // A shell that dies mid-command can never report the
                    // end — stop the spinner.
                    self.command_running = false;
                    cx.emit(TerminalViewEvent::ActivityChanged);
                    // The completion event goes first: a script that exits
                    // the shell itself still resolves its run before the
                    // exit event can close the surface out from under it.
                    // `None` — a signal kill or a PTY teardown — resolves
                    // the run as a failure rather than leaving it pending.
                    cx.emit(TerminalViewEvent::CommandFinished(code));
                    cx.emit(TerminalViewEvent::Exited);
                }
            }
        }
        changed
    }

    /// Scans the grid lines added since the previous poll for a localhost
    /// URL — how a dev server announces its port — and returns the best
    /// candidate not reported before. `https://<id>.localhost` beats other
    /// loopback forms when both appear.
    fn detect_localhost_url(&mut self) -> Option<String> {
        let session = self.session.as_mut()?;
        let term = session.term.lock();
        let url = scan_grid_localhost_url(
            &term,
            &mut session.localhost_scan_watermark,
            &self.reported_localhost_urls,
        );
        drop(term);
        if let Some(url) = &url {
            if self.reported_localhost_urls.len() >= MAX_REPORTED_LOCALHOST_URLS {
                self.reported_localhost_urls.clear();
            }
            self.reported_localhost_urls.insert(url.clone());
        }
        url
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The command bar's field sits inside this view's dispatch path, so
        // its keystrokes bubble through here — while it owns focus, nothing
        // may reach the PTY.
        if self.command_bar_focused(window, cx) {
            return;
        }
        self.pause_cursor_blink(cx);
        let keystroke = &event.keystroke;
        // ⌘I opens the command bar: describe a command, review what the
        // session's agent writes, then insert it at the prompt.
        if terminal_clipboard_modifier_pressed(&keystroke.modifiers)
            && keystroke.key.eq_ignore_ascii_case("i")
        {
            self.open_command_bar(window, cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if terminal_clipboard_modifier_pressed(&keystroke.modifiers)
            && keystroke.key.eq_ignore_ascii_case("c")
        {
            self.copy_selection(cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        if terminal_clipboard_modifier_pressed(&keystroke.modifiers)
            && keystroke.key.eq_ignore_ascii_case("a")
        {
            self.select_all(cx);
            window.prevent_default();
            cx.stop_propagation();
            return;
        }
        // ⌘K stays with the command palette binding; only ⌘⇧K clears here.
        if terminal_clipboard_modifier_pressed(&keystroke.modifiers)
            && keystroke.modifiers.shift
            && keystroke.key.eq_ignore_ascii_case("k")
        {
            if let Some(session) = &self.session {
                session.clear_scrollback();
            }
            window.prevent_default();
            cx.stop_propagation();
            return;
        }

        let Some(session) = &self.session else {
            return;
        };
        if terminal_clipboard_modifier_pressed(&keystroke.modifiers)
            && keystroke.key.eq_ignore_ascii_case("v")
        {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                session.term.lock().selection = None;
                let bytes = bracketed_paste(text, session.mode());
                session.scroll_to_bottom();
                session.write(bytes);
                session.dirty.store(true, Ordering::Release);
                window.prevent_default();
                cx.stop_propagation();
            }
            return;
        }

        if let Some(bytes) = terminal_key_bytes(keystroke, session.mode()) {
            session.term.lock().selection = None;
            session.scroll_to_bottom();
            session.write(bytes);
            session.dirty.store(true, Ordering::Release);
            window.prevent_default();
            cx.stop_propagation();
        }
    }

    fn cell_width(&self) -> f32 {
        self.measured_cell_width
            .as_ref()
            .map(|(_, _, width)| *width)
            .unwrap_or(TERMINAL_CELL_WIDTH)
    }

    fn grid_point_for_position(
        &self,
        position: Point<Pixels>,
        clamp_to_grid: bool,
    ) -> Option<(TerminalPoint, Side)> {
        let bounds = self.grid_bounds.get()?;
        let session = self.session.as_ref()?;
        let display_offset = session.term.lock().grid().display_offset();
        terminal_grid_point(
            bounds,
            position,
            self.cell_width(),
            session.cell_size.1,
            session.grid_size.0,
            session.grid_size.1,
            display_offset,
            clamp_to_grid,
        )
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The overlay scrollbar's listeners already ran (bubble order is
        // reverse registration and it paints above the grid) but do not stop
        // propagation; a grab or track click must not also start a selection
        // underneath the bar.
        if self.scrollbar_state.engaged() {
            return;
        }
        let mouse_mode = self
            .session
            .as_ref()
            .is_some_and(|session| session.mouse_mode(event.modifiers.shift));
        if event.button != MouseButton::Left && !mouse_mode {
            // Outside mouse mode the context menu owns right clicks and
            // middle clicks do nothing.
            return;
        }
        window.focus(&self.focus_handle, cx);
        let Some((point, side)) = self.grid_point_for_position(event.position, false) else {
            return;
        };

        // A link-modifier press on a link starts a gesture rather than
        // acting: the press is swallowed and the release opens the link when
        // it lands on the same one.
        if event.button == MouseButton::Left
            && link_modifier_pressed(&event.modifiers, cx)
            && (open_links_in_mouse_mode(cx) || !mouse_mode)
        {
            self.link_gesture = self
                .session
                .as_mut()
                .and_then(|session| session.link_at(point))
                .filter(|link| {
                    terminal_link_target(&link.value, self.working_directory.as_path()).is_some()
                });
            if let Some(link) = self.link_gesture.clone() {
                self.selecting = false;
                self.hovered_link = Some(link);
                window.prevent_default();
                cx.stop_propagation();
                cx.notify();
                return;
            }
        }

        if mouse_mode {
            if let Some(session) = &self.session
                && let Some(report) =
                    mouse_button_report(point, event.button, event.modifiers, true, session.mode())
            {
                session.write(report);
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }

        let Some(session) = &self.session else {
            return;
        };

        let mut term = session.term.lock();
        if event.modifiers.shift
            && let Some(selection) = term.selection.as_mut()
        {
            selection.update(point, side);
        } else {
            let selection_type = match event.click_count {
                2 => SelectionType::Semantic,
                count if count >= 3 => SelectionType::Lines,
                _ => SelectionType::Simple,
            };
            term.selection = Some(Selection::new(selection_type, point, side));
        }
        drop(term);

        self.selecting = true;
        session.dirty.store(true, Ordering::Release);
        cx.stop_propagation();
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        // A live link gesture keeps swallowing the press until release;
        // dragging off the link's bounds cancels it first.
        if let Some(link) = &self.link_gesture {
            let off_link = self
                .grid_point_for_position(event.position, true)
                .is_none_or(|(point, _)| !link.bounds.contains(&point));
            if off_link {
                self.link_gesture = None;
                if self.set_hovered_link(None) {
                    cx.notify();
                }
            } else {
                return;
            }
        }

        if self.selecting {
            let hover_changed = self.set_hovered_link(None);
            if event.pressed_button != Some(MouseButton::Left) {
                if hover_changed {
                    cx.notify();
                }
                return;
            }
            let Some((point, side)) = self.grid_point_for_position(event.position, true) else {
                return;
            };
            let Some(session) = &self.session else {
                return;
            };
            if let Some(selection) = session.term.lock().selection.as_mut() {
                selection.update(point, side);
                session.dirty.store(true, Ordering::Release);
                cx.stop_propagation();
                cx.notify();
            }
            return;
        }

        // While the program reports mouse input, motion belongs to it and
        // there is no local link hover. Reports only repeat on a cell
        // change.
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.mouse_mode(event.modifiers.shift))
        {
            let hover_changed = self.set_hovered_link(None);
            if let Some((point, _)) = self.grid_point_for_position(event.position, true)
                && self.last_mouse_cell != Some(point)
            {
                self.last_mouse_cell = Some(point);
                if let Some(session) = &self.session
                    && let Some(report) = mouse_moved_report(
                        point,
                        event.pressed_button,
                        event.modifiers,
                        session.mode(),
                    )
                {
                    session.write(report);
                }
            }
            if hover_changed {
                cx.notify();
            }
            return;
        }

        if self.refresh_hovered_link(link_modifier_pressed(&event.modifiers, cx), event.position) {
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let was_selecting = self.selecting;
        self.selecting = false;
        self.last_mouse_cell = None;

        // Resolve a link gesture: releasing on the link the press started on
        // opens it; releasing anywhere else just drops the gesture.
        if let Some(link) = self.link_gesture.take() {
            self.hovered_link = None;
            let same_link = self
                .grid_point_for_position(event.position, false)
                .and_then(|(point, _)| self.session.as_mut()?.link_at(point))
                .is_some_and(|current| current == link);
            if same_link
                && let Some(target) =
                    terminal_link_target(&link.value, self.working_directory.as_path())
            {
                match target {
                    TerminalLinkTarget::Url(url) => cx.open_url(&url),
                    TerminalLinkTarget::File(path) => {
                        crate::platform::reveal_in_file_manager(&path, cx)
                    }
                }
            }
            cx.notify();
            return;
        }

        if let Some(session) = &self.session
            && session.mouse_mode(event.modifiers.shift)
            && let Some((point, _)) = self.grid_point_for_position(event.position, true)
            && let Some(report) =
                mouse_button_report(point, event.button, event.modifiers, false, session.mode())
        {
            session.write(report);
        }

        // Copy-on-select resolves at release, once the drag's selection is
        // final. The selection itself stays visible.
        if was_selecting && copy_on_select(cx) {
            self.copy_selection(cx);
        }
    }

    fn on_mouse_exit(&mut self, _: &MouseExitEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.set_hovered_link(None) {
            cx.notify();
        }
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.refresh_hovered_link(
            link_modifier_pressed(&event.modifiers, cx)
                && !self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.mouse_mode(event.modifiers.shift)),
            window.mouse_position(),
        ) {
            cx.notify();
        }
    }

    fn refresh_hovered_link(&mut self, command_pressed: bool, position: Point<Pixels>) -> bool {
        let link = if command_pressed {
            self.grid_point_for_position(position, false)
                .and_then(|(point, _)| self.session.as_mut()?.link_at(point))
        } else {
            None
        };
        self.set_hovered_link(link)
    }

    fn set_hovered_link(&mut self, link: Option<TerminalLink>) -> bool {
        if self.hovered_link == link {
            return false;
        }
        self.hovered_link = link;
        true
    }

    fn selected_text(&self) -> Option<String> {
        self.session
            .as_ref()?
            .term
            .lock()
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    fn copy_selection(&self, cx: &mut App) {
        if let Some(text) = self.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, text: String, cx: &mut Context<Self>) {
        self.pause_cursor_blink(cx);
        let Some(session) = &self.session else {
            return;
        };
        session.term.lock().selection = None;
        session.scroll_to_bottom();
        session.write(bracketed_paste(text, session.mode()));
        session.dirty.store(true, Ordering::Release);
        cx.notify();
    }

    fn open_command_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Reopening while the bar is up just returns its focus.
        if let Some(bar) = &self.command_bar {
            let focus = bar.input.read(cx).focus();
            window.focus(&focus, cx);
            return;
        }
        // Embedded terminals are provider-setup probes, not user shells.
        if self.embedded || self.exited || self.session.is_none() {
            return;
        }
        let input = cx.new(|cx| {
            TextInput::new(window, cx)
                .multi_line()
                .auto_height()
                .max_lines(4)
                .placeholder(tr!("terminal_command.placeholder"))
        });
        let focus = input.read(cx).focus();
        self.command_bar = Some(TerminalCommandBar {
            input,
            phase: CommandBarPhase::Describe,
            error: None,
            provider: None,
            generation: 0,
        });
        // The input joins the dispatch tree only after the bar draws —
        // the same two-frame deferral the commit dialog uses.
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| window.focus(&focus, cx));
        });
        cx.notify();
    }

    fn close_command_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_bar.take().is_none() {
            return;
        }
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Whether the bar's field owns focus — while it does, no keystroke
    /// here may reach the PTY.
    fn command_bar_focused(&self, window: &Window, cx: &App) -> bool {
        self.command_bar
            .as_ref()
            .is_some_and(|bar| bar.input.read(cx).focus().is_focused(window))
    }

    fn command_bar_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(phase) = self.command_bar.as_ref().map(|bar| bar.phase) else {
            return;
        };
        match phase {
            CommandBarPhase::Describe => self.command_bar_generate(cx),
            // A request is in flight; Enter does nothing until it lands.
            CommandBarPhase::Generating => {}
            CommandBarPhase::Review => self.insert_generated_command(false, window, cx),
        }
        cx.notify();
    }

    fn command_bar_generate(&mut self, cx: &mut Context<Self>) {
        let Some(bar) = self.command_bar.as_mut() else {
            return;
        };
        let request = bar.input.read(cx).content().trim().to_owned();
        if request.is_empty() {
            return;
        }
        bar.phase = CommandBarPhase::Generating;
        bar.generation += 1;
        bar.error = None;
        bar.provider = None;
        let generation = bar.generation;
        bar.input.update(cx, |input, _| input.set_read_only(true));
        let scrollback = self.command_bar_scrollback();
        cx.emit(TerminalViewEvent::GenerateCommand {
            generation,
            request,
            scrollback,
            cwd: self.working_directory.clone(),
            shell: self.shell_name.clone(),
        });
    }

    /// The app's daemon reply for a `GenerateCommand` request — the
    /// generation check keeps a stale answer off a bar that moved on.
    pub fn apply_command_generation(
        &mut self,
        generation: u64,
        provider: SharedString,
        result: std::result::Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(bar) = self
            .command_bar
            .as_mut()
            .filter(|bar| bar.generation == generation && bar.phase == CommandBarPhase::Generating)
        else {
            return;
        };
        match result {
            Ok(command) => {
                bar.provider = Some(provider);
                bar.phase = CommandBarPhase::Review;
                bar.input.update(cx, |input, cx| {
                    input.set_read_only(false);
                    input.set_content(command, cx);
                });
            }
            Err(error) => {
                bar.phase = CommandBarPhase::Describe;
                bar.error = Some(error.into());
                bar.input.update(cx, |input, _| input.set_read_only(false));
            }
        }
        cx.notify();
    }

    /// Write the reviewed command into the shell's edit buffer without
    /// executing it — `run` follows the paste with a carriage return.
    /// Multi-line text needs bracketed paste: without it the shell would
    /// execute each line as it arrives. Only the Review phase may insert;
    /// elsewhere the field still holds the request, not a command.
    fn insert_generated_command(&mut self, run: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bar) = self
            .command_bar
            .as_mut()
            .filter(|bar| bar.phase == CommandBarPhase::Review)
        else {
            return;
        };
        let command = bar.input.read(cx).content().trim().to_owned();
        if command.is_empty() {
            self.close_command_bar(window, cx);
            return;
        }
        let Some(session) = &self.session else {
            return;
        };
        let mode = session.mode();
        if command.contains('\n') && !mode.contains(TermMode::BRACKETED_PASTE) {
            bar.error = Some(tr!("terminal_command.multiline_blocked").into());
            cx.notify();
            return;
        }
        session.term.lock().selection = None;
        session.write(bracketed_paste(command, mode));
        if run {
            session.write(b"\r".to_vec());
        }
        session.dirty.store(true, Ordering::Release);
        self.close_command_bar(window, cx);
    }

    /// The bar overlays the grid's top edge; the terminal keeps streaming
    /// behind it. Insert writes into the shell's edit buffer — plain Enter
    /// pastes without running, ⌘↵ pastes and runs, Escape dismisses.
    fn command_bar_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let bar = self.command_bar.as_ref()?;
        let theme = Theme::current(cx);
        let input_focus = bar.input.read(cx).focus();
        let key_hint = |action: &dyn gpui::Action| {
            ShortcutHint::action_in(action, &input_focus).resolve(window, cx)
        };
        // Multi-line insert is only safe when the shell asked for
        // bracketed paste — otherwise each line would execute as it lands.
        let multi_line_blocked = bar.phase == CommandBarPhase::Review
            && bar.input.read(cx).content().contains('\n')
            && self
                .session
                .as_ref()
                .is_none_or(|session| !session.mode().contains(TermMode::BRACKETED_PASTE));

        let mut hints: Vec<String> = Vec::new();
        match bar.phase {
            CommandBarPhase::Generating => hints.push(tr!("terminal_command.generating")),
            CommandBarPhase::Describe => {
                if let Some(key) = key_hint(&ConfirmTerminalCommand) {
                    hints.push(format!("{key} {}", tr_cow!("terminal_command.generate")));
                }
            }
            CommandBarPhase::Review => {
                if multi_line_blocked {
                    hints.push(tr!("terminal_command.multiline_blocked"));
                } else {
                    if let Some(key) = key_hint(&ConfirmTerminalCommand) {
                        hints.push(format!("{key} {}", tr_cow!("terminal_command.insert")));
                    }
                    if let Some(key) = key_hint(&RunTerminalCommand) {
                        hints.push(format!("{key} {}", tr_cow!("terminal_command.run")));
                    }
                }
            }
        }
        if !multi_line_blocked && let Some(key) = key_hint(&DismissTerminalCommand) {
            hints.push(format!("{key} {}", tr_cow!("terminal_command.dismiss")));
        }
        let mut footer = hints.join(" · ");
        if bar.phase == CommandBarPhase::Review
            && let Some(provider) = &bar.provider
        {
            footer = format!(
                "{} · {footer}",
                tr!("terminal_command.via", provider => provider.as_str())
            );
        }

        Some(
            div()
                .id("terminal-command-bar")
                .key_context("TerminalCommandBar")
                .absolute()
                .top(px(6.0))
                .left(px(TERMINAL_PADDING_X))
                // The overlay scrollbar owns the grid's right edge.
                .right(px(TERMINAL_PADDING_X + 10.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(10.0))
                .border(hairline())
                .border_color(theme.border_subtle)
                .bg(theme.composer)
                .shadow_lg()
                // A click on the bar's padding must not refocus the shell.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_action(cx.listener(|this, _: &ConfirmTerminalCommand, window, cx| {
                    this.command_bar_confirm(window, cx)
                }))
                .on_action(cx.listener(|this, _: &RunTerminalCommand, window, cx| {
                    match this.command_bar.as_ref().map(|bar| bar.phase) {
                        Some(CommandBarPhase::Review) => {
                            this.insert_generated_command(true, window, cx)
                        }
                        // Outside review the field holds a request, so the
                        // chord means the same thing Enter does.
                        _ => this.command_bar_confirm(window, cx),
                    }
                }))
                .on_action(cx.listener(|this, _: &DismissTerminalCommand, window, cx| {
                    this.close_command_bar(window, cx)
                }))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(div().pt(px(3.0)).child(crate::ui::icon(
                            "icons/sparkle.svg",
                            12.5,
                            theme.accent,
                        )))
                        .child(div().min_w_0().flex_1().child(bar.input.clone())),
                )
                .when_some(bar.error.clone(), |bar_el, error| {
                    bar_el.child(
                        div()
                            .pl(px(20.0))
                            .text_size(sp(11.0))
                            .text_color(theme.danger)
                            .child(error),
                    )
                })
                .when(!footer.is_empty(), |bar_el| {
                    bar_el.child(
                        div()
                            .pl(px(20.0))
                            .text_size(sp(11.0))
                            .text_color(theme.text_ghost)
                            .child(footer),
                    )
                })
                .into_any_element(),
        )
    }

    /// The bottom grid lines — scrollback plus screen — sent with a
    /// generation request so "that failed" has a referent.
    fn command_bar_scrollback(&self) -> String {
        let Some(session) = &self.session else {
            return String::new();
        };
        let term = session.term.lock();
        let bottom = term.bottommost_line();
        let top = term
            .topmost_line()
            .max(Line(bottom.0 - COMMAND_BAR_CONTEXT_LINES as i32 + 1));
        let text = term.bounds_to_string(
            TerminalPoint::new(top, Column(0)),
            TerminalPoint::new(bottom, term.last_column()),
        );
        if text.len() <= COMMAND_BAR_CONTEXT_MAX_BYTES {
            return text;
        }
        let mut start = text.len() - COMMAND_BAR_CONTEXT_MAX_BYTES;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..].to_owned()
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        let Some(session) = &self.session else {
            return;
        };
        let mut term = session.term.lock();
        let start = TerminalPoint::new(term.topmost_line(), Column(0));
        let end = TerminalPoint::new(term.bottommost_line(), term.last_column());
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
        drop(term);
        session.dirty.store(true, Ordering::Release);
        cx.notify();
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.session else {
            return;
        };
        let delta = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / session.cell_size.1,
            ScrollDelta::Lines(delta) => delta.y,
        };
        self.scroll_accumulator += delta;
        let lines = self.scroll_accumulator.trunc() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_accumulator -= lines as f32;

        // While the program reports mouse input the wheel is its input too:
        // each line is one scroll-button press at the cursor's cell.
        if session.mouse_mode(event.modifiers.shift) {
            if let Some((point, _)) = self.grid_point_for_position(event.position, true)
                && let Some(reports) = scroll_report(point, lines, event, session.mode())
            {
                for report in reports {
                    session.write(report);
                }
            }
            cx.stop_propagation();
            cx.notify();
            return;
        }

        session.scroll(lines);
        cx.stop_propagation();
        cx.notify();
    }

    fn ensure_cursor_focus_tracking(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.cursor_focus_tracking_started {
            return;
        }
        self.cursor_focus_tracking_started = true;

        let focus_handle = self.focus_handle.clone();
        self._subscriptions.extend([
            cx.observe_window_activation(window, |terminal, window, cx| {
                terminal.update_cursor_blinking(window, cx);
            }),
            cx.on_focus(&focus_handle, window, |terminal, window, cx| {
                terminal.update_cursor_blinking(window, cx);
            }),
            cx.on_blur(&focus_handle, window, |terminal, window, cx| {
                terminal.update_cursor_blinking(window, cx);
            }),
        ]);
        self.update_cursor_blinking(window, cx);
    }

    fn update_cursor_blinking(&mut self, window: &Window, cx: &mut Context<Self>) {
        let focused = window.is_window_active() && self.focus_handle.is_focused(window);
        self.cursor_blink.update(cx, |cursor, cx| {
            if focused {
                cursor.start(cx);
            } else {
                cursor.stop(cx);
            }
        });
    }

    fn pause_cursor_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink.update(cx, |cursor, cx| cursor.pause(cx));
    }
}

impl EventEmitter<TerminalViewEvent> for TerminalView {}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_cursor_focus_tracking(window, cx);
        let theme = Theme::current(cx);
        let selection_color = theme.selection;
        let viewport = window.viewport_size();
        let panel_width = self.panel_width;
        let body_height = (f32::from(viewport.height) - 48.0).max(120.0);
        // The rows are laid out by `StyledText` at the font's own advance, so
        // the grid must be sized from that same measured advance or the text
        // wraps short of (or past) the panel edge.
        let font_size = font_size(cx);
        // GPUI snaps every absolute length to whole device pixels, so each
        // row paints at the snapped pitch. Grid math — the PTY's cell size,
        // mouse hit testing, scroll deltas — must divide by that same pitch
        // or painted rows drift off the input grid.
        let cell_height = f32::from(window.pixel_snap(px(terminal_cell_height(font_size))));
        let code_family = crate::fonts::current(cx).code;
        let cell_width = match &self.measured_cell_width {
            Some((family, size, width)) if *family == code_family && *size == font_size => *width,
            _ => {
                let text_system = cx.text_system();
                let font_id = text_system.resolve_font(&terminal_font(&code_family));
                let width = text_system
                    .advance(font_id, px(font_size), 'm')
                    .map_or(TERMINAL_CELL_WIDTH, |advance| f32::from(advance.width));
                self.measured_cell_width = Some((code_family.clone(), font_size, width));
                width
            }
        };
        // The painted bounds already sit inside the grid's padding, so the
        // embedded math doesn't subtract it again. Before the first prepaint
        // an embedded terminal falls through to the panel defaults; the PTY
        // resize lands a frame later.
        let embedded_bounds = if self.embedded {
            self.grid_bounds.get()
        } else {
            None
        };
        let (columns, rows) = match embedded_bounds {
            Some(bounds) => (
                (f32::from(bounds.size.width) / cell_width)
                    .floor()
                    .max(TERMINAL_MIN_COLUMNS as f32) as usize,
                (f32::from(bounds.size.height) / cell_height)
                    .floor()
                    .max(TERMINAL_MIN_ROWS as f32) as usize,
            ),
            None => (
                ((panel_width - TERMINAL_PADDING_X * 2.0) / cell_width)
                    .floor()
                    .max(TERMINAL_MIN_COLUMNS as f32) as usize,
                ((body_height - TERMINAL_PADDING_Y * 2.0) / cell_height)
                    .floor()
                    .max(TERMINAL_MIN_ROWS as f32) as usize,
            ),
        };

        let terminal_focused = window.is_window_active() && self.focus_handle.is_focused(window);
        let cursor_style =
            terminal_cursor_style(terminal_focused, self.cursor_blink.read(cx).visible());
        if let Some(session) = self.session.as_mut() {
            session.resize(columns, rows, cell_width, cell_height);
        }
        if self.selecting {
            self.set_hovered_link(None);
        } else {
            self.refresh_hovered_link(
                link_modifier_pressed(&window.modifiers(), cx)
                    && !self
                        .session
                        .as_ref()
                        .is_some_and(|session| session.mouse_mode(window.modifiers().shift)),
                window.mouse_position(),
            );
        }
        let hovered_link = self.hovered_link.as_ref().map(|link| &link.bounds);
        let snapshot = self
            .session
            .as_ref()
            .map(|session| session.snapshot(theme, selection_color, cursor_style, hovered_link));
        let mut screen = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .relative()
            .cursor_text()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_exit(cx.listener(Self::on_mouse_exit))
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed));

        if self.hovered_link.is_some() {
            screen = screen.cursor_pointer();
        }

        if let Some(snapshot) = snapshot {
            let TerminalSnapshot {
                rows: snapshot_rows,
                mosaics,
                outline_cursor,
            } = snapshot;
            for row in snapshot_rows {
                let runs = row
                    .runs
                    .into_iter()
                    .map(|run| {
                        let mut run_font = terminal_font(&code_family);
                        if run.style.bold {
                            run_font.weight = FontWeight::BOLD;
                        }
                        if run.style.italic {
                            run_font.style = FontStyle::Italic;
                        }
                        TextRun {
                            len: run.len,
                            font: run_font,
                            color: run.style.foreground,
                            background_color: Some(run.style.background),
                            underline: run.style.underline.then_some(UnderlineStyle {
                                thickness: px(1.0),
                                color: Some(run.style.foreground),
                                wavy: false,
                            }),
                            strikethrough: run.style.strikeout.then_some(StrikethroughStyle {
                                thickness: px(1.0),
                                color: Some(run.style.foreground),
                            }),
                        }
                    })
                    .collect::<Vec<_>>();
                screen = screen.child(
                    div()
                        .h(px(cell_height))
                        .flex_none()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(font_size))
                        .line_height(px(cell_height))
                        .child(StyledText::new(row.text).with_runs(runs)),
                );
            }
            if !mosaics.is_empty() {
                screen = screen.child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            for mosaic in &mosaics {
                                paint_mosaic(
                                    window,
                                    point(
                                        bounds.origin.x + px(mosaic.column as f32 * cell_width),
                                        bounds.origin.y + px(mosaic.row as f32 * cell_height),
                                    ),
                                    cell_width,
                                    cell_height,
                                    mosaic,
                                );
                            }
                        },
                    )
                    .absolute()
                    .inset_0(),
                );
            }
            if let Some((row, column)) = outline_cursor {
                screen = screen.child(
                    div()
                        .absolute()
                        .left(px(column as f32 * cell_width))
                        .top(px(row as f32 * cell_height))
                        .w(px(cell_width))
                        .h(px(cell_height))
                        .border(hairline())
                        .border_color(theme.text),
                );
            }
        } else {
            screen = screen.child(
                div()
                    .p(px(12.0))
                    .text_size(sp(12.5))
                    .line_height(sp(17.0))
                    .text_color(if self.error.is_some() {
                        theme.danger
                    } else {
                        theme.text_tertiary
                    })
                    .child(
                        self.error
                            .clone()
                            .unwrap_or_else(|| tr!("terminal.starting")),
                    ),
            );
        }

        let grid_bounds = self.grid_bounds.clone();
        screen = screen.child(
            canvas(
                move |bounds, _, _| grid_bounds.set(Some(bounds)),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );

        let context_terminal = cx.entity();
        let screen = context_menu(
            div().size_full().child(screen),
            "terminal-context-menu",
            &self.context_menu,
            move |cx| {
                let has_selection = context_terminal.read(cx).selected_text().is_some();
                let can_paste = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text())
                    .is_some_and(|text| !text.is_empty());
                let has_session = context_terminal.read(cx).session.is_some();

                let copy_terminal = context_terminal.clone();
                let paste_terminal = context_terminal.clone();
                let select_all_terminal = context_terminal.clone();
                let command_terminal = context_terminal.clone();
                // The chords are hand-rolled in `on_key_down` rather than
                // registered as bindings, so these labels are authored text.
                vec![
                    MenuItem::new(tr!("menu.copy"), move |_, cx| {
                        let selected_text = { copy_terminal.read(cx).selected_text() };
                        if let Some(text) = selected_text {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                        }
                    })
                    .shortcut(crate::platform::primary_shortcut("⌘C", "Ctrl+Shift+C"))
                    .disabled(!has_selection),
                    MenuItem::new(tr!("menu.paste"), move |_, cx| {
                        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
                        else {
                            return;
                        };
                        paste_terminal.update(cx, |terminal, cx| terminal.paste(text, cx));
                    })
                    .shortcut(crate::platform::primary_shortcut("⌘V", "Ctrl+Shift+V"))
                    .disabled(!can_paste),
                    MenuItem::Separator,
                    MenuItem::new(tr!("terminal_command.open"), move |window, cx| {
                        command_terminal
                            .update(cx, |terminal, cx| terminal.open_command_bar(window, cx));
                    })
                    .shortcut(crate::platform::primary_shortcut("⌘I", "Ctrl+Shift+I"))
                    .disabled(!has_session),
                    MenuItem::Separator,
                    MenuItem::new(tr!("menu.select_all"), move |_, cx| {
                        select_all_terminal.update(cx, |terminal, cx| terminal.select_all(cx));
                    })
                    .shortcut(crate::platform::primary_shortcut("⌘A", "Ctrl+Shift+A"))
                    .disabled(!has_session),
                ]
            },
        );

        let scrollbar = self.session.as_ref().map(|session| {
            scrollbar::vertical(
                &TerminalScrollbarTarget {
                    term: session.term.clone(),
                    dirty: session.dirty.clone(),
                    viewport_rows: session.grid_size.1,
                    cell_height,
                },
                &self.scrollbar_state,
            )
        });

        let command_bar = self.command_bar_element(window, cx);

        // Only an embedded terminal keeps the header strip. Its trailing
        // inset clears the kill button the parent overlays at the toolbar's
        // right edge.
        let toolbar = self.embedded.then(|| {
            let title = if self.title().trim().is_empty() {
                tr!("right_panel.terminal")
            } else {
                self.title().to_owned()
            };
            let directory = self
                .working_directory
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
                .unwrap_or_else(|| tr!("workspace.workspace"));
            div()
                .h(px(TERMINAL_TOOLBAR_HEIGHT))
                .flex_none()
                .pl(px(10.0))
                .pr(px(34.0))
                .flex()
                .items_center()
                .gap(px(7.0))
                .border_b(hairline())
                .border_color(theme.separator)
                .bg(theme.surface)
                .child(
                    div()
                        .w(px(6.0))
                        .h(px(6.0))
                        .rounded_full()
                        .bg(if self.exited {
                            theme.danger
                        } else {
                            theme.accent
                        }),
                )
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .truncate()
                        .text_size(sp(12.5))
                        .text_color(theme.text_secondary)
                        .child(SharedString::from(title)),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(sp(12.5))
                        .text_color(theme.text_tertiary)
                        .child(directory),
                )
        });

        let grid = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .px(px(TERMINAL_PADDING_X))
            .py(px(TERMINAL_PADDING_Y))
            .bg(theme.terminal)
            .overflow_hidden()
            .flex()
            .flex_col()
            .relative()
            .child(screen)
            .children(scrollbar)
            .children(command_bar);

        div()
            .id("alacritty-terminal")
            .key_context("Terminal")
            .track_focus(&self.focus_handle)
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(theme.terminal)
            .children(toolbar)
            .child(grid)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel))
    }
}

fn terminal_cursor_style(focused: bool, blink_visible: bool) -> TerminalCursorStyle {
    if !focused {
        TerminalCursorStyle::Outline
    } else if blink_visible {
        TerminalCursorStyle::Solid
    } else {
        TerminalCursorStyle::Hidden
    }
}

fn terminal_grid_point(
    bounds: Bounds<Pixels>,
    position: Point<Pixels>,
    cell_width: f32,
    cell_height: f32,
    columns: usize,
    rows: usize,
    display_offset: usize,
    clamp_to_grid: bool,
) -> Option<(TerminalPoint, Side)> {
    if columns == 0 || rows == 0 || (!clamp_to_grid && !bounds.contains(&position)) {
        return None;
    }

    let x = f32::from(position.x - bounds.origin.x);
    let y = f32::from(position.y - bounds.origin.y);
    let max_x = columns as f32 * cell_width;
    let max_y = rows as f32 * cell_height;
    let x = x.clamp(0.0, max_x);
    let y = y.clamp(0.0, max_y);
    let column = ((x / cell_width).floor() as usize).min(columns - 1);
    let viewport_row = ((y / cell_height).floor() as usize).min(rows - 1) as i32;
    let side = if x >= max_x || x % cell_width >= cell_width / 2.0 {
        Side::Right
    } else {
        Side::Left
    };
    let line = Line(viewport_row - display_offset.min(i32::MAX as usize) as i32);

    Some((TerminalPoint::new(line, Column(column)), side))
}

/// Apply the same delimiter heuristics as Alacritty's hint system.
fn post_process_terminal_link<T: EventListener>(
    term: &Term<T>,
    regex_match: &Match,
) -> Option<Match> {
    let mut iter = term.grid().iter_from(*regex_match.start());
    let mut character = iter.cell().c;
    let end = *regex_match.end();
    let mut open_parens = 0;
    let mut open_brackets = 0;

    loop {
        match character {
            '(' => open_parens += 1,
            '[' => open_brackets += 1,
            ')' if open_parens == 0 => {
                iter.prev();
                break;
            }
            ')' => open_parens -= 1,
            ']' if open_brackets == 0 => {
                iter.prev();
                break;
            }
            ']' => open_brackets -= 1,
            _ => {}
        }

        if iter.point() == end {
            break;
        }
        let Some(indexed) = iter.next() else {
            break;
        };
        character = indexed.cell.c;
    }

    let start = *regex_match.start();
    while iter.point() != start {
        if !matches!(
            character,
            '.' | ',' | ':' | ';' | '?' | '!' | '(' | '[' | '\''
        ) {
            break;
        }
        let Some(indexed) = iter.prev() else {
            break;
        };
        character = indexed.cell.c;
    }

    (start <= iter.point()).then(|| start..=iter.point())
}

fn plain_link_at<T: EventListener>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    point: TerminalPoint,
) -> Option<(String, Match)> {
    let mut start = term.line_search_left(point);
    let mut end = term.line_search_right(point);
    start.line = start.line.max(point.line - MAX_TERMINAL_LINK_SEARCH_LINES);
    end.line = end.line.min(point.line + MAX_TERMINAL_LINK_SEARCH_LINES);

    let raw_match = RegexIter::new(start, end, Direction::Right, term, regex)
        .find(|bounds| bounds.contains(&point))?;
    let raw_end = *raw_match.end();
    let mut next_match = Some(raw_match);

    // Post-processing can split a greedy match at an unmatched closing
    // bracket. Continue inside the original range in case the clicked URL is
    // a later segment of that match.
    while let Some(regex_match) = next_match {
        let processed = post_process_terminal_link(term, &regex_match);
        if processed
            .as_ref()
            .is_some_and(|bounds| bounds.contains(&point))
        {
            let bounds = processed.unwrap();
            let value = term.bounds_to_string(*bounds.start(), *bounds.end());
            return Some((value, bounds));
        }

        let next_start = processed
            .as_ref()
            .map_or_else(|| *regex_match.start(), |bounds| *bounds.end())
            .add(term, Boundary::Grid, 1);
        if next_start > raw_end {
            return None;
        }
        next_match = term.regex_search_right(regex, next_start, raw_end);
    }

    None
}

fn hyperlink_at<T: EventListener>(term: &Term<T>, point: TerminalPoint) -> Option<(String, Match)> {
    let hyperlink = term.grid()[point].hyperlink()?;
    let grid = term.grid();

    let mut end = point;
    for cell in grid.iter_from(point) {
        if cell.hyperlink().as_ref() == Some(&hyperlink) {
            end = cell.point;
        } else {
            break;
        }
    }

    let mut start = point;
    let mut iter = grid.iter_from(point);
    while let Some(cell) = iter.prev() {
        if cell.hyperlink().as_ref() == Some(&hyperlink) {
            start = cell.point;
        } else {
            break;
        }
    }

    Some((hyperlink.uri().to_owned(), start..=end))
}

fn terminal_link_target(value: &str, working_directory: &Path) -> Option<TerminalLinkTarget> {
    if let Some(path) = existing_terminal_file_path(value, working_directory) {
        return Some(TerminalLinkTarget::File(path));
    }

    let url = url::Url::parse(value).ok()?;
    if url.scheme() == "file" {
        return None;
    }
    Some(TerminalLinkTarget::Url(url.to_string()))
}

fn existing_terminal_file_path(value: &str, working_directory: &Path) -> Option<PathBuf> {
    let mut path = match url::Url::parse(value) {
        Ok(url) if url.scheme() == "file" => url.to_file_path().ok()?,
        Ok(_) => return None,
        Err(_) => {
            if let Some(relative) = value.strip_prefix("~/") {
                dirs::home_dir()?.join(relative)
            } else {
                let path = Path::new(value);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    working_directory.join(path)
                }
            }
        }
    };

    loop {
        if path.exists() {
            return Some(path);
        }
        let value = path.to_string_lossy();
        let (prefix, suffix) = value.rsplit_once(':')?;
        if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        path = PathBuf::from(prefix);
    }
}

/// The encoding a mouse-aware program picked: ?1006h SGR, ?1005h UTF-8, or
/// the default X10 form.
enum MouseFormat {
    Sgr,
    Normal(bool),
}

impl MouseFormat {
    fn from_mode(mode: TermMode) -> Self {
        if mode.contains(TermMode::SGR_MOUSE) {
            MouseFormat::Sgr
        } else if mode.contains(TermMode::UTF8_MOUSE) {
            MouseFormat::Normal(true)
        } else {
            MouseFormat::Normal(false)
        }
    }
}

enum MouseButtonCode {
    LeftButton = 0,
    MiddleButton = 1,
    RightButton = 2,
    LeftMove = 32,
    MiddleMove = 33,
    RightMove = 34,
    NoneMove = 35,
    ScrollUp = 64,
    ScrollDown = 65,
    Other = 99,
}

impl MouseButtonCode {
    fn from_move_button(button: Option<MouseButton>) -> Self {
        match button {
            Some(MouseButton::Left) => MouseButtonCode::LeftMove,
            Some(MouseButton::Middle) => MouseButtonCode::MiddleMove,
            Some(MouseButton::Right) => MouseButtonCode::RightMove,
            Some(MouseButton::Navigate(_)) => MouseButtonCode::Other,
            None => MouseButtonCode::NoneMove,
        }
    }

    fn from_button(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => MouseButtonCode::LeftButton,
            MouseButton::Middle => MouseButtonCode::MiddleButton,
            MouseButton::Right => MouseButtonCode::RightButton,
            MouseButton::Navigate(_) => MouseButtonCode::Other,
        }
    }

    fn from_scroll(event: &ScrollWheelEvent) -> Self {
        let is_positive = match event.delta {
            ScrollDelta::Pixels(pixels) => pixels.y > px(0.),
            ScrollDelta::Lines(lines) => lines.y > 0.,
        };

        if is_positive {
            MouseButtonCode::ScrollUp
        } else {
            MouseButtonCode::ScrollDown
        }
    }

    fn is_other(&self) -> bool {
        matches!(self, MouseButtonCode::Other)
    }
}

/// Each scrolled line is one scroll-button press at the cursor's cell —
/// `None` when the program isn't reporting or the cell is off screen.
fn scroll_report(
    point: TerminalPoint,
    scroll_lines: i32,
    event: &ScrollWheelEvent,
    mode: TermMode,
) -> Option<impl Iterator<Item = Vec<u8>>> {
    if mode.intersects(TermMode::MOUSE_MODE) {
        mouse_report(
            point,
            MouseButtonCode::from_scroll(event),
            true,
            event.modifiers,
            MouseFormat::from_mode(mode),
        )
        .map(|report| std::iter::repeat(report).take(scroll_lines.unsigned_abs() as usize))
    } else {
        None
    }
}

fn mouse_button_report(
    point: TerminalPoint,
    button: MouseButton,
    modifiers: Modifiers,
    pressed: bool,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let button = MouseButtonCode::from_button(button);
    if !button.is_other() && mode.intersects(TermMode::MOUSE_MODE) {
        mouse_report(
            point,
            button,
            pressed,
            modifiers,
            MouseFormat::from_mode(mode),
        )
    } else {
        None
    }
}

fn mouse_moved_report(
    point: TerminalPoint,
    button: Option<MouseButton>,
    modifiers: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let button = MouseButtonCode::from_move_button(button);

    if !button.is_other() && mode.intersects(TermMode::MOUSE_MOTION | TermMode::MOUSE_DRAG) {
        // Only drags are reported in drag mode, so block NoneMove.
        if mode.contains(TermMode::MOUSE_DRAG) && matches!(button, MouseButtonCode::NoneMove) {
            None
        } else {
            mouse_report(point, button, true, modifiers, MouseFormat::from_mode(mode))
        }
    } else {
        None
    }
}

/// The bytes to send to the PTY for one mouse event at a cell — `None` for
/// points in the scrollback, which a mouse-aware program cannot see.
fn mouse_report(
    point: TerminalPoint,
    button: MouseButtonCode,
    pressed: bool,
    modifiers: Modifiers,
    format: MouseFormat,
) -> Option<Vec<u8>> {
    if point.line < 0 {
        return None;
    }

    let mut mods = 0;
    if modifiers.shift {
        mods += 4;
    }
    if modifiers.alt {
        mods += 8;
    }
    if modifiers.control {
        mods += 16;
    }

    match format {
        MouseFormat::Sgr => {
            Some(sgr_mouse_report(point, button as u8 + mods, pressed).into_bytes())
        }
        MouseFormat::Normal(utf8) => {
            if pressed {
                normal_mouse_report(point, button as u8 + mods, utf8)
            } else {
                normal_mouse_report(point, 3 + mods, utf8)
            }
        }
    }
}

fn normal_mouse_report(point: TerminalPoint, button: u8, utf8: bool) -> Option<Vec<u8>> {
    let max_point = if utf8 { 2015 } else { 223 };

    if point.line >= max_point || point.column >= max_point as usize {
        return None;
    }

    let mut msg = vec![b'\x1b', b'[', b'M', 32 + button];

    let mouse_pos_encode = |pos: usize| -> Vec<u8> {
        let pos = 32 + 1 + pos;
        let first = 0xC0 + pos / 64;
        let second = 0x80 + (pos & 63);
        vec![first as u8, second as u8]
    };

    if utf8 && point.column >= 95 {
        msg.append(&mut mouse_pos_encode(point.column.0));
    } else {
        msg.push(32 + 1 + point.column.0 as u8);
    }

    if utf8 && point.line >= 95 {
        msg.append(&mut mouse_pos_encode(point.line.0 as usize));
    } else {
        msg.push(32 + 1 + point.line.0 as u8);
    }

    Some(msg)
}

fn sgr_mouse_report(point: TerminalPoint, button: u8, pressed: bool) -> String {
    let c = if pressed { 'M' } else { 'm' };
    format!(
        "\x1b[<{};{};{}{}",
        button,
        point.column.0 + 1,
        point.line.0 + 1,
        c
    )
}

/// The bottom `count` non-blank rows of the live screen, oldest first.
/// Grid lines index the visible screen as `0..screen_lines` regardless of
/// scrollback or display offset.
fn tail_lines<T: EventListener>(term: &Term<T>, count: usize) -> Vec<String> {
    let grid = term.grid();
    let mut lines = Vec::with_capacity(count);
    for index in (0..grid.screen_lines() as i32).rev() {
        let row = &grid[Line(index)];
        let mut text = String::with_capacity(row.len());
        for cell in row {
            if cell.flags.intersects(
                Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER | Flags::HIDDEN,
            ) {
                text.push(' ');
                continue;
            }
            text.push(cell.c);
            if let Some(zerowidth) = cell.zerowidth() {
                text.extend(zerowidth);
            }
        }
        let text = text.trim_end();
        if text.is_empty() {
            continue;
        }
        lines.push(text.to_owned());
        if lines.len() == count {
            break;
        }
    }
    lines.reverse();
    lines
}

/// Scans the grid lines added since `watermark` was last advanced for a
/// localhost URL and returns the best candidate not in `reported`.
/// Alternate-screen apps (editors, TUIs) print file content, not server
/// announcements; scanning them only invites false positives.
fn scan_grid_localhost_url<T: EventListener>(
    term: &Term<T>,
    watermark: &mut usize,
    reported: &HashSet<String>,
) -> Option<String> {
    if term.mode().contains(TermMode::ALT_SCREEN) {
        return None;
    }
    let grid = term.grid();
    let total = grid.history_size() + grid.screen_lines();
    let unseen = total.saturating_sub(*watermark);
    *watermark = total;
    // At the scrollback cap `total` stops growing while fresh lines still
    // rotate through the bottom, so the visible screen is always rescanned.
    let scan_lines = (unseen + LOCALHOST_SCAN_OVERLAP)
        .max(grid.screen_lines())
        .min(total)
        .min(LOCALHOST_SCAN_MAX_LINES);
    if scan_lines == 0 {
        return None;
    }
    let bottom = term.bottommost_line();
    let top = Line(bottom.0 - scan_lines as i32 + 1);
    let text = term.bounds_to_string(
        TerminalPoint::new(top, Column(0)),
        TerminalPoint::new(bottom, term.last_column()),
    );
    localhost_urls(&text)
        .into_iter()
        .filter(|url| !reported.contains(url))
        .max_by_key(|url| localhost_url_rank(url))
}

/// Normalized localhost URLs found in a chunk of terminal output, in the
/// order they appear and with duplicates removed.
fn localhost_urls(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    LOCALHOST_URL_REGEX
        .captures_iter(text)
        .filter_map(|captures| captures.name("url"))
        .filter_map(|capture| normalize_localhost_url(capture.as_str()))
        .filter(|url| seen.insert(url.clone()))
        .collect()
}

/// Trims punctuation output tends to append right after a URL, gives
/// scheme-less `host:port` matches an `http://` scheme, and rewrites the
/// listen-everywhere `0.0.0.0` to the browsable `localhost`.
fn normalize_localhost_url(raw: &str) -> Option<String> {
    let mut raw = raw.trim_end_matches(['.', ',', ';', ':', '!', '?']);
    for (closer, opener) in [(')', '('), (']', '['), ('}', '{')] {
        while raw.ends_with(closer) && raw.matches(closer).count() > raw.matches(opener).count() {
            raw = &raw[..raw.len() - 1];
        }
    }
    // A closer can expose fresh trailing punctuation: "localhost:3000.)".
    raw = raw.trim_end_matches(['.', ',', ';', ':', '!', '?']);
    let with_scheme = if raw.contains("://") {
        raw.to_owned()
    } else {
        format!("http://{raw}")
    };
    let mut url = url::Url::parse(&with_scheme).ok()?;
    match url.host_str()? {
        "localhost" | "127.0.0.1" | "::1" => {}
        "0.0.0.0" => url.set_host(Some("localhost")).ok()?,
        host if host.ends_with(".localhost") => {}
        _ => return None,
    }
    Some(url.into())
}

/// Portless-style `https://<id>.localhost` URLs name the app rather than a
/// bare port and carry TLS — preferred over any other loopback form.
pub(crate) fn localhost_url_rank(url: &str) -> u8 {
    let host = url
        .strip_prefix("https://")
        .map(|rest| rest.split(['/', ':']).next().unwrap_or_default());
    match host {
        Some(host) if host.ends_with(".localhost") => 2,
        _ => 1,
    }
}

/// ⌘⇧K: drop the scrollback and every row above the cursor, moving the line
/// being edited to the top of the screen — Terminal.app's "Clear Scrollback"
/// and Zed's `terminal::Clear` behavior. The alt screen has no scrollback and
/// its rows belong to the running app, so there only the history is dropped.
fn clear_scrollback<T: EventListener>(term: &mut Term<T>) {
    term.selection = None;
    term.grid_mut().clear_history();
    if term.mode().contains(TermMode::ALT_SCREEN) {
        return;
    }

    let cursor = term.grid().cursor.point;
    term.grid_mut().reset_region(..cursor.line);
    let line = term.grid()[cursor.line][..Column(term.grid().columns())]
        .iter()
        .cloned()
        .enumerate()
        .collect::<Vec<_>>();
    for (index, cell) in line {
        term.grid_mut()[Line(0)][Column(index)] = cell;
    }
    term.grid_mut().cursor.point = TerminalPoint::new(Line(0), cursor.column);
    if term.grid().screen_lines() > 1 {
        term.grid_mut().reset_region(Line(1)..);
    }
}

fn bracketed_paste(text: String, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

fn terminal_key_bytes(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let modifiers = keystroke.modifiers;
    let key = keystroke.key.as_str();

    if modifiers.platform {
        #[cfg(target_os = "macos")]
        return match key {
            "left" => Some(vec![0x01]),
            "right" => Some(vec![0x05]),
            "backspace" => Some(vec![0x15]),
            _ => None,
        };

        #[cfg(not(target_os = "macos"))]
        return None;
    }

    let modifier = 1
        + u8::from(modifiers.shift)
        + u8::from(modifiers.alt) * 2
        + u8::from(modifiers.control) * 4;
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    // Terminal.app and Alacritty send the readline-friendly ESC b/f for
    // ⌥←/⌥→ so word movement works without inputrc entries; every other
    // modifier combination and platform keeps the xterm CSI encoding.
    let option_word_arrow = cfg!(target_os = "macos")
        && modifiers.alt
        && !modifiers.shift
        && !modifiers.control
        && !modifiers.function;
    let meta = |text: &str| {
        if modifiers.alt {
            format!("\x1b{text}")
        } else {
            text.to_owned()
        }
    };
    let special = match key {
        "enter" | "return" => Some(meta("\r")),
        "tab" if modifiers.shift => Some("\x1b[Z".to_owned()),
        "tab" => Some(meta("\t")),
        "backspace" => Some(meta("\x7f")),
        "escape" => Some(meta("\x1b")),
        "up" => Some(cursor_sequence('A', modifier, app_cursor)),
        "down" => Some(cursor_sequence('B', modifier, app_cursor)),
        "right" if option_word_arrow => Some("\x1bf".to_owned()),
        "right" => Some(cursor_sequence('C', modifier, app_cursor)),
        "left" if option_word_arrow => Some("\x1bb".to_owned()),
        "left" => Some(cursor_sequence('D', modifier, app_cursor)),
        "home" => Some(csi_sequence('H', modifier)),
        "end" => Some(csi_sequence('F', modifier)),
        "insert" => Some(tilde_sequence(2, modifier)),
        "delete" | "forwarddelete" => Some(tilde_sequence(3, modifier)),
        "pageup" => Some(tilde_sequence(5, modifier)),
        "pagedown" => Some(tilde_sequence(6, modifier)),
        "f1" => Some(function_sequence('P', modifier)),
        "f2" => Some(function_sequence('Q', modifier)),
        "f3" => Some(function_sequence('R', modifier)),
        "f4" => Some(function_sequence('S', modifier)),
        "f5" => Some(tilde_sequence(15, modifier)),
        "f6" => Some(tilde_sequence(17, modifier)),
        "f7" => Some(tilde_sequence(18, modifier)),
        "f8" => Some(tilde_sequence(19, modifier)),
        "f9" => Some(tilde_sequence(20, modifier)),
        "f10" => Some(tilde_sequence(21, modifier)),
        "f11" => Some(tilde_sequence(23, modifier)),
        "f12" => Some(tilde_sequence(24, modifier)),
        _ => None,
    };
    if let Some(special) = special {
        return Some(special.into_bytes());
    }

    // ⌥ is Meta: send ESC plus the bare key. On macOS key_char is the
    // Option-composed glyph (∫ for ⌥B), which no shell binds — so with
    // alt held take the unmodified key, keeping key_char only for named
    // keys like space.
    let text = if modifiers.alt {
        (key.chars().count() == 1)
            .then_some(key)
            .or(keystroke.key_char.as_deref())
    } else {
        keystroke
            .key_char
            .as_deref()
            .or_else(|| (key.chars().count() == 1).then_some(key))
    }?;
    let mut bytes = if modifiers.control {
        control_bytes(text)?
    } else if modifiers.alt && modifiers.shift {
        text.to_ascii_uppercase().into_bytes()
    } else {
        text.as_bytes().to_vec()
    };
    if modifiers.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn control_bytes(text: &str) -> Option<Vec<u8>> {
    let character = text.chars().next()?.to_ascii_lowercase();
    let byte = match character {
        ' ' | '@' => 0,
        'a'..='z' => character as u8 - b'a' + 1,
        '[' => 27,
        '\\' => 28,
        ']' => 29,
        '^' => 30,
        '_' => 31,
        '?' => 127,
        _ => return None,
    };
    Some(vec![byte])
}

fn cursor_sequence(final_byte: char, modifier: u8, app_cursor: bool) -> String {
    if modifier == 1 {
        format!("\x1b{}{}", if app_cursor { 'O' } else { '[' }, final_byte)
    } else {
        format!("\x1b[1;{modifier}{final_byte}")
    }
}

fn csi_sequence(final_byte: char, modifier: u8) -> String {
    if modifier == 1 {
        format!("\x1b[{final_byte}")
    } else {
        format!("\x1b[1;{modifier}{final_byte}")
    }
}

fn function_sequence(final_byte: char, modifier: u8) -> String {
    if modifier == 1 {
        format!("\x1bO{final_byte}")
    } else {
        format!("\x1b[1;{modifier}{final_byte}")
    }
}

fn tilde_sequence(number: u8, modifier: u8) -> String {
    if modifier == 1 {
        format!("\x1b[{number}~")
    } else {
        format!("\x1b[{number};{modifier}~")
    }
}

fn resolve_color(
    color: Color,
    colors: &alacritty_terminal::term::color::Colors,
    theme: Theme,
    foreground: bool,
) -> Hsla {
    let rgb = match color {
        Color::Spec(color) => Some(color),
        Color::Indexed(index) => {
            colors[index as usize].or_else(|| Some(terminal_rgb(index as usize, theme)))
        }
        Color::Named(NamedColor::Foreground | NamedColor::BrightForeground) => None,
        Color::Named(NamedColor::Background) => return theme.terminal,
        Color::Named(NamedColor::Cursor) => return theme.text,
        Color::Named(named) => {
            colors[named as usize].or_else(|| Some(terminal_rgb(named as usize, theme)))
        }
    };
    rgb.map(rgb_to_hsla).unwrap_or(if foreground {
        theme.text
    } else {
        theme.terminal
    })
}

fn rgb_to_hsla(color: Rgb) -> Hsla {
    rgb((u32::from(color.r) << 16) | (u32::from(color.g) << 8) | u32::from(color.b)).into()
}

fn terminal_rgb(index: usize, theme: Theme) -> Rgb {
    // The active palette's own 16-color table; the 6×6×6 cube and grayscale
    // ramp above index 15 are standard.
    let is_dark = theme.is_dark;
    let ansi = &theme.ansi;
    let value = match index {
        0..=15 => ansi[index],
        16..=231 => {
            let index = index - 16;
            let channel = |value: usize| {
                if value == 0 {
                    0
                } else {
                    55 + value as u32 * 40
                }
            };
            let red = channel(index / 36);
            let green = channel((index / 6) % 6);
            let blue = channel(index % 6);
            (red << 16) | (green << 8) | blue
        }
        232..=255 => {
            let value = 8 + (index as u32 - 232) * 10;
            (value << 16) | (value << 8) | value
        }
        value
            if value >= NamedColor::DimBlack as usize && value <= NamedColor::DimWhite as usize =>
        {
            let base = ansi[value - NamedColor::DimBlack as usize];
            // Faint fades toward the surface: black on dark, white on light.
            let dim = |channel: u32| {
                if is_dark {
                    channel * 2 / 3
                } else {
                    channel + (255 - channel) / 3
                }
            };
            (dim((base >> 16) & 0xff) << 16) | (dim((base >> 8) & 0xff) << 8) | dim(base & 0xff)
        }
        _ => {
            let rgba: gpui::Rgba = theme.text.into();
            return Rgb {
                r: (rgba.r.clamp(0.0, 1.0) * 255.0) as u8,
                g: (rgba.g.clamp(0.0, 1.0) * 255.0) as u8,
                b: (rgba.b.clamp(0.0, 1.0) * 255.0) as u8,
            };
        }
    };
    Rgb {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::vte::ansi::Processor;
    use gpui::{Modifiers, point, size};

    fn key(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            key: key.into(),
            key_char: key_char.map(str::to_owned),
            modifiers,
        }
    }

    fn parse_terminal(input: &[u8]) -> Term<VoidListener> {
        let dimensions = TerminalDimensions {
            columns: 40,
            rows: 3,
        };
        let mut term = Term::new(Config::default(), &dimensions, VoidListener);
        let mut processor: Processor = Processor::new();
        processor.advance(&mut term, input);
        term
    }

    fn terminal_point_for(content: &str, needle: &str) -> TerminalPoint {
        let offset = content.find(needle).unwrap();
        TerminalPoint::new(Line((offset / 40) as i32), Column(offset % 40))
    }

    #[test]
    fn tail_reports_bottom_non_blank_screen_lines() {
        // Three-row screen: the first line has already scrolled into
        // history, so only what is still on screen counts.
        let term = parse_terminal(b"one\r\ntwo\r\nthree\r\nfour");
        assert_eq!(tail_lines(&term, 3), vec!["two", "three", "four"]);
        assert_eq!(tail_lines(&term, 1), vec!["four"]);
    }

    #[test]
    fn tail_skips_blank_rows_and_trailing_whitespace() {
        let term = parse_terminal(b"alpha   \r\n\r\nomega");
        assert_eq!(tail_lines(&term, 3), vec!["alpha", "omega"]);
        assert!(tail_lines(&parse_terminal(b""), 3).is_empty());
    }

    fn plain_link_value(term: &Term<VoidListener>, point: TerminalPoint) -> Option<String> {
        let mut regex = RegexSearch::new(TERMINAL_LINK_REGEX).unwrap();
        plain_link_at(term, &mut regex, point).map(|(value, _)| value)
    }

    #[test]
    fn detects_plain_and_osc8_terminal_links() {
        let content = "visit (https://example.com/docs). next";
        let term = parse_terminal(content.as_bytes());
        assert_eq!(
            plain_link_value(&term, terminal_point_for(content, "example")),
            Some("https://example.com/docs".to_owned())
        );
        assert_eq!(
            plain_link_value(&term, terminal_point_for(content, ").")),
            None
        );

        let term = parse_terminal(b"x\x1b]8;;https://waku.gg\x1b\\Goddard\x1b]8;;\x1b\\ y");
        let (value, bounds) = hyperlink_at(&term, TerminalPoint::new(Line(0), Column(2))).unwrap();
        assert_eq!(value, "https://waku.gg");
        assert_eq!(
            bounds,
            TerminalPoint::new(Line(0), Column(1))..=TerminalPoint::new(Line(0), Column(7))
        );
    }

    #[test]
    fn detects_links_across_soft_wrapped_lines() {
        let content = "prefix https://example.com/a/very/long/path/that/wraps suffix";
        let term = parse_terminal(content.as_bytes());
        let mut regex = RegexSearch::new(TERMINAL_LINK_REGEX).unwrap();
        let (value, bounds) =
            plain_link_at(&term, &mut regex, terminal_point_for(content, "that")).unwrap();

        assert_eq!(value, "https://example.com/a/very/long/path/that/wraps");
        assert_eq!(*bounds.start(), terminal_point_for(content, "https"));
        assert_eq!(bounds.end().line, Line(1));
    }

    #[test]
    fn resolves_terminal_urls_and_existing_project_paths() {
        let working_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            terminal_link_target("https://example.com/docs", working_directory),
            Some(TerminalLinkTarget::Url(
                "https://example.com/docs".to_owned()
            ))
        );
        assert_eq!(
            terminal_link_target("src/terminal.rs:42:8", working_directory),
            Some(TerminalLinkTarget::File(
                working_directory.join("src/terminal.rs")
            ))
        );
        assert_eq!(
            terminal_link_target("src/does-not-exist.rs", working_directory),
            None
        );
    }

    #[test]
    fn encodes_terminal_control_and_cursor_keys() {
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "c",
                    Some("c"),
                    Modifiers {
                        control: true,
                        ..Default::default()
                    }
                ),
                TermMode::empty()
            ),
            Some(vec![3])
        );
        assert_eq!(
            terminal_key_bytes(&key("up", None, Modifiers::default()), TermMode::APP_CURSOR),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "left",
                    None,
                    Modifiers {
                        control: true,
                        ..Default::default()
                    }
                ),
                TermMode::empty()
            ),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(
                &key(
                    "backspace",
                    None,
                    Modifiers {
                        alt: true,
                        ..Default::default()
                    }
                ),
                TermMode::empty()
            ),
            Some(b"\x1b\x7f".to_vec())
        );

        let alt_b = key(
            "b",
            Some("∫"),
            Modifiers {
                alt: true,
                ..Default::default()
            },
        );
        assert_eq!(
            terminal_key_bytes(&alt_b, TermMode::empty()),
            Some(b"\x1bb".to_vec())
        );

        #[cfg(target_os = "macos")]
        {
            let alt_left = key(
                "left",
                None,
                Modifiers {
                    alt: true,
                    ..Default::default()
                },
            );
            assert_eq!(
                terminal_key_bytes(&alt_left, TermMode::empty()),
                Some(b"\x1bb".to_vec())
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_terminal_clipboard_shortcuts_preserve_ctrl_c_for_sigint() {
        let control = Modifiers {
            control: true,
            ..Default::default()
        };
        let control_shift = Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        };

        assert!(control.secondary());
        assert!(!terminal_clipboard_modifier_pressed(&control));
        assert!(terminal_clipboard_modifier_pressed(&control_shift));
    }

    #[test]
    fn clear_scrollback_keeps_only_the_cursor_line() {
        let mut term = parse_terminal(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nprompt$ ");
        assert_eq!(term.grid().history_size(), 3);
        assert_eq!(term.grid().cursor.point.line, Line(2));

        clear_scrollback(&mut term);

        assert_eq!(term.grid().history_size(), 0);
        assert_eq!(term.grid().cursor.point.line, Line(0));
        assert_eq!(term.grid().cursor.point.column, Column(8));
        let top = term.bounds_to_string(
            TerminalPoint::new(Line(0), Column(0)),
            TerminalPoint::new(Line(0), term.last_column()),
        );
        assert_eq!(top.trim_end(), "prompt$");
        let below = term.bounds_to_string(
            TerminalPoint::new(Line(1), Column(0)),
            TerminalPoint::new(Line(2), term.last_column()),
        );
        assert!(below.trim().is_empty());
    }

    #[test]
    fn clear_scrollback_leaves_the_alt_screen_alone() {
        let mut term = parse_terminal(b"main\r\n\x1b[?1049halt screen");
        assert!(term.mode().contains(TermMode::ALT_SCREEN));
        clear_scrollback(&mut term);
        let active = term.bounds_to_string(
            TerminalPoint::new(Line(1), Column(0)),
            TerminalPoint::new(Line(1), term.last_column()),
        );
        assert_eq!(active.trim_end(), "alt screen");
    }

    #[test]
    fn wraps_bracketed_paste_only_when_requested() {
        assert_eq!(
            bracketed_paste("hello".into(), TermMode::BRACKETED_PASTE),
            b"\x1b[200~hello\x1b[201~"
        );
        assert_eq!(bracketed_paste("hello".into(), TermMode::empty()), b"hello");
    }

    #[test]
    fn cursor_blinks_while_focused_and_outlines_when_unfocused() {
        assert_eq!(
            terminal_cursor_style(false, false),
            TerminalCursorStyle::Outline
        );
        assert_eq!(
            terminal_cursor_style(false, true),
            TerminalCursorStyle::Outline
        );
        assert_eq!(
            terminal_cursor_style(true, false),
            TerminalCursorStyle::Hidden
        );
        assert_eq!(
            terminal_cursor_style(true, true),
            TerminalCursorStyle::Solid
        );
    }

    #[test]
    fn maps_pointer_positions_into_scrollback_grid_coordinates() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(72.0), px(64.0)));
        let position = point(
            px(10.0 + TERMINAL_CELL_WIDTH * 2.0 + 5.0),
            px(20.0 + TERMINAL_CELL_HEIGHT + 8.0),
        );

        assert_eq!(
            terminal_grid_point(
                bounds,
                position,
                TERMINAL_CELL_WIDTH,
                TERMINAL_CELL_HEIGHT,
                10,
                4,
                3,
                false
            ),
            Some((TerminalPoint::new(Line(-2), Column(2)), Side::Right))
        );
        assert_eq!(
            terminal_grid_point(
                bounds,
                point(px(0.0), px(0.0)),
                TERMINAL_CELL_WIDTH,
                TERMINAL_CELL_HEIGHT,
                10,
                4,
                3,
                false
            ),
            None
        );
    }

    #[test]
    fn clamps_selection_drags_to_the_terminal_grid_edges() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(72.0), px(64.0)));

        assert_eq!(
            terminal_grid_point(
                bounds,
                point(px(500.0), px(500.0)),
                TERMINAL_CELL_WIDTH,
                TERMINAL_CELL_HEIGHT,
                10,
                4,
                0,
                true
            ),
            Some((TerminalPoint::new(Line(3), Column(9)), Side::Right))
        );
        assert_eq!(
            terminal_grid_point(
                bounds,
                point(px(-50.0), px(-50.0)),
                TERMINAL_CELL_WIDTH,
                TERMINAL_CELL_HEIGHT,
                10,
                4,
                3,
                true
            ),
            Some((TerminalPoint::new(Line(-3), Column(0)), Side::Left))
        );
    }

    #[test]
    fn detects_localhost_urls_in_terminal_output() {
        assert_eq!(
            localhost_urls("➜  Local:   http://localhost:5173/\n"),
            vec!["http://localhost:5173/".to_owned()]
        );
        assert_eq!(
            localhost_urls("listening on localhost:3000"),
            vec!["http://localhost:3000/".to_owned()]
        );
        assert_eq!(
            localhost_urls("Serving at http://127.0.0.1:8000/api"),
            vec!["http://127.0.0.1:8000/api".to_owned()]
        );
        assert_eq!(
            localhost_urls("ready on https://my-app.localhost"),
            vec!["https://my-app.localhost/".to_owned()]
        );
        // The listen-everywhere form opens as localhost.
        assert_eq!(
            localhost_urls("dev server on 0.0.0.0:8080"),
            vec!["http://localhost:8080/".to_owned()]
        );
    }

    #[test]
    fn localhost_url_detection_ignores_lookalikes_and_punctuation() {
        assert!(localhost_urls("host my-localhost:3000").is_empty());
        assert!(localhost_urls("notlocalhost:3000").is_empty());
        assert!(localhost_urls("https://example.com:443").is_empty());
        assert!(localhost_urls("localhost").is_empty());
        // A bare host:port needs digits; "localhost:" alone is not a server.
        assert!(localhost_urls("at localhost: soon").is_empty());
        assert_eq!(
            localhost_urls("(http://localhost:3000)."),
            vec!["http://localhost:3000/".to_owned()]
        );
        assert_eq!(
            localhost_urls("open http://localhost:3000, then"),
            vec!["http://localhost:3000/".to_owned()]
        );
    }

    #[test]
    fn prefers_portless_localhost_subdomains() {
        assert_eq!(localhost_url_rank("https://my-app.localhost/"), 2);
        assert_eq!(localhost_url_rank("https://localhost/"), 1);
        assert_eq!(localhost_url_rank("http://my-app.localhost/"), 1);
        assert_eq!(localhost_url_rank("http://localhost:3000/"), 1);

        let candidates =
            localhost_urls("target http://localhost:3000\nvia https://my-app.localhost\n");
        let best = candidates
            .into_iter()
            .max_by_key(|url| localhost_url_rank(url))
            .unwrap();
        assert_eq!(best, "https://my-app.localhost/");
    }

    #[test]
    fn scans_new_grid_lines_for_localhost_urls() {
        let mut term = parse_terminal(
            b"$ python3 -m http.server\nServing HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...\n",
        );
        let mut watermark = 0;
        let mut reported = HashSet::new();

        let url = scan_grid_localhost_url(&term, &mut watermark, &reported).unwrap();
        assert_eq!(url, "http://localhost:8000/");
        reported.insert(url);

        // No new output: the overlap rescan only re-finds reported URLs.
        assert_eq!(
            scan_grid_localhost_url(&term, &mut watermark, &reported),
            None
        );

        // A better URL printed later wins over the already-reported one.
        let mut processor: Processor = Processor::new();
        processor.advance(&mut term, b"also via https://my-app.localhost\n");
        assert_eq!(
            scan_grid_localhost_url(&term, &mut watermark, &reported),
            Some("https://my-app.localhost/".to_owned())
        );
    }

    #[test]
    fn detects_a_localhost_url_split_across_output_batches() {
        let mut term = parse_terminal(b"ready at http://local");
        let mut watermark = 0;
        let reported = HashSet::new();
        assert_eq!(
            scan_grid_localhost_url(&term, &mut watermark, &reported),
            None
        );

        let mut processor: Processor = Processor::new();
        processor.advance(&mut term, b"host:3000/\n");
        assert_eq!(
            scan_grid_localhost_url(&term, &mut watermark, &reported),
            Some("http://localhost:3000/".to_owned())
        );
    }

    fn scroll_event(lines: f32) -> ScrollWheelEvent {
        ScrollWheelEvent {
            delta: ScrollDelta::Lines(point(0., lines)),
            ..Default::default()
        }
    }

    #[test]
    fn sgr_mouse_reports_encode_button_cell_and_release() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let point = TerminalPoint::new(Line(2), Column(4));

        assert_eq!(
            mouse_button_report(point, MouseButton::Left, Modifiers::none(), true, mode),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            mouse_button_report(point, MouseButton::Left, Modifiers::none(), false, mode),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
        // Shift is +4, Alt is +8, Ctrl is +16; the platform key is not
        // encodable.
        assert_eq!(
            mouse_button_report(point, MouseButton::Right, Modifiers::command(), true, mode),
            Some(b"\x1b[<2;5;3M".to_vec())
        );
        assert_eq!(
            mouse_button_report(
                point,
                MouseButton::Left,
                Modifiers {
                    shift: true,
                    alt: true,
                    ..Default::default()
                },
                true,
                mode,
            ),
            Some(b"\x1b[<12;5;3M".to_vec())
        );
        // Scrollback cells report nothing — the program cannot see them.
        assert_eq!(
            mouse_button_report(
                TerminalPoint::new(Line(-1), Column(4)),
                MouseButton::Left,
                Modifiers::none(),
                true,
                mode,
            ),
            None
        );
    }

    #[test]
    fn scroll_report_repeats_one_press_per_line() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let point = TerminalPoint::new(Line(0), Column(0));

        let reports: Vec<Vec<u8>> = scroll_report(point, 3, &scroll_event(1.), mode)
            .expect("mouse mode should produce scroll reports")
            .collect();
        assert_eq!(reports, vec![b"\x1b[<64;1;1M".to_vec(); 3]);

        let reports: Vec<Vec<u8>> = scroll_report(point, -2, &scroll_event(-1.), mode)
            .expect("mouse mode should produce scroll reports")
            .collect();
        assert_eq!(reports, vec![b"\x1b[<65;1;1M".to_vec(); 2]);

        assert!(scroll_report(point, 3, &scroll_event(1.), TermMode::empty()).is_none());
    }

    #[test]
    fn mouse_motion_reports_follow_the_reporting_mode() {
        let point = TerminalPoint::new(Line(0), Column(0));
        let none = Modifiers::none();

        // Click reporting alone never reports motion.
        assert_eq!(
            mouse_moved_report(point, None, none, TermMode::MOUSE_REPORT_CLICK),
            None
        );
        // Any-motion mode reports plain moves and drags.
        assert_eq!(
            mouse_moved_report(
                point,
                None,
                none,
                TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE
            ),
            Some(b"\x1b[<35;1;1M".to_vec())
        );
        // Drag mode reports drags but swallows plain moves.
        assert_eq!(
            mouse_moved_report(
                point,
                Some(MouseButton::Left),
                none,
                TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE,
            ),
            Some(b"\x1b[<32;1;1M".to_vec())
        );
        assert_eq!(
            mouse_moved_report(
                point,
                None,
                none,
                TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE
            ),
            None
        );
    }

    #[test]
    fn mosaic_glyph_covers_block_elements_and_sextants() {
        assert!(matches!(
            mosaic_glyph('█'),
            Some(MosaicGlyph::Rect {
                x0: 0,
                y0: 0,
                x1: 8,
                y1: 8
            })
        ));
        assert!(matches!(
            mosaic_glyph('▄'),
            Some(MosaicGlyph::Rect { y0: 4, y1: 8, .. })
        ));
        assert!(matches!(
            mosaic_glyph('▌'),
            Some(MosaicGlyph::Rect { x1: 4, .. })
        ));
        assert!(matches!(
            mosaic_glyph('▐'),
            Some(MosaicGlyph::Rect { x0: 4, .. })
        ));
        // U+1FB00 BLOCK SEXTANT-1 fills only the top-left cell; U+1FB3B
        // BLOCK SEXTANT-23456 fills everything else.
        assert!(matches!(
            mosaic_glyph('\u{1FB00}'),
            Some(MosaicGlyph::Sextant(0b000001))
        ));
        assert!(matches!(
            mosaic_glyph('\u{1FB3B}'),
            Some(MosaicGlyph::Sextant(0b111110))
        ));
        // Box drawing stays on the text path — the fonts cover it.
        assert!(mosaic_glyph('a').is_none());
        assert!(mosaic_glyph('│').is_none());
        assert!(mosaic_glyph('─').is_none());
    }

    #[gpui::test]
    fn a_click_selects_the_row_under_the_pointer(cx: &mut gpui::TestAppContext) {
        let (view, cx) = cx.add_window_view(|_, cx| {
            TerminalView::with_launch(PathBuf::from("/tmp"), TerminalLaunch::Shell, cx)
        });
        cx.run_until_parked();

        let (bounds, cell_height, rows) = view.read_with(cx, |view, _| {
            let bounds = view
                .grid_bounds
                .get()
                .expect("grid bounds are recorded during prepaint");
            let session = view.session.as_ref().expect("session spawned");
            (bounds, session.cell_size.1, session.grid_size.1)
        });
        // Rows paint at the device-pixel-snapped pitch; the cell height used
        // for hit testing must match it or deep rows report the wrong line.
        let painted_pitch = view.update_in(cx, |_, window, cx| {
            f32::from(window.pixel_snap(px(terminal_cell_height(font_size(cx)))))
        });
        assert_eq!(cell_height, painted_pitch);

        for row in [0, 2, 5, rows - 2] {
            let position = point(
                bounds.origin.x + px(4.0),
                bounds.origin.y + px((row as f32 + 0.5) * painted_pitch),
            );
            cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::none());
            view.read_with(cx, |view, _| {
                let term = view.session.as_ref().unwrap().term.lock();
                let selection = term.selection.as_ref().expect("click starts a selection");
                assert!(
                    selection.intersects_range(Line(row as i32)..=Line(row as i32)),
                    "a click at the middle of painted row {row} must select row {row}"
                );
                assert!(
                    !selection.intersects_range(Line(row as i32 - 1)..=Line(row as i32 - 1)),
                    "a click at the middle of painted row {row} must not select the row above"
                );
            });
        }
    }
}
