use gpui::Window;

#[cfg(target_os = "macos")]
pub fn show_about_panel() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(main_thread).orderFrontStandardAboutPanel(None);
}

#[cfg(not(target_os = "macos"))]
pub fn show_about_panel() {}

/// Register embedded font data with CoreText at process scope. GPUI's
/// `add_fonts` only feeds its private font-kit source, which CoreText cascade
/// matching cannot see — and it refuses symbols-only faces outright (fonts
/// with no 'm' glyph). Fonts referenced through `FontFallbacks` therefore
/// must be registered here instead.
#[cfg(target_os = "macos")]
pub fn register_fonts_with_coretext(fonts: &[&'static [u8]]) -> anyhow::Result<()> {
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGDataProviderCreateWithData(
            info: *mut c_void,
            data: *const u8,
            size: usize,
            release_callback: *const c_void,
        ) -> *mut c_void;
        fn CGFontCreateWithDataProvider(provider: *mut c_void) -> *mut c_void;
        fn CGDataProviderRelease(provider: *mut c_void);
        fn CGFontRelease(font: *mut c_void);
    }
    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        fn CTFontManagerRegisterGraphicsFont(font: *mut c_void, error: *mut *mut c_void) -> bool;
    }

    for (index, data) in fonts.iter().enumerate() {
        unsafe {
            let provider = CGDataProviderCreateWithData(
                std::ptr::null_mut(),
                data.as_ptr(),
                data.len(),
                std::ptr::null(),
            );
            anyhow::ensure!(!provider.is_null(), "font {index}: not a readable buffer");
            let font = CGFontCreateWithDataProvider(provider);
            CGDataProviderRelease(provider);
            anyhow::ensure!(!font.is_null(), "font {index}: not a valid font");
            let registered = CTFontManagerRegisterGraphicsFont(font, std::ptr::null_mut());
            CGFontRelease(font);
            anyhow::ensure!(registered, "font {index}: CoreText registration failed");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn register_fonts_with_coretext(_: &[&'static [u8]]) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn init_reduce_motion(cx: &mut gpui::App) {
    use objc2_app_kit::NSWorkspace;

    cx.set_reduce_motion(NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion());
}

#[cfg(target_os = "linux")]
pub fn init_reduce_motion(cx: &mut gpui::App) {
    if let Ok(value) = std::env::var("GODDARD_REDUCE_MOTION")
        && let Some(enabled) = parse_boolean_setting(&value)
    {
        cx.set_reduce_motion(enabled);
        return;
    }

    // GNOME exposes its animation preference through GSettings. Resolve it
    // once off the UI thread; frames only read GPUI's in-memory flag.
    cx.spawn(async move |cx| {
        let enabled = cx
            .background_executor()
            .spawn(async move { linux_reduce_motion_enabled() })
            .await;
        cx.update(|cx| cx.set_reduce_motion(enabled));
    })
    .detach();
}

#[cfg(target_os = "linux")]
fn linux_reduce_motion_enabled() -> bool {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "enable-animations"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| parse_boolean_setting(&value))
        .is_some_and(|animations_enabled| !animations_enabled)
}

/// Ease of Access → "Show animations in Windows" clears
/// `SPI_GETCLIENTAREAANIMATION`. GPUI has no Windows implementation of its
/// own, and the call only reads a cached user setting, so startup can ask
/// directly.
#[cfg(target_os = "windows")]
pub fn init_reduce_motion(cx: &mut gpui::App) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SystemParametersInfoW,
    };

    let mut animations_enabled: i32 = 1;
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            std::ptr::from_mut(&mut animations_enabled).cast(),
            0,
        )
    };
    if read != 0 {
        cx.set_reduce_motion(animations_enabled == 0);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn init_reduce_motion(_: &mut gpui::App) {}

/// With "Reduce transparency" on, macOS drops all vibrancy — the Sidebar
/// material degrades to a flat tint that fakes a blur, so callers should
/// treat sidebar transparency as off and paint the solid fill instead.
#[cfg(target_os = "macos")]
pub fn reduce_transparency() -> bool {
    use objc2_app_kit::NSWorkspace;

    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency()
}

#[cfg(not(target_os = "macos"))]
pub fn reduce_transparency() -> bool {
    false
}

/// The OS "increase contrast" accessibility preference. Read when a theme is
/// built — palette construction is rare, so no caching is needed.
#[cfg(target_os = "macos")]
pub fn increase_contrast() -> bool {
    use objc2_app_kit::NSWorkspace;

    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast()
}

/// GNOME's High Contrast is a theme, not a flag — honor an explicit override
/// and leave other desktops to their own settings until one is wired up.
#[cfg(target_os = "linux")]
pub fn increase_contrast() -> bool {
    std::env::var("GODDARD_INCREASE_CONTRAST")
        .ok()
        .and_then(|value| parse_boolean_setting(&value))
        .unwrap_or(false)
}

/// SystemParametersInfo's high-contrast query reflects Ease of Access →
/// Contrast themes. The call only reads a cached user setting, so it is safe
/// to ask directly at theme-build time.
#[cfg(target_os = "windows")]
pub fn increase_contrast() -> bool {
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW};

    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        dwFlags: 0,
        lpszDefaultScheme: std::ptr::null_mut(),
    };
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            0,
            std::ptr::from_mut(&mut hc).cast(),
            0,
        )
    };
    read != 0 && hc.dwFlags & HCF_HIGHCONTRASTON != 0
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn increase_contrast() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn parse_boolean_setting(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Deliver an audible macOS notification. GPUI owns the notification-center
/// delegate (and therefore click responses); Goddard only supplies content here
/// because GPUI's generic payload does not currently expose a sound field.
#[cfg(target_os = "macos")]
pub fn show_task_notification(tag: &str, title: &str, body: &str, _: &gpui::App) {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNNotificationSound, UNUserNotificationCenter,
    };

    // UserNotifications raises an Objective-C exception for an executable
    // outside an application bundle, including unit tests and `cargo run`.
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }

    let tag = tag.to_owned();
    let title = title.to_owned();
    let body = body.to_owned();
    let authorization = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            return;
        }

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));

        // A nil trigger delivers immediately. The stable task tag replaces an
        // older completion banner for the same task and comes back on click.
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&tag),
            &content,
            None,
        );
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &authorization,
        );
}

