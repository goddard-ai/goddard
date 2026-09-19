//! Platform build metadata.
//!
//! Every native updater verifies the public release key exported here. On
//! Windows, Explorer, the taskbar, and the Programs list also read the icon
//! and version block out of the PE image itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    export_sparkle_public_key();
    export_commit_sha();

    #[cfg(target_os = "windows")]
    {
        // GPUI's Taffy layout and text shaping recurse deeply enough to
        // overflow the 1 MiB the MSVC linker defaults to.
        println!("cargo:rustc-link-arg-bins=/stack:{}", 8 * 1024 * 1024);
        embed_windows_resources();
    }
}

/// Republish `SUPublicEDKey` from the macOS Info.plist as a compile-time
/// constant.
///
/// The Linux and Windows updaters verify the same EdDSA signatures
/// `generate_appcast` writes, against the same key. Reading the plist here
/// rather than repeating the key in Rust means the platforms cannot drift
/// into a feed the app rejects.
fn export_sparkle_public_key() {
    const PLIST: &str = "resources/Info.plist";
    const KEY: &str = "<key>SUPublicEDKey</key>";

    println!("cargo:rerun-if-changed={PLIST}");

    let plist = std::fs::read_to_string(PLIST).expect("read the app Info.plist");
    let value = plist
        .split_once(KEY)
        .and_then(|(_, rest)| rest.split_once("<string>"))
        .and_then(|(_, rest)| rest.split_once("</string>"))
        .map(|(value, _)| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{PLIST} has no SUPublicEDKey"));

    println!("cargo:rustc-env=GODDARD_SPARKLE_PUBLIC_ED_KEY={value}");
}

/// Republish the checked-out commit as `GODDARD_COMMIT_SHA` so development
/// builds can report the exact source each binary was built from. Unset when
/// the build host has no git checkout — a tarball build simply omits it.
///
/// A `-dirty` suffix marks tracked worktree changes: a build almost always
/// carries uncommitted edits, so a bare hash would overstate what is running.
fn export_commit_sha() {
    let Some(mut sha) = git(&["rev-parse", "--short", "HEAD"]) else {
        return;
    };
    if git(&["status", "--porcelain"]).is_some_and(|status| !status.is_empty()) {
        sha.push_str("-dirty");
    }
    println!("cargo:rustc-env=GODDARD_COMMIT_SHA={sha}");

    // Re-resolve on checkout movement: HEAD covers branch switches and
    // detached commits, the ref it names covers new commits on a branch, and
    // the index covers staging so the dirty marker stays honest. The emitted
    // env only rebuilds dependents when the sha actually changes.
    // The package's own inputs are watched too: once any rerun-if-changed is
    // emitted, Cargo drops its everything-changes default, and without these
    // a rebuild caused by a plain source edit would reuse a stale flag.
    for directory in ["src", "locales", "assets"] {
        println!("cargo:rerun-if-changed={directory}");
    }
    let Some(git_dir) = git(&["rev-parse", "--absolute-git-dir"]) else {
        return;
    };
    let git_dir = std::path::PathBuf::from(git_dir);
    println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
    println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
    let Ok(head) = std::fs::read_to_string(git_dir.join("HEAD")) else {
        return;
    };
    let Some(reference) = head.trim().strip_prefix("ref: ") else {
        return;
    };
    // Branch refs live in the common dir every worktree shares; packed-refs
    // covers refs stored packed instead of loose.
    let Some(common) = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]) else {
        return;
    };
    let common = std::path::PathBuf::from(common);
    println!(
        "cargo:rerun-if-changed={}",
        common.join("packed-refs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        common.join(reference).display()
    );
}

/// `git <arguments>` trimmed to a single line, `None` when git is missing or
/// the command fails.
fn git(arguments: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(arguments)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|output| !output.is_empty())
}

#[cfg(target_os = "windows")]
fn embed_windows_resources() {
    const ICON: &str = "resources/windows/AppIcon.ico";

    println!("cargo:rerun-if-changed={ICON}");

    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ICON);
    // The resource compiler reads `.rc` as C source, so a Windows path
    // separator has to survive as a literal backslash.
    let icon = icon.to_string_lossy().replace('\\', "\\\\");

    let package_version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    // VERSIONINFO wants four numeric fields; Goddard's version has three.
    let mut fields = package_version
        .split(['.', '-', '+'])
        .map(|field| field.parse::<u16>().unwrap_or(0))
        .chain(std::iter::repeat(0));
    let file_version = format!(
        "{},{},{},{}",
        fields.next().unwrap_or(0),
        fields.next().unwrap_or(0),
        fields.next().unwrap_or(0),
        fields.next().unwrap_or(0),
    );
    let description = std::env::var("CARGO_PKG_DESCRIPTION").unwrap_or_default();

    let resources = format!(
        r#"1 ICON "{icon}"

1 VERSIONINFO
FILEVERSION {file_version}
PRODUCTVERSION {file_version}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "Goddard\0"
            VALUE "FileDescription", "{description}\0"
            VALUE "FileVersion", "{package_version}\0"
            VALUE "InternalName", "goddard\0"
            VALUE "OriginalFilename", "goddard.exe\0"
            VALUE "ProductName", "Goddard\0"
            VALUE "ProductVersion", "{package_version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    );

    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let script = out_dir.join("waku.rc");
    std::fs::write(&script, resources).expect("write the resource script");

    // GPUI embeds the application manifest through its own resource script,
    // so this one only claims the icon and version block.
    embed_resource::compile(&script, embed_resource::NONE)
        .manifest_optional()
        .expect("compile Windows resources");
}