#[cfg(not(target_os = "macos"))]
pub fn show_task_notification(tag: &str, title: &str, body: &str, cx: &gpui::App) {
    cx.show_system_notification(gpui::SystemNotification {
        tag: tag.to_owned().into(),
        title: title.to_owned().into(),
        body: body.to_owned().into(),
        actions: Vec::new(),
    });
}

#[cfg(target_os = "macos")]
thread_local! {
    /// `AVAudioPlayer` stops when deallocated, so the playing instance is
    /// retained until the next play replaces it. Playback outlives this only
    /// by the sound's own sub-second length.
    static PLAYING_COMPLETION_SOUND:
        std::cell::RefCell<Option<objc2::rc::Retained<objc2_avf_audio::AVAudioPlayer>>> =
        const { std::cell::RefCell::new(None) };
}

/// The bundled sounds' embedded MP3 payloads.
#[cfg(target_os = "macos")]
fn completion_sound_data(sound: waku_client::persistence::CompletionSound) -> &'static [u8] {
    use waku_client::persistence::CompletionSound;

    match sound {
        CompletionSound::Bleep => include_bytes!("../assets/sounds/bleep.mp3").as_slice(),
        CompletionSound::Gentle => include_bytes!("../assets/sounds/gentle.mp3").as_slice(),
        CompletionSound::Bubble => include_bytes!("../assets/sounds/bubble.mp3").as_slice(),
        CompletionSound::Chime => include_bytes!("../assets/sounds/chime.mp3").as_slice(),
        CompletionSound::Retro => include_bytes!("../assets/sounds/retro.mp3").as_slice(),
    }
}

/// Per-sound loudness compensation, multiplied with the user's volume so the
/// bundled set lands at a comparable level. Retro's recording runs hot.
#[cfg(target_os = "macos")]
fn completion_sound_gain(sound: waku_client::persistence::CompletionSound) -> f32 {
    match sound {
        waku_client::persistence::CompletionSound::Retro => 0.5,
        _ => 1.0,
    }
}

/// Play one of the bundled turn-completion sounds at `volume` relative to its
/// recorded level — 1.0 plays it as bundled and the slider allows up to 2.0.
/// `AVAudioPlayer` decodes the embedded MP3 itself and its volume is a linear
/// gain that can boost past 1.0, where `NSSound` clamps; `play` returns
/// immediately. There is no smaller portable API, so other platforms stay
/// silent for now.
#[cfg(target_os = "macos")]
pub fn play_completion_sound(sound: waku_client::persistence::CompletionSound, volume: f32) {
    use objc2::AnyThread;
    use objc2_avf_audio::AVAudioPlayer;
    use objc2_foundation::NSData;

    let volume = (volume * completion_sound_gain(sound))
        .clamp(0.0, waku_client::persistence::MAX_COMPLETION_SOUND_VOLUME);
    let data = NSData::with_bytes(completion_sound_data(sound));
    let Ok(player) = (unsafe { AVAudioPlayer::initWithData_error(AVAudioPlayer::alloc(), &data) })
    else {
        return;
    };
    unsafe {
        player.setVolume(volume);
        if player.play() {
            PLAYING_COMPLETION_SOUND.with_borrow_mut(|slot| *slot = Some(player));
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn play_completion_sound(_: waku_client::persistence::CompletionSound, _: f32) {}

#[cfg(target_os = "macos")]
fn app_icon_for_application_path(
    application_path: &objc2_foundation::NSString,
) -> Option<std::sync::Arc<gpui::Image>> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSWorkspace};
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize};

    let image = NSWorkspace::sharedWorkspace().iconForFile(application_path);
    image.setSize(NSSize::new(32.0, 32.0));
    // Extract one small representation. `TIFFRepresentation` would serialize
    // the icon's entire rep stack — ~72 MB and hundreds of milliseconds per
    // app for a 1024px icon — and then hand GPUI a 1024px PNG to decode on
    // first paint. Proposing a 32pt rect selects the nearest small rep.
    let mut proposed = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(32.0, 32.0));
    let cg_image =
        unsafe { image.CGImageForProposedRect_context_hints(&mut proposed, None, None) }?;
    let bitmap_rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &cg_image);
    let properties = NSDictionary::new();
    let png_data = unsafe {
        bitmap_rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
    }?;
    let bytes = unsafe { png_data.as_bytes_unchecked() };
    (!bytes.is_empty()).then(|| {
        std::sync::Arc::new(gpui::Image::from_bytes(
            gpui::ImageFormat::Png,
            bytes.to_vec(),
        ))
    })
}

#[cfg(target_os = "macos")]
pub fn load_app_icon_for_bundle_id(bundle_id: &str) -> Option<std::sync::Arc<gpui::Image>> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let bundle_id = NSString::from_str(bundle_id);
    let application_url =
        NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&bundle_id)?;
    let application_path = application_url.path()?;
    app_icon_for_application_path(&application_path)
}

#[cfg(not(target_os = "macos"))]
pub fn load_app_icon_for_bundle_id(_: &str) -> Option<std::sync::Arc<gpui::Image>> {
    None
}

/// A folder-capable application the header's "open project in" control can
/// target, resolved against what is installed on this machine.
#[derive(Clone)]
pub struct ExternalApp {
    /// Stable identifier persisted as the user's preferred target.
    pub id: &'static str,
    pub label: &'static str,
    /// The bundle id that resolved here, for launching.
    pub bundle_id: &'static str,
    pub icon: std::sync::Arc<gpui::Image>,
}

impl ExternalApp {
    /// Reads as an editor for a single file — the catalog also lists the
    /// file manager and terminals, which only meaningfully open folders.
    pub fn is_editor(&self) -> bool {
        matches!(
            self.id,
            "vscode" | "cursor" | "zed" | "devin" | "xcode" | "android-studio"
        )
    }

    /// The `<scheme>://file/<path>:<line>` deep link this app registers, when
    /// it has one. Apps without one get a plain document open instead.
    fn file_line_scheme(&self) -> Option<&'static str> {
        match self.id {
            "vscode" => Some("vscode"),
            "cursor" => Some("cursor"),
            "zed" => Some("zed"),
            "devin" => Some("windsurf"),
            _ => None,
        }
    }
}

/// The `<scheme>://file/<path>:<line>` URL an editor deep link expects. The
/// `url` crate percent-encodes the path; `:` survives the path encode set,
/// which is what the editors split the line on.
fn editor_file_line_url(scheme: &str, path: &std::path::Path, line: u32) -> Option<String> {
    let mut url = url::Url::parse(&format!("{scheme}://file/")).ok()?;
    url.set_path(&format!("{}:{line}", path.to_string_lossy()));
    Some(url.into())
}

/// Open the file `path` in `app`, landing on `line` when the app takes a
/// line deep link and opening the document plainly when it does not. Every
/// route hands off to the OS asynchronously, so this is safe from any click
/// path.
pub fn open_file_in_app(
    path: &std::path::Path,
    line: Option<u32>,
    app: &ExternalApp,
    cx: &gpui::App,
) {
    if let (Some(line), Some(scheme)) = (line, app.file_line_scheme()) {
        if let Some(url) = editor_file_line_url(scheme, path, line) {
            cx.open_url(&url);
            return;
        }
    }
    open_path_in_app(path, app.bundle_id);
}

/// Known folder-capable apps in menu order — editors, the file manager,
/// terminals, IDEs. An entry lists every bundle id it ships under; the first
/// installed one wins.
#[cfg(target_os = "macos")]
const TERMY_BUNDLE_ID: &str = "com.lassevestergaard.termy";

#[cfg(target_os = "macos")]
const OPEN_IN_CATALOG: &[(&str, &str, &[&str])] = &[
    ("vscode", "VS Code", &["com.microsoft.VSCode"]),
    ("cursor", "Cursor", &["com.todesktop.230313mzl4w4u92"]),
    ("zed", "Zed", &["dev.zed.Zed", "dev.zed.Zed-Preview"]),
    ("devin", "Devin", &["com.exafunction.windsurf"]),
    ("finder", "Finder", &["com.apple.finder"]),
    ("terminal", "Terminal", &["com.apple.Terminal"]),
    ("termy", "Termy", &[TERMY_BUNDLE_ID]),
    ("iterm2", "iTerm2", &["com.googlecode.iterm2"]),
    ("kitty", "Kitty", &["net.kovidgoyal.kitty"]),
    ("ghostty", "Ghostty", &["com.mitchellh.ghostty"]),
    ("warp", "Warp", &["dev.warp.Warp-Stable", "dev.warp.Warp"]),
    ("xcode", "Xcode", &["com.apple.dt.Xcode"]),
    (
        "android-studio",
        "Android Studio",
        &["com.google.android.studio"],
    ),
];

/// Resolve which catalog apps are installed, with their icons.
#[cfg(target_os = "macos")]
pub fn detect_open_in_apps() -> Vec<ExternalApp> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let workspace = NSWorkspace::sharedWorkspace();
    OPEN_IN_CATALOG
        .iter()
        .filter_map(|&(id, label, bundle_ids)| {
            bundle_ids.iter().find_map(|&bundle_id| {
                let application_url = workspace
                    .URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
                let application_path = application_url.path()?;
                Some(ExternalApp {
                    id,
                    label,
                    bundle_id,
                    icon: app_icon_for_application_path(&application_path)?,
                })
            })
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
pub fn detect_open_in_apps() -> Vec<ExternalApp> {
    Vec::new()
}

/// Open `path` in the application `bundle_id`, activating it. Launch Services
/// delivers the open asynchronously, so this never blocks.
#[cfg(target_os = "macos")]
pub fn open_path_in_app(path: &std::path::Path, bundle_id: &str) {
    use objc2_app_kit::{NSWorkspace, NSWorkspaceOpenConfiguration};
    use objc2_foundation::{NSArray, NSString, NSURL};

    let workspace = NSWorkspace::sharedWorkspace();
    let Some(application_url) =
        workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))
    else {
        return;
    };
    let url = if bundle_id == TERMY_BUNDLE_ID {
        // Termy rejects folder file URLs; its public new-tab route accepts the
        // working directory as an encoded query parameter instead.
        let Some(url) = NSURL::URLWithString(&NSString::from_str(&termy_open_url(path))) else {
            return;
        };
        url
    } else {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    };
    workspace.openURLs_withApplicationAtURL_configuration_completionHandler(
        &NSArray::from_retained_slice(&[url]),
        &application_url,
        &NSWorkspaceOpenConfiguration::configuration(),
        None,
    );
}

#[cfg(target_os = "macos")]
fn termy_open_url(path: &std::path::Path) -> String {
    let mut url = url::Url::parse("termy://new").expect("static Termy URL should be valid");
    url.query_pairs_mut()
        .append_pair("dir", &path.to_string_lossy());
    url.into()
}

#[cfg(not(target_os = "macos"))]
pub fn open_path_in_app(_: &std::path::Path, _: &str) {}

/// Select `path` in the platform file manager. GPUI dispatches Linux portal
/// and subprocess work away from the UI thread.
pub fn reveal_in_file_manager(path: &std::path::Path, cx: &gpui::App) {
    cx.reveal_path(path);
}

/// Open `path` with its default application — a document in its editor.
pub fn open_with_default_app(path: &std::path::Path, cx: &gpui::App) {
    cx.open_with_system(path);
}

/// Decode the embedded desktop icon once. X11 consumes the RGBA pixels from
/// `WindowOptions`; Wayland associates the window through `app_id` and its
/// installed desktop entry.
#[cfg(target_os = "linux")]
pub fn linux_app_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    static ICON: std::sync::LazyLock<Option<std::sync::Arc<image::RgbaImage>>> =
        std::sync::LazyLock::new(|| {
            image::load_from_memory(include_bytes!("../resources/linux/app-icon.png"))
                .ok()
                .map(|image| std::sync::Arc::new(image.into_rgba8()))
        });
    ICON.clone()
}

/// A compact shortcut label for the platform's primary GUI modifier.
pub const fn primary_shortcut<'a>(macos: &'a str, other: &'a str) -> &'a str {
    if cfg!(target_os = "macos") {
        macos
    } else {
        other
    }
}

/// Keep Goddard's single main window alive when the user closes it. This preserves
/// the current session and lets a Dock activation reveal the same GPUI window.
#[cfg(target_os = "macos")]
pub fn configure_main_window_close_behavior(window: &Window, cx: &gpui::App) {
    window.on_window_should_close(cx, |window, _| {
        hide_window(window);
        false
    });
}

#[cfg(not(target_os = "macos"))]
pub fn configure_main_window_close_behavior(_: &Window, _: &gpui::App) {}

#[cfg(target_os = "macos")]
pub fn hide_window(window: &mut Window) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(_main_thread) = MainThreadMarker::new() else {
        return;
    };

    // GPUI owns this view and its NSWindow. AppKit access stays on the main
    // thread, and orderOut hides without triggering GPUI's close callback.
    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        if let Some(native_window) = view.window() {
            native_window.orderOut(None);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn hide_window(window: &mut Window) {
    window.remove_window();
}

#[cfg(target_os = "macos")]
thread_local! {
    static SIDEBAR_GLASS_VIEW: std::cell::RefCell<
        Option<objc2::rc::Retained<objc2_app_kit::NSGlassEffectView>>,
    > = const { std::cell::RefCell::new(None) };
}

/// NSGlassEffectView ships with macOS 26; the class lookup is the availability
/// check, so older systems keep the vibrancy + tint path untouched.
#[cfg(target_os = "macos")]
fn glass_effect_supported() -> bool {
    objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some()
}

#[cfg(target_os = "macos")]
const SIDEBAR_WIDTH: f64 = 252.0;

pub fn start_window_move(window: &Window) {
    window.start_window_move();
}

/// Perform the platform's titlebar double-click action. GPUI delegates this
/// to the user's system preference on macOS, while Linux client decorations
/// must toggle maximize explicitly.
pub fn titlebar_double_click(window: &Window) {
    #[cfg(target_os = "macos")]
    window.titlebar_double_click();

    // Windows performs the user's configured caption double-click action in
    // `DefWindowProc`, which sees the click because the drag region reports
    // itself as caption to the hit test.
    #[cfg(target_os = "windows")]
    let _ = window;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    if window.window_controls().maximize && window.is_resizable() {
        window.zoom_window();
    }
}

/// Match Cursor's macOS glass window stack without asking GPUI's transparent
/// Metal target to blend two translucent quads. The semantic tint is painted
/// by GPUI as the sidebar's translucent fill — the slider drives its opacity —
/// while native material shows through the remainder: Sidebar vibrancy, or on
/// macOS 26 an `NSGlassEffectView` filling the strip with the vibrancy view
/// deactivated beneath it — glass lenses whatever is composited beneath it,
/// and leaving vibrancy on would have it refract already-blurred material,
/// which reads as plain blur. With `transparent` off the effect view stops
/// rendering and the glass view hides; GPUI's sidebar fill is opaque by then
/// and covers the strip itself.
#[cfg(target_os = "macos")]
pub fn configure_sidebar_material(
    window: &Window,
    sidebar: gpui::Hsla,
    dark: bool,
    transparent: bool,
) {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode,
        NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let transparent = transparent && !reduce_transparency();
    let glass_active = transparent && glass_effect_supported();
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };

    // GPUI owns the view hierarchy and creates the effect view before the
    // root entity is installed. We only adjust public AppKit properties.
    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        let Some(native_window) = view.window() else {
            return;
        };
        let sidebar_rgb: gpui::Rgba = sidebar.into();
        let (r, g, b) = (
            f64::from(sidebar_rgb.r),
            f64::from(sidebar_rgb.g),
            f64::from(sidebar_rgb.b),
        );
        let background = if transparent {
            if dark {
                NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, 0.25)
            } else {
                NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 1.0, 1.0, 0.0)
            }
        } else {
            // The window stays non-opaque, so a clear pixel would otherwise
            // show the desktop; an opaque backdrop keeps any uncovered gap
            // the same color GPUI paints the sidebar.
            NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
        };
        native_window.setBackgroundColor(Some(&background));

        let Some(content_view) = native_window.contentView() else {
            return;
        };

        let mut configured_effect = false;
        for subview in content_view.subviews().iter() {
            let Some(effect_view) = subview.downcast_ref::<NSVisualEffectView>() else {
                continue;
            };
            effect_view.setMaterial(NSVisualEffectMaterial::Sidebar);
            effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            // Glass samples the window backdrop beneath it; with vibrancy
            // active it would refract already-blurred material, so when the
            // glass surface owns the strip the vibrancy view turns off and
            // the glass lenses the desktop directly.
            effect_view.setState(if transparent && !glass_active {
                NSVisualEffectState::Active
            } else {
                NSVisualEffectState::Inactive
            });
            configured_effect = true;
        }
        if !configured_effect {
            return;
        }

        // macOS 26 fills the strip with a real liquid-glass surface.
        // Allocation itself is gated — the class is absent on older systems
        // and `class!` would panic.
        if glass_effect_supported() {
            // A light constant tint keeps the bare lens on-theme; the
            // slider's density lives in GPUI's translucent sidebar fill
            // above the Metal layer.
            let glass_tint = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 0.15);
            SIDEBAR_GLASS_VIEW.with_borrow_mut(|slot| {
                let needs_new_view = slot.as_ref().is_none_or(|glass_view| {
                    glass_view
                        .window()
                        .as_deref()
                        .is_none_or(|window| !std::ptr::eq(window, native_window.as_ref()))
                });
                if needs_new_view {
                    let mut frame = content_view.bounds();
                    frame.size.width = SIDEBAR_WIDTH;
                    let glass_view = objc2_app_kit::NSGlassEffectView::initWithFrame(
                        objc2_app_kit::NSGlassEffectView::alloc(main_thread),
                        frame,
                    );
                    glass_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
                    // The strip reaches the window edge; a capsule radius would
                    // round the wrong corners.
                    glass_view.setCornerRadius(0.0);
                    content_view.addSubview_positioned_relativeTo(
                        &glass_view,
                        NSWindowOrderingMode::Below,
                        Some(view),
                    );
                    *slot = Some(glass_view);
                }

                if let Some(glass_view) = slot.as_ref() {
                    glass_view.setHidden(!glass_active);
                    if glass_active {
                        glass_view.setTintColor(Some(&glass_tint));
                    }
                }
            });
        }

    }
}

#[cfg(not(target_os = "macos"))]
pub fn configure_sidebar_material(_: &Window, _: gpui::Hsla, _: bool, _: bool) {}

#[cfg(target_os = "macos")]
pub fn set_sidebar_material_width(window: &Window, width: f32) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(_main_thread) = MainThreadMarker::new() else {
        return;
    };

    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        let Some(native_window) = view.window() else {
            return;
        };
        SIDEBAR_GLASS_VIEW.with_borrow(|slot| {
            let Some(glass_view) = slot.as_ref().filter(|glass_view| {
                glass_view
                    .window()
                    .as_deref()
                    .is_some_and(|window| std::ptr::eq(window, native_window.as_ref()))
            }) else {
                return;
            };
            let mut frame = glass_view.frame();
            frame.size.width = width.into();
            glass_view.setFrame(frame);
        });
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_sidebar_material_width(_: &Window, _: f32) {}

/// The window-server id (`-[NSWindow windowNumber]`) used to snapshot this
/// window's own contents — Big Picture's blurred backdrop.
#[cfg(target_os = "macos")]
pub fn window_capture_id(window: &Window) -> Option<u32> {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        u32::try_from(view.window()?.windowNumber()).ok()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn window_capture_id(_: &Window) -> Option<u32> {
    None
}

/// A heavily blurred snapshot of the window's own contents, for painting
/// under an overlay's scrim. Pure CPU work — capture, channel repack,
/// downscale, gaussian — meant for a background executor; the frame only
/// paints the returned `RenderImage`.
#[cfg(target_os = "macos")]
pub fn blurred_window_snapshot(window_id: u32) -> Option<std::sync::Arc<gpui::RenderImage>> {
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapFormat, NSBitmapImageRep};
    use objc2_core_graphics::CGImage;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    unsafe extern "C" {
        fn CGWindowListCreateImage(
            bounds: NSRect,
            option: u32,
            window_id: u32,
            image_option: u32,
        ) -> *mut CGImage;
    }
    const OPTION_INCLUDING_WINDOW: u32 = 1 << 3;
    const IMAGE_IGNORE_FRAMING: u32 = 1;
    const IMAGE_NOMINAL_RESOLUTION: u32 = 1 << 4;

    // CGRectNull — the whole window rather than a rect of it.
    let image = unsafe {
        let raw = CGWindowListCreateImage(
            NSRect::new(
                NSPoint::new(f64::INFINITY, f64::INFINITY),
                NSSize::new(0.0, 0.0),
            ),
            OPTION_INCLUDING_WINDOW,
            window_id,
            IMAGE_IGNORE_FRAMING | IMAGE_NOMINAL_RESOLUTION,
        );
        objc2::rc::Retained::from_raw(raw)?
    };
    let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
    if rep.isPlanar() || rep.bitsPerSample() != 8 {
        return None;
    }
    let width = usize::try_from(rep.pixelsWide()).ok()?;
    let height = usize::try_from(rep.pixelsHigh()).ok()?;
    let bytes_per_row = usize::try_from(rep.bytesPerRow()).ok()?;
    let samples = usize::try_from(rep.samplesPerPixel()).ok()?;
    let format = rep.bitmapFormat();
    let data = rep.bitmapData();
    if data.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, bytes_per_row.checked_mul(height)?) };
    let bgra = crate::browser::bgra_from_bitmap(
        bytes,
        width,
        height,
        bytes_per_row,
        samples,
        format.contains(NSBitmapFormat::AlphaFirst),
        format.contains(NSBitmapFormat::ThirtyTwoBitLittleEndian),
    )?;
    let buffer = image::RgbaImage::from_raw(width as u32, height as u32, bgra)?;
    // A hard downscale plus a small-radius gaussian on the thumbnail reads as
    // a deep blur once the image stretches back across the window.
    let blurred = image::imageops::blur(
        &image::imageops::resize(
            &buffer,
            (width as u32 / 8).max(48),
            (height as u32 / 8).max(48),
            image::imageops::FilterType::Triangle,
        ),
        3.0,
    );
    Some(std::sync::Arc::new(gpui::RenderImage::new(vec![
        image::Frame::new(blurred),
    ])))
}

#[cfg(not(target_os = "macos"))]
pub fn blurred_window_snapshot(_: u32) -> Option<std::sync::Arc<gpui::RenderImage>> {
    None
}

/// Opt-in three-finger trackpad swipe for back/forward, recognized from the
/// window's touch stream by the platform layer.
#[cfg(target_os = "macos")]
pub fn set_trackpad_navigation_swipe_enabled(window: &Window, enabled: bool) {
    window.set_trackpad_navigation_swipe_enabled(enabled);
}

#[cfg(not(target_os = "macos"))]
pub fn set_trackpad_navigation_swipe_enabled(_: &Window, _: bool) {}

/// Follow macOS when `dark` is `None`, otherwise force the native titlebar,
/// traffic lights, menus, and vibrancy to the selected appearance.
#[cfg(target_os = "macos")]
pub fn set_window_appearance(window: &Window, dark: Option<bool>) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
        NSView,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let Some(_main_thread) = MainThreadMarker::new() else {
        return;
    };

    unsafe {
        let view = handle.ns_view.cast::<NSView>().as_ref();
        let Some(native_window) = view.window() else {
            return;
        };
        let appearance = dark.and_then(|dark| {
            NSAppearance::appearanceNamed(if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            })
        });
        native_window.setAppearance(appearance.as_deref());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_window_appearance(_: &Window, _: Option<bool>) {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::parse_boolean_setting;

    #[test]
    fn boolean_desktop_settings_are_parsed_case_insensitively() {
        assert_eq!(parse_boolean_setting(" true\n"), Some(true));
        assert_eq!(parse_boolean_setting("OFF"), Some(false));
        assert_eq!(parse_boolean_setting("default"), None);
    }

    #[test]
    fn embedded_linux_icon_decodes_at_desktop_size() {
        let icon = super::linux_app_icon().expect("embedded PNG should decode");

        assert_eq!(icon.dimensions(), (256, 256));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use std::{borrow::Cow, path::Path};

    use super::{completion_sound_data, termy_open_url};

    #[test]
    fn termy_projects_use_the_new_tab_deeplink() {
        let url = url::Url::parse(&termy_open_url(Path::new("/tmp/project +%")))
            .expect("Termy deeplink should be valid");

        assert_eq!(url.scheme(), "termy");
        assert_eq!(url.host_str(), Some("new"));
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![(Cow::Borrowed("dir"), Cow::Borrowed("/tmp/project +%"))]
        );
    }

    #[test]
    fn every_bundled_completion_sound_decodes() {
        use objc2::AnyThread;
        use objc2_app_kit::NSSound;
        use objc2_foundation::NSData;
        use waku_client::persistence::CompletionSound;

        for variant in CompletionSound::ALL {
            let data = NSData::with_bytes(completion_sound_data(variant));
            let sound = NSSound::initWithData(NSSound::alloc(), &data)
                .unwrap_or_else(|| panic!("{} should decode", variant.label()));
            assert!(sound.duration() > 0.0, "{}", variant.label());
        }
    }
}
