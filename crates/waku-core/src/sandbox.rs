//! One ephemeral shuru VM per sandboxed session.
//!
//! The daemon spawns `shuru run --stdio` and drives the provider CLI inside
//! the guest over its JSON-lines protocol (`spawn`/`exec`/`input`/`kill` plus
//! `output`/`exit` notifications). The worktree is mounted read-write so edits
//! land on the host checkout live; provider API keys ride the `--secret`
//! proxy substitution, so a real key never exists inside the guest.
//!
//! VM lifetime is the runtime's: the handle owns the `shuru` child process,
//! and dropping it kills the process — which closes the guest's stdio channel
//! and stops the VM. That covers daemon teardown the way the host-side
//! guardian script does for local children.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use base64::Engine;
use parking_lot::Mutex;
use serde_json::{Value, json};
use waku_protocol::model::{ProviderKind, SandboxSetupStatus};

/// The guest path the session's worktree is mounted at — also the provider's
/// working directory inside the VM.
const GUEST_WORKSPACE: &str = "/workspace";

/// Dev-server ports worth forwarding host:guest at boot. `shuru -p` is a
/// boot-time-only switch, so the set has to be guessed ahead — these cover
/// the usual frameworks (Vite/Next/Angular/Astro/Flask/Django/Rails/Hugo/
/// Jupyter) plus a couple of retries. A port that's busy on the host is
/// skipped rather than failing the launch, so a host dev server never
/// breaks sandboxed session start.
const DEV_PORTS: &[u16] = &[
    3000, 3001, 3002, 4000, 4173, 4200, 4321, 5000, 5173, 5174, 5175, 5176, 8000, 8001, 8080, 8888,
    9000, 9090, 1313,
];

/// The subset of [`DEV_PORTS`] currently free on the host — forwarding a
/// bound port would abort the whole `shuru run`.
fn free_dev_ports() -> Vec<u16> {
    DEV_PORTS
        .iter()
        .copied()
        .filter(|port| std::net::TcpListener::bind(("127.0.0.1", *port)).is_ok())
        .collect()
}

/// What one provider needs inside the guest: where its CLI lives, which
/// checkpoint provides it, which hosts its traffic may reach, and which
/// secrets the proxy substitutes.
struct GuestSpec {
    binary: &'static str,
    checkpoint: &'static str,
    /// The shell script run inside a throwaway VM (open network) whose disk
    /// becomes the checkpoint. Must leave the provider binary installed —
    /// and must install it outside `/.local`: the builder VM's HOME is `/`,
    /// while a session VM mounts the provider home over `/root`, so anything
    /// left under the build-time home is shadowed at runtime.
    install: &'static str,
    allow_hosts: &'static [&'static str],
    /// (guest env name, host env name, allowed hosts) — the guest receives a
    /// placeholder; the proxy swaps in the real value only for these hosts.
    /// When every entry is unset on the host, the provider needs credentials
    /// in its shared sandbox home instead.
    secrets: &'static [(&'static str, &'static str, &'static [&'static str])],
    /// Human-readable name list for the missing-credentials error.
    key_hint: &'static str,
    /// Credential paths under the provider's guest home — any one that
    /// exists (file or non-empty directory) marks the provider signed in.
    /// Checked on the host: no VM boot needed to gate a launch.
    auth_files: &'static [&'static str],
    /// argv tail after `binary` for the provider's interactive sign-in, run
    /// in a throwaway guest attached to a terminal tab. `None` = environment
    /// secrets are the only way in.
    login: Option<&'static [&'static str]>,
    /// Host ports the login flow needs forwarded guest-side — OAuth flows
    /// that listen on a fixed localhost port. Empty for paste-token and
    /// device-code flows.
    login_ports: &'static [u16],
}

const CODEX_ENDPOINTS: &[&str] = &[
    "api.openai.com",
    "*.openai.com",
    "chatgpt.com",
    "*.chatgpt.com",
    "auth.openai.com",
];

const CLAUDE_ENDPOINTS: &[&str] = &[
    "api.anthropic.com",
    "statsig.anthropic.com",
    "claude.ai",
    "*.claude.ai",
];

const DEVIN_ENDPOINTS: &[&str] = &[
    "api.devin.ai",
    "app.devin.ai",
    "*.devin.ai",
    "server.codeium.com",
    "*.codeium.com",
];

const CURSOR_ENDPOINTS: &[&str] = &["cursor.com", "*.cursor.com", "cursor.sh", "*.cursor.sh"];

const AMP_ENDPOINTS: &[&str] = &["ampcode.com", "*.ampcode.com"];

const DROID_ENDPOINTS: &[&str] = &["factory.ai", "*.factory.ai"];

/// Fx fans out to whichever backend the user signed into — the Vercel AI
/// Gateway, Codex, or Grok — so its allowlist is the union.
const FX_ENDPOINTS: &[&str] = &[
    "ai-gateway.vercel.sh",
    "vercel.com",
    "*.vercel.com",
    "*.vercel.sh",
    "fx.sh",
    "*.fx.sh",
    "api.openai.com",
    "*.openai.com",
    "chatgpt.com",
    "*.chatgpt.com",
    "api.x.ai",
    "*.x.ai",
];

const GROK_ENDPOINTS: &[&str] = &["x.ai", "*.x.ai"];

const KIMI_ENDPOINTS: &[&str] = &[
    "api.moonshot.ai",
    "*.moonshot.ai",
    "api.moonshot.cn",
    "*.moonshot.cn",
    "kimi.com",
    "*.kimi.com",
];

/// Goose talks to whichever LLM backend the user configured — the allowlist
/// is the union of the common providers rather than a vendor's endpoints.
const GOOSE_ENDPOINTS: &[&str] = &[
    "api.anthropic.com",
    "*.anthropic.com",
    "api.openai.com",
    "*.openai.com",
    "chatgpt.com",
    "*.chatgpt.com",
    "generativelanguage.googleapis.com",
    "*.googleapis.com",
    "api.moonshot.cn",
    "api.moonshot.ai",
    "api.deepseek.com",
    "api.groq.com",
    "api.mistral.ai",
    "openrouter.ai",
    "*.openrouter.ai",
    "api.x.ai",
    "ollama.com",
    "*.ollama.com",
    "github.com",
    "*.github.com",
    "aaif-goose.github.io",
];

const PI_ENDPOINTS: &[&str] = &["pi.dev", "*.pi.dev"];

const OHMYPI_ENDPOINTS: &[&str] = &["omp.sh", "*.omp.sh"];

const CODEX_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y bubblewrap; \
    cd /tmp; \
    curl -fsSL https://github.com/openai/codex/releases/latest/download/codex-aarch64-unknown-linux-musl.tar.gz -o codex.tar.gz; \
    mkdir -p codex-pkg; \
    tar -xzf codex.tar.gz -C codex-pkg; \
    bin=$(find codex-pkg -type f -name 'codex*' | head -1); \
    install -m 755 \"$bin\" /usr/local/bin/codex; \
    codex --version";

const CLAUDE_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://claude.ai/install.sh | bash; \
    install -m 755 /.local/bin/claude /usr/local/bin/claude; \
    claude --version";

// Vendor installers may exit nonzero after a successful install (post-install
// login nudges, telemetry checks) — `|| true` tolerates that while the
// `--version` check at the end still fails the build when the binary never
// landed. Binaries are symlinked rather than copied: installers lay out
// versioned directories whose launcher needs its siblings, and `/.local`
// stays visible inside session VMs (only `/root` is mounted over).

const DEVIN_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://cli.devin.ai/install.sh | bash || true; \
    ln -sfn /.local/bin/devin /usr/local/bin/devin; \
    devin --version";

const CURSOR_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://cursor.com/install | bash || true; \
    cp -a /.local/share/cursor-agent /opt/cursor-agent; \
    bin=$(find /opt/cursor-agent -type f \\( -name cursor-agent -o -name agent \\) | head -1); \
    ln -sf \"$bin\" /usr/local/bin/cursor-agent; \
    cursor-agent --version";

const AMP_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://ampcode.com/install.sh | bash || true; \
    command -v amp >/dev/null || { bin=$(find /.local /.amp /opt -maxdepth 4 -name amp 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/amp; }; \
    amp --version";

const DROID_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://app.factory.ai/cli | sh || true; \
    command -v droid >/dev/null || { bin=$(find /.local /opt /usr/local -maxdepth 4 -name droid 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/droid; }; \
    droid --version";

const FX_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://fx.sh/setup.sh | bash || true; \
    command -v fx >/dev/null || { bin=$(find /.local /opt /usr/local -maxdepth 4 -name fx 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/fx; }; \
    fx --version";

const GROK_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://x.ai/cli/install.sh | bash || true; \
    command -v grok >/dev/null || { bin=$(find /.local /opt /usr/local -maxdepth 4 -name grok 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/grok; }; \
    grok --version";

const KIMI_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash || true; \
    command -v kimi >/dev/null || { bin=$(find /.local /.kimi-code /opt /usr/local -maxdepth 4 -name kimi 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/kimi; }; \
    kimi --version";

// Goose's release tarball is bz2-compressed — the base image lacks bzip2.
const GOOSE_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates bzip2; \
    curl -fsSL https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh | bash || true; \
    command -v goose >/dev/null || { bin=$(find /.local /opt /usr/local -maxdepth 4 -name goose 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/goose; }; \
    goose --version";

// pi is a Node CLI — install Node through mise into the same fixed prefix
// runtime layers use, so `/opt/mise/shims` on the session PATH resolves it.
const PI_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://mise.jdx.dev/install.sh | sh; \
    install -m 755 /.local/bin/mise /usr/local/bin/mise; \
    mkdir -p /opt/mise /opt/mise-config; \
    export MISE_DATA_DIR=/opt/mise MISE_CONFIG_DIR=/opt/mise-config \
        MISE_GLOBAL_CONFIG_FILE=/opt/mise-config/config.toml; \
    mise use -g node@lts; \
    export PATH=/opt/mise/shims:$PATH; \
    curl -fsSL https://pi.dev/install.sh | sh || true; \
    command -v pi >/dev/null || { bin=$(find /.local /.pi /opt /usr/local -maxdepth 8 -name pi 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/pi; }; \
    pi --version";

const OHMYPI_INSTALL: &str = "set -eux; \
    apt-get update; \
    apt-get install -y curl ca-certificates; \
    curl -fsSL https://omp.sh/install | sh || true; \
    command -v omp >/dev/null || { bin=$(find /.local /opt /usr/local -maxdepth 4 -name omp 2>/dev/null | head -1); \
    ln -sf \"$bin\" /usr/local/bin/omp; }; \
    omp --version";

fn guest_spec(provider: ProviderKind) -> Option<GuestSpec> {
    match provider {
        ProviderKind::Codex => Some(GuestSpec {
            binary: "/usr/local/bin/codex",
            checkpoint: "waku-provider-codex",
            install: CODEX_INSTALL,
            allow_hosts: CODEX_ENDPOINTS,
            secrets: &[
                ("OPENAI_API_KEY", "OPENAI_API_KEY", CODEX_ENDPOINTS),
                ("CODEX_API_KEY", "CODEX_API_KEY", CODEX_ENDPOINTS),
            ],
            key_hint: "OPENAI_API_KEY or CODEX_API_KEY",
            auth_files: &[".codex/auth.json"],
            // Device-code flow — the browser OAuth listener can't receive
            // a host-browser callback from inside the guest.
            login: Some(&["login", "--device-auth"]),
            login_ports: &[],
        }),
        ProviderKind::Claude => Some(GuestSpec {
            binary: "/usr/local/bin/claude",
            checkpoint: "waku-provider-claude",
            install: CLAUDE_INSTALL,
            allow_hosts: CLAUDE_ENDPOINTS,
            secrets: &[
                ("ANTHROPIC_API_KEY", "ANTHROPIC_API_KEY", CLAUDE_ENDPOINTS),
                (
                    "CLAUDE_CODE_OAUTH_TOKEN",
                    "CLAUDE_CODE_OAUTH_TOKEN",
                    CLAUDE_ENDPOINTS,
                ),
            ],
            key_hint: "ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN",
            auth_files: &[".claude/.credentials.json"],
            login: Some(&["auth", "login"]),
            login_ports: &[],
        }),
        ProviderKind::Amp => Some(GuestSpec {
            binary: "/usr/local/bin/amp",
            checkpoint: "waku-provider-amp",
            install: AMP_INSTALL,
            allow_hosts: AMP_ENDPOINTS,
            secrets: &[("AMP_API_KEY", "AMP_API_KEY", AMP_ENDPOINTS)],
            key_hint: "AMP_API_KEY",
            auth_files: &[".config/amp", ".amp"],
            login: Some(&["login"]),
            login_ports: &[],
        }),
        ProviderKind::Cursor => Some(GuestSpec {
            binary: "/usr/local/bin/cursor-agent",
            checkpoint: "waku-provider-cursor",
            install: CURSOR_INSTALL,
            allow_hosts: CURSOR_ENDPOINTS,
            secrets: &[("CURSOR_API_KEY", "CURSOR_API_KEY", CURSOR_ENDPOINTS)],
            key_hint: "CURSOR_API_KEY",
            auth_files: &[".config/cursor-agent", ".cursor-agent"],
            login: Some(&["login"]),
            login_ports: &[],
        }),
        ProviderKind::Devin => Some(GuestSpec {
            binary: "/usr/local/bin/devin",
            checkpoint: "waku-provider-devin",
            install: DEVIN_INSTALL,
            allow_hosts: DEVIN_ENDPOINTS,
            secrets: &[],
            key_hint: "",
            auth_files: &[".local/share/devin/credentials.toml"],
            // The manual token flow prints a URL and accepts a pasted
            // token — the only flow guaranteed to work with no browser
            // callback into the guest.
            login: Some(&["auth", "login", "--force-manual-token-flow"]),
            login_ports: &[],
        }),
        ProviderKind::Droid => Some(GuestSpec {
            binary: "/usr/local/bin/droid",
            checkpoint: "waku-provider-droid",
            install: DROID_INSTALL,
            allow_hosts: DROID_ENDPOINTS,
            secrets: &[("FACTORY_API_KEY", "FACTORY_API_KEY", DROID_ENDPOINTS)],
            key_hint: "FACTORY_API_KEY",
            auth_files: &[".factory", ".config/droid"],
            // Sign-in happens inside the TUI — the tab just runs `droid`.
            login: Some(&[]),
            login_ports: &[],
        }),
        ProviderKind::Fx => Some(GuestSpec {
            binary: "/usr/local/bin/fx",
            checkpoint: "waku-provider-fx",
            install: FX_INSTALL,
            allow_hosts: FX_ENDPOINTS,
            secrets: &[("AI_GATEWAY_API_KEY", "AI_GATEWAY_API_KEY", FX_ENDPOINTS)],
            key_hint: "AI_GATEWAY_API_KEY",
            auth_files: &[".config/fx", ".fx"],
            login: Some(&["login"]),
            login_ports: &[],
        }),
        ProviderKind::Goose => Some(GuestSpec {
            binary: "/usr/local/bin/goose",
            checkpoint: "waku-provider-goose",
            install: GOOSE_INSTALL,
            allow_hosts: GOOSE_ENDPOINTS,
            // Goose is multi-provider — credentials are whatever `goose
            // configure` writes, not a fixed env name.
            secrets: &[],
            key_hint: "",
            auth_files: &[".config/goose"],
            login: Some(&["configure"]),
            login_ports: &[],
        }),
        ProviderKind::Grok => Some(GuestSpec {
            binary: "/usr/local/bin/grok",
            checkpoint: "waku-provider-grok",
            install: GROK_INSTALL,
            allow_hosts: GROK_ENDPOINTS,
            secrets: &[("XAI_API_KEY", "XAI_API_KEY", GROK_ENDPOINTS)],
            key_hint: "XAI_API_KEY",
            auth_files: &[".grok", ".config/grok"],
            // Device-code flow — no browser can open inside the guest.
            login: Some(&["login", "--device-auth"]),
            login_ports: &[],
        }),
        ProviderKind::Kimi => Some(GuestSpec {
            binary: "/usr/local/bin/kimi",
            checkpoint: "waku-provider-kimi",
            install: KIMI_INSTALL,
            allow_hosts: KIMI_ENDPOINTS,
            secrets: &[],
            key_hint: "",
            auth_files: &[".kimi", ".config/kimi"],
            login: Some(&["login"]),
            login_ports: &[],
        }),
        ProviderKind::OhMyPi => Some(GuestSpec {
            binary: "/usr/local/bin/omp",
            checkpoint: "waku-provider-ohmypi",
            install: OHMYPI_INSTALL,
            allow_hosts: OHMYPI_ENDPOINTS,
            secrets: &[],
            key_hint: "",
            auth_files: &[".omp", ".config/omp"],
            login: Some(&["auth-broker", "login"]),
            login_ports: &[],
        }),
        ProviderKind::Pi => Some(GuestSpec {
            binary: "/usr/local/bin/pi",
            checkpoint: "waku-provider-pi",
            install: PI_INSTALL,
            allow_hosts: PI_ENDPOINTS,
            secrets: &[],
            key_hint: "",
            auth_files: &[".pi", ".config/pi"],
            // Sign-in is `/login` inside the TUI — the tab runs `pi`.
            login: Some(&[]),
            login_ports: &[],
        }),
        _ => None,
    }
}

/// Which providers can run inside the sandbox VM today. Others fail honestly
/// rather than silently running on the host — and `ProviderKind::
/// supports_sandbox` answers the same question for clients, so keep the
/// guest spec table and the protocol flag in the same set.
pub fn sandbox_capable(provider: ProviderKind) -> bool {
    debug_assert_eq!(guest_spec(provider).is_some(), provider.supports_sandbox());
    provider.supports_sandbox()
}

/// The provider's persistent guest home — one directory per provider under
/// the daemon's data dir, mounted at `/root` in every VM that runs that
/// provider. Credentials a sign-in writes land here and survive across
/// sessions and VM restarts, which is what makes sign-in a once-per-provider
/// step. Sharing is per provider, never global: a session mounts only its
/// own provider's home, so no session can read another provider's creds.
fn sandbox_home(provider: ProviderKind, data_dir: &Path) -> PathBuf {
    data_dir.join("sandbox-homes").join(provider.id())
}

/// Whether any of the spec's credential paths exists under the provider's
/// sandbox home — a directory counts only when it has entries.
fn sandbox_signed_in_at(home: &Path, spec: &GuestSpec) -> bool {
    spec.auth_files.iter().any(|path| {
        let path = home.join(path);
        if path.is_file() {
            return true;
        }
        path.is_dir()
            && std::fs::read_dir(&path)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
    })
}

/// Whether the provider's shared sandbox home already holds credentials.
/// Public for the `sandboxAuthStatus` daemon command.
pub fn sandbox_signed_in(provider: ProviderKind, data_dir: &Path) -> bool {
    let Some(spec) = guest_spec(provider) else {
        return false;
    };
    sandbox_signed_in_at(&sandbox_home(provider, data_dir), &spec)
}

/// The host-side invocation that signs a provider in: `shuru run` a
/// throwaway VM off the provider checkpoint with its home mounted, running
/// the spec's login argv. The caller wraps it in a PTY — `shuru run` gives
/// the guest a real tty, so interactive and browser-assisted flows work.
/// The returned cwd is the homes root — shuru only mounts host paths
/// beneath its working directory.
pub fn sign_in_invocation(
    provider: ProviderKind,
    data_dir: &Path,
) -> anyhow::Result<(PathBuf, Vec<String>, PathBuf)> {
    let spec = guest_spec(provider)
        .ok_or_else(|| anyhow!("{} cannot run in the sandbox VM", provider.display_name()))?;
    let login = spec.login.ok_or_else(|| {
        anyhow!(
            "{} takes credentials from the environment ({}), not an interactive sign-in",
            provider.display_name(),
            spec.key_hint,
        )
    })?;
    let shuru = shuru_binary()?;
    let home = sandbox_home(provider, data_dir);
    std::fs::create_dir_all(&home).with_context(|| {
        format!(
            "could not create the sandbox home directory {}",
            home.display()
        )
    })?;
    let homes_root = home.parent().unwrap_or(data_dir).to_path_buf();
    let mut args = vec![
        "run".to_owned(),
        "--from".to_owned(),
        spec.checkpoint.to_owned(),
        // The sign-in VM writes the provider's credentials back through the
        // mounted home — rw mounts need the explicit opt-in.
        "--allow-host-writes".to_owned(),
        "--mount".to_owned(),
        format!("{}:/root:rw", home.display()),
        "--allow-net".to_owned(),
    ];
    for host in spec.allow_hosts {
        args.push("--allow-host".to_owned());
        args.push((*host).to_owned());
    }
    for port in spec.login_ports {
        args.push("-p".to_owned());
        args.push(format!("{port}:{port}"));
    }
    args.push("--".to_owned());
    // `shuru run` guests default to HOME=/ and a minimal env — the provider's
    // login must resolve the mounted home, or credentials die with the VM.
    args.push("env".to_owned());
    for kv in [
        "HOME=/root",
        "USER=root",
        "SHELL=/bin/sh",
        "TERM=xterm-256color",
        "TMPDIR=/tmp",
        // No browser can open inside the guest — CLIs honoring this fall
        // back to device-code or printed-URL flows (cursor-agent et al).
        "NO_OPEN_BROWSER=1",
        "PATH=/usr/local/sbin:/usr/local/bin:/opt/mise/shims:/usr/bin:/sbin:/bin",
        "MISE_DATA_DIR=/opt/mise",
        "MISE_CONFIG_DIR=/opt/mise-config",
        "MISE_GLOBAL_CONFIG_FILE=/opt/mise-config/config.toml",
    ] {
        args.push(kv.to_owned());
    }
    args.push(spec.binary.to_owned());
    args.extend(login.iter().map(|arg| arg.to_string()));
    Ok((shuru, args, homes_root))
}

/// The shuru CLI itself: `GODDARD_SHURU_BIN` wins for development, then the
/// shell's PATH, then the documented install location.
pub(crate) fn shuru_binary() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("GODDARD_SHURU_BIN").map(PathBuf::from) {
        return Ok(path);
    }
    // Release bundles carry the VM runner at Contents/Resources/shuru — the
    // daemon lives in Contents/MacOS, so its sibling is a deterministic
    // lookup that needs no PATH.
    if let Ok(exe) = std::env::current_exe()
        && let Some(bundled) = exe.parent().map(|dir| dir.join("../Resources/shuru"))
        && bundled.is_file()
    {
        return Ok(bundled);
    }
    if let Some(path) = crate::command_env::find_executable("shuru") {
        return Ok(path);
    }
    let local = dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".local/bin/shuru");
    if local.is_file() {
        return Ok(local);
    }
    bail!("the shuru binary is not installed — sandboxed tasks need it (https://shuru.run)")
}

/// Download the OS image on first use — `shuru init` is a no-op once assets
/// exist, so calling it unconditionally costs nothing after the first time.
/// The download phase is reported only when the rootfs is actually absent so
/// every later launch skips straight to booting.
fn ensure_os_image(
    shuru: &Path,
    progress: &mut impl FnMut(SandboxSetupStatus),
) -> anyhow::Result<()> {
    let missing = !dirs::home_dir()
        .map(|home| home.join(".local/share/shuru/rootfs.ext4"))
        .is_some_and(|rootfs| rootfs.is_file());
    if missing {
        progress(SandboxSetupStatus::DownloadingImage);
    }
    static ONCE: Mutex<()> = Mutex::new(());
    let _guard = ONCE.lock();
    let status = crate::command_env::plain_command(shuru)
        .arg("init")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .status()
        .context("could not run `shuru init`")?;
    if !status.success() {
        bail!("the sandbox OS image is not downloaded and `shuru init` failed");
    }
    Ok(())
}

fn checkpoint_names(shuru: &Path) -> anyhow::Result<Vec<String>> {
    let output = crate::command_env::plain_command(shuru)
        .args(["checkpoint", "list"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("could not list shuru checkpoints")?;
    if !output.status.success() {
        bail!("`shuru checkpoint list` failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1) // NAME/SIZE/CREATED header
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect())
}

/// The install command that turns the base image into a provider checkpoint.
/// Runs inside the VM with open network during `checkpoint create`; the
/// session VM itself is restricted to the provider's endpoint allowlist.
fn checkpoint_install_argv(spec: &GuestSpec) -> Vec<String> {
    vec!["sh".to_owned(), "-c".to_owned(), spec.install.to_owned()]
}

/// Build a checkpoint layer once — the download happens in a throwaway VM
/// with open network, and every later session boots straight from the saved
/// disk state. `from` chains onto an earlier checkpoint (base image when
/// `None`). `phase` produces the progress event only when a build actually
/// runs — a cached layer reports nothing and skips straight through.
fn ensure_layer(
    shuru: &Path,
    name: &str,
    from: Option<&str>,
    install: &[String],
    phase: impl FnOnce() -> SandboxSetupStatus,
    progress: &mut impl FnMut(SandboxSetupStatus),
) -> anyhow::Result<()> {
    if checkpoint_names(shuru)?.iter().any(|n| n == name) {
        return Ok(());
    }
    progress(phase());
    eprintln!(
        "goddard-daemon: building sandbox checkpoint {name} (first sandboxed task downloads its toolchain)"
    );
    let mut command = crate::command_env::plain_command(shuru);
    command.arg("checkpoint").arg("create").arg(name);
    if let Some(from) = from {
        command.arg("--from").arg(from);
    }
    command
        .arg("--allow-net")
        .arg("--")
        .args(install)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command
        .status()
        .with_context(|| format!("could not build sandbox checkpoint {name}"))?;
    if !status.success() {
        bail!("sandbox checkpoint {name} failed to build — the toolchain could not be installed");
    }
    Ok(())
}

/// A project toolchain the worktree's manifests ask for — installed into a
/// checkpoint layer with mise so sessions never install at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeSpec {
    tool: &'static str,
    version: String,
}

/// Where mise keeps its toolchains inside the guest. Pinned outside HOME so
/// it resolves identically no matter which HOME a spawn env carries.
const MISE_DATA_DIR: &str = "/opt/mise";
const MISE_CONFIG_DIR: &str = "/opt/mise-config";

/// Scan the worktree's manifests for the runtimes a task will need. Cheap
/// file reads on the spawn path — a handful of existence checks and small
/// file parses, never a recursive walk.
fn detect_runtimes(worktree: &Path) -> Vec<RuntimeSpec> {
    let has = |name: &str| worktree.join(name).is_file();
    let read = |name: &str| std::fs::read_to_string(worktree.join(name)).ok();
    let tool_versions = read(".tool-versions")
        .map(|content| parse_tool_versions(&content))
        .unwrap_or_default();
    let mise_tools = read("mise.toml")
        .map(|content| parse_mise_tools(&content))
        .unwrap_or_default();
    let pinned = |tool: &str| -> Option<String> {
        tool_versions
            .get(tool)
            .or_else(|| mise_tools.get(tool))
            .cloned()
    };

    let mut specs = Vec::new();
    if has("deno.json") || has("deno.jsonc") {
        specs.push(RuntimeSpec {
            tool: "deno",
            version: pinned("deno").unwrap_or_else(|| "latest".to_owned()),
        });
    }
    if has("bun.lock") || has("bun.lockb") {
        specs.push(RuntimeSpec {
            tool: "bun",
            version: pinned("bun").unwrap_or_else(|| "latest".to_owned()),
        });
    }
    if has("package.json") {
        let version = pinned("node")
            .or_else(|| {
                read(".nvmrc").map(|content| {
                    let v = content.trim().trim_start_matches('v');
                    if v.starts_with("lts") {
                        "lts".to_owned()
                    } else {
                        v.to_owned()
                    }
                })
            })
            .or_else(|| {
                read("package.json").and_then(|content| {
                    serde_json::from_str::<Value>(&content)
                        .ok()?
                        .pointer("/engines/node")
                        .and_then(Value::as_str)
                        .and_then(|range| {
                            // "20.x", "^20", ">=20" — take the pinned major;
                            // anything vaguer gets the LTS fallback.
                            range.split(['.', 'x', ' ']).find_map(|part| {
                                let digits: String =
                                    part.chars().filter(|c| c.is_ascii_digit()).collect();
                                (!digits.is_empty()).then_some(digits)
                            })
                        })
                })
            })
            .unwrap_or_else(|| "lts".to_owned());
        specs.push(RuntimeSpec {
            tool: "node",
            version,
        });
    }
    if has("pyproject.toml") || has("requirements.txt") || has("uv.lock") || has(".python-version")
    {
        let version = pinned("python")
            .or_else(|| read(".python-version").map(|v| v.trim().to_owned()))
            .unwrap_or_else(|| "3".to_owned());
        specs.push(RuntimeSpec {
            tool: "python",
            version,
        });
    }
    if has("Cargo.toml") {
        let version = pinned("rust")
            .or_else(|| {
                read("rust-toolchain.toml").and_then(|content| {
                    content.lines().find_map(|line| {
                        let line = line.trim();
                        line.strip_prefix("channel")
                            .and_then(|rest| rest.split('"').nth(1))
                            .map(str::to_owned)
                    })
                })
            })
            .or_else(|| read("rust-toolchain").map(|v| v.trim().to_owned()))
            .unwrap_or_else(|| "stable".to_owned());
        specs.push(RuntimeSpec {
            tool: "rust",
            version,
        });
    }
    if has("go.mod") {
        let version = pinned("go")
            .or_else(|| {
                read("go.mod").and_then(|content| {
                    content.lines().find_map(|line| {
                        line.trim().strip_prefix("go ").map(|v| v.trim().to_owned())
                    })
                })
            })
            .unwrap_or_else(|| "latest".to_owned());
        specs.push(RuntimeSpec {
            tool: "go",
            version,
        });
    }
    specs
}

fn parse_tool_versions(content: &str) -> HashMap<&'static str, String> {
    const TOOLS: &[&str] = &["node", "python", "rust", "go", "bun", "deno"];
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        if let (Some(tool), Some(version)) = (parts.next(), parts.next())
            && TOOLS.contains(&tool)
        {
            map.insert(
                *TOOLS.iter().find(|t| **t == tool).unwrap(),
                version.to_owned(),
            );
        }
    }
    map
}

/// The `[tools]` table of a mise.toml — `node = "lts"` pairs. Loose line
/// parsing keeps this dependency-free; a malformed file just yields nothing.
fn parse_mise_tools(content: &str) -> HashMap<&'static str, String> {
    const TOOLS: &[&str] = &["node", "python", "rust", "go", "bun", "deno"];
    let mut map = HashMap::new();
    let mut in_tools = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_tools = line == "[tools]";
            continue;
        }
        if !in_tools {
            continue;
        }
        if let Some((name, value)) = line.split_once('=')
            && let Some(tool) = TOOLS.iter().find(|t| **t == name.trim())
        {
            let version = value.trim().trim_matches('"').trim_matches('\'');
            if !version.is_empty() {
                map.insert(*tool, version.to_owned());
            }
        }
    }
    map
}

/// The checkpoint a session boots: the provider layer alone, or a runtime
/// layer stacked on it named for exactly what it contains.
fn session_checkpoint(spec: &GuestSpec, runtimes: &[RuntimeSpec]) -> String {
    if runtimes.is_empty() {
        return spec.checkpoint.to_owned();
    }
    let mut name = spec.checkpoint.to_owned();
    for runtime in runtimes {
        let version: String = runtime
            .version
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        name.push_str(&format!("-{}-{}", runtime.tool, version));
    }
    name
}

/// The mise install script for a runtime layer — mise itself lands in a
/// fixed prefix, then each requested toolchain installs to `MISE_DATA_DIR`
/// so the spawn env can point at it unconditionally.
fn runtime_install_argv(runtimes: &[RuntimeSpec]) -> Vec<String> {
    let installs: String = runtimes
        .iter()
        .map(|r| format!("mise use -g {}@{}; ", r.tool, r.version))
        .collect();
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "set -eux; \
             apt-get update -qq; \
             apt-get install -y -qq curl ca-certificates; \
             curl -fsSL https://mise.jdx.dev/install.sh | sh; \
             install -m 755 /.local/bin/mise /usr/local/bin/mise; \
             mkdir -p {MISE_DATA_DIR} {MISE_CONFIG_DIR}; \
             export MISE_DATA_DIR={MISE_DATA_DIR} MISE_CONFIG_DIR={MISE_CONFIG_DIR} \
                 MISE_GLOBAL_CONFIG_FILE={MISE_CONFIG_DIR}/config.toml; \
             {installs}\
             mise ls"
        ),
    ]
}

/// Environment variables that mean something only on the host: the agent
/// surface points at this daemon's loopback (unreachable in the guest), and
/// secret names are dropped so the proxy's placeholders are the only values
/// the guest ever sees.
fn env_scrubbed(name: &str, secrets: &[String]) -> bool {
    secrets.iter().any(|secret| secret == name)
        || matches!(name, "SSH_AUTH_SOCK" | "SSH_AGENT_PID")
        || name.starts_with("GODDARD_")
        || name.starts_with("WAKU_")
}

/// Spawn the command a driver built — on the host, or inside the session's
/// guest when `sandbox` is set. Reads argv/env/cwd back out of the `Command`
/// the same way [`crate::command_env::guard_command`] does.
pub fn spawn(command: &Command, sandbox: Option<&Arc<ShuruVm>>) -> anyhow::Result<DriverChild> {
    match sandbox {
        None => {
            let mut clone = Command::new(command.get_program());
            clone.args(command.get_args());
            for (name, value) in command.get_envs() {
                match value {
                    Some(value) => {
                        clone.env(name, value);
                    }
                    None => {
                        clone.env_remove(name);
                    }
                }
            }
            if let Some(cwd) = command.get_current_dir() {
                clone.current_dir(cwd);
            }
            clone
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            crate::command_env::spawn(&mut clone)
                .map(DriverChild::host)
                .context("could not spawn the provider process")
        }
        Some(vm) => vm.spawn(command),
    }
}

/// Ensure the OS image and the provider's checkpoint layer exist — the
/// work a session launch and a sign-in VM share. Reports setup phases only
/// when work actually runs.
fn ensure_provider_image(
    provider: ProviderKind,
    spec: &GuestSpec,
    shuru: &Path,
    progress: &mut impl FnMut(SandboxSetupStatus),
) -> anyhow::Result<()> {
    ensure_os_image(shuru, progress)?;
    ensure_layer(
        shuru,
        spec.checkpoint,
        None,
        &checkpoint_install_argv(spec),
        || SandboxSetupStatus::BuildingToolchain {
            toolchain: provider.display_name().to_owned(),
        },
        progress,
    )
}

/// Build the OS image and provider checkpoint a sign-in VM boots from —
/// the first sandboxed touch on a provider pays the install here rather
/// than mid-login.
pub fn ensure_sign_in_image(provider: ProviderKind) -> anyhow::Result<()> {
    let spec = guest_spec(provider)
        .ok_or_else(|| anyhow!("{} cannot run in the sandbox VM", provider.display_name()))?;
    let shuru = shuru_binary()?;
    ensure_provider_image(provider, &spec, &shuru, &mut |_| {})
}

/// Prepare a sandboxed launch: resolve the toolchain checkpoint, boot the VM
/// with the worktree mounted, and hand back everything the driver needs to
/// spawn inside it. Honest failure — a sandboxed session that cannot prepare
/// reports why instead of running on the host. `progress` reports the phase
/// as the launch reaches it so a client can name what the session is doing
/// while it sits at Connecting.
pub fn launch_for_provider(
    provider: ProviderKind,
    worktree: &Path,
    data_dir: &Path,
    mut progress: impl FnMut(SandboxSetupStatus),
) -> anyhow::Result<GuestLaunch> {
    let spec = guest_spec(provider).ok_or_else(|| {
        anyhow!(
            "{} cannot run in the sandbox VM yet — pick This Mac or another provider",
            provider.display_name()
        )
    })?;
    let shuru = shuru_binary()?;
    ensure_provider_image(provider, &spec, &shuru, &mut progress)?;
    let runtimes = detect_runtimes(worktree);
    let checkpoint = session_checkpoint(&spec, &runtimes);
    if !runtimes.is_empty() {
        let toolchain = runtimes
            .iter()
            .map(|runtime| format!("{}@{}", runtime.tool, runtime.version))
            .collect::<Vec<_>>()
            .join(", ");
        ensure_layer(
            &shuru,
            &checkpoint,
            Some(spec.checkpoint),
            &runtime_install_argv(&runtimes),
            || SandboxSetupStatus::BuildingToolchain { toolchain },
            &mut progress,
        )?;
    }

    // The guest HOME is the provider's shared sandbox home — sign-in state
    // and provider session state (`~/.codex`, `~/.claude`) survive VM
    // restarts and carry across every task on this provider. First launch
    // adopts a per-worktree home from before the homes were shared so
    // existing resume state is not stranded.
    let guest_home = sandbox_home(provider, data_dir);
    if !guest_home.exists() {
        let legacy_home = worktree.join(".goddard").join("sandbox-home");
        if legacy_home.is_dir()
            && let Some(parent) = guest_home.parent()
        {
            let _ = std::fs::create_dir_all(parent);
            let _ = std::fs::rename(&legacy_home, &guest_home);
        }
    }
    std::fs::create_dir_all(&guest_home).with_context(|| {
        format!(
            "could not create the sandbox home directory {}",
            guest_home.display()
        )
    })?;

    let mut secrets = Vec::new();
    let mut scrub = Vec::new();
    for &(guest_name, host_env, hosts) in spec.secrets {
        if std::env::var_os(host_env).is_some() {
            secrets.push(SecretSpec {
                guest_name: guest_name.to_owned(),
                host_env: host_env.to_owned(),
                hosts: hosts.iter().map(|host| host.to_string()).collect(),
            });
            scrub.push(guest_name.to_owned());
        }
    }
    // No env secret is set and the shared home holds no credentials — the
    // provider cannot authenticate. When the spec offers an interactive
    // sign-in, report NeedsAuth so the client can open it in a terminal tab
    // instead of surfacing a dead end.
    if secrets.is_empty() && !sandbox_signed_in_at(&guest_home, &spec) {
        if spec.login.is_some() {
            progress(SandboxSetupStatus::NeedsAuth);
            return Err(anyhow!(keyed!(
                "errors.sandbox_sign_in_required",
                provider = provider.display_name().to_owned()
            )))
            .context("could not start the sandboxed provider");
        }
        bail!(
            "sandboxed {} needs {} in the environment — \
             the key is proxied to the provider endpoint and never enters the VM",
            provider.display_name(),
            spec.key_hint,
        );
    }

    progress(SandboxSetupStatus::BootingVm);
    let vm = ShuruVm::launch(GuestConfig {
        shuru,
        // shuru only mounts host paths beneath its own working directory —
        // run it from the worktree's parent so the mount validates.
        cwd: worktree.parent().unwrap_or(worktree).to_path_buf(),
        checkpoint,
        mounts: vec![
            (worktree.to_path_buf(), GUEST_WORKSPACE.to_owned()),
            (guest_home, "/root".to_owned()),
        ],
        allow_hosts: spec
            .allow_hosts
            .iter()
            .map(|host| host.to_string())
            .collect(),
        ports: free_dev_ports(),
        secrets,
        scrub,
    })?;

    Ok(GuestLaunch {
        vm,
        binary: PathBuf::from(spec.binary),
        cwd: PathBuf::from(GUEST_WORKSPACE),
    })
}

/// What a sandboxed session needs injected into its start options.
pub struct GuestLaunch {
    pub vm: Arc<ShuruVm>,
    pub binary: PathBuf,
    pub cwd: PathBuf,
}

struct SecretSpec {
    guest_name: String,
    host_env: String,
    hosts: Vec<String>,
}

struct GuestConfig {
    shuru: PathBuf,
    /// The shuru process's working directory — its mount guard rejects any
    /// host path outside this, so it must contain every mounted worktree.
    cwd: PathBuf,
    checkpoint: String,
    mounts: Vec<(PathBuf, String)>,
    allow_hosts: Vec<String>,
    /// Dev-server ports forwarded host:guest — the printed `localhost:N`
    /// URL keeps working on the host.
    ports: Vec<u16>,
    secrets: Vec<SecretSpec>,
    /// Secret env names — their real values are stripped from spawn env so
    /// the proxy's placeholders are the only values the guest ever sees.
    scrub: Vec<String>,
}

struct ProcSinks {
    stdout: Sender<Vec<u8>>,
    stderr: Sender<Vec<u8>>,
    exit: Sender<i32>,
}

/// A running `shuru run --stdio` process: one VM, one stdio channel, a demux
/// thread routing responses and per-process event streams.
pub struct ShuruVm {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    procs: Mutex<HashMap<String, ProcSinks>>,
    /// Output/exit events that arrived before their pid was registered —
    /// spawn results can return after the guest already produced output.
    early: Mutex<HashMap<String, Vec<Value>>>,
    scrub: Vec<String>,
    next_id: AtomicU64,
}

impl ShuruVm {
    fn launch(config: GuestConfig) -> anyhow::Result<Arc<Self>> {
        let mut command = crate::command_env::plain_command(&config.shuru);
        command
            .arg("run")
            .arg("--stdio")
            .arg("--from")
            .arg(&config.checkpoint)
            .arg("--allow-net")
            .arg("--allow-host-writes");
        for (host, guest) in &config.mounts {
            command
                .arg("--mount")
                .arg(format!("{}:{}:rw", host.display(), guest));
        }
        for host in &config.allow_hosts {
            command.arg("--allow-host").arg(host);
        }
        for port in &config.ports {
            command.arg("-p").arg(format!("{port}:{port}"));
        }
        for secret in &config.secrets {
            command.arg("--secret").arg(format!(
                "{}={}@{}",
                secret.guest_name,
                secret.host_env,
                secret.hosts.join(",")
            ));
        }
        command
            .current_dir(&config.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = crate::command_env::spawn(&mut command)
            .context("could not start `shuru run --stdio`")?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("shuru stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("shuru stderr unavailable"))?;
        // shuru logs its own progress on stderr — forward it rather than
        // letting a full pipe stall the boot.
        thread::Builder::new()
            .name("waku-shuru-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    match line {
                        Ok(line) => eprintln!("shuru: {line}"),
                        Err(_) => break,
                    }
                }
            })
            .context("could not start the shuru stderr drain")?;

        let mut reader = BufReader::new(stdout);
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if Instant::now() > deadline {
                let _ = child.kill();
                bail!("the sandbox VM did not become ready");
            }
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    let _ = child.kill();
                    bail!("`shuru run --stdio` exited before the VM was ready");
                }
                Ok(_) => {
                    let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
                        continue;
                    };
                    if message.get("method").and_then(Value::as_str) == Some("ready") {
                        break;
                    }
                }
                Err(error) => {
                    let _ = child.kill();
                    return Err(error).context("could not read the shuru ready event");
                }
            }
        }

        let vm = Arc::new(Self {
            stdin: Mutex::new(
                child
                    .stdin
                    .take()
                    .ok_or_else(|| anyhow!("shuru stdin unavailable"))?,
            ),
            child: Mutex::new(child),
            pending: Mutex::new(HashMap::new()),
            procs: Mutex::new(HashMap::new()),
            early: Mutex::new(HashMap::new()),
            scrub: config.scrub,
            next_id: AtomicU64::new(1),
        });
        let weak = Arc::downgrade(&vm);
        thread::Builder::new()
            .name("waku-shuru-demux".into())
            .spawn(move || {
                for line in reader.lines() {
                    let Some(vm) = weak.upgrade() else {
                        return;
                    };
                    let Ok(line) = line else {
                        return;
                    };
                    vm.dispatch(line.trim());
                }
            })
            .context("could not start the shuru demux thread")?;
        Ok(vm)
    }

    fn dispatch(&self, line: &str) {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            if let Some(tx) = self.pending.lock().remove(&id) {
                let _ = tx.send(message);
            }
            return;
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return;
        };
        let Some(params) = message.get("params") else {
            return;
        };
        match method {
            "output" | "exit" => {
                let Some(pid) = params.get("pid").and_then(Value::as_str) else {
                    return;
                };
                let mut procs = self.procs.lock();
                match procs.get(pid) {
                    Some(sinks) => {
                        if method == "output" {
                            let stream = params
                                .get("stream")
                                .and_then(Value::as_str)
                                .unwrap_or("stdout");
                            let data = params
                                .get("data")
                                .and_then(Value::as_str)
                                .and_then(|data| {
                                    base64::engine::general_purpose::STANDARD.decode(data).ok()
                                })
                                .unwrap_or_default();
                            let sink = if stream == "stderr" {
                                &sinks.stderr
                            } else {
                                &sinks.stdout
                            };
                            let _ = sink.send(data);
                        } else {
                            let code =
                                params.get("code").and_then(Value::as_i64).unwrap_or(-1) as i32;
                            let _ = sinks.exit.send(code);
                            // Dropping the senders ends the readers' streams.
                            procs.remove(pid);
                        }
                    }
                    None => {
                        drop(procs);
                        self.early
                            .lock()
                            .entry(pid.to_owned())
                            .or_default()
                            .push(message.clone());
                    }
                }
            }
            _ => {}
        }
    }

    fn write(&self, message: Value) -> std::io::Result<()> {
        let mut stdin = self.stdin.lock();
        writeln!(stdin, "{message}")
    }

    /// A request that expects a response.
    fn call(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending.lock().insert(id, tx);
        let sent = self.write(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        if let Err(error) = sent {
            self.pending.lock().remove(&id);
            return Err(error).context("could not write to the sandbox VM");
        }
        let message = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(message) => message,
            Err(error) => {
                self.pending.lock().remove(&id);
                return Err(error).context("the sandbox VM stopped answering");
            }
        };
        if let Some(error) = message.get("error") {
            bail!(
                "sandbox {method} failed: {}",
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            );
        }
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }

    /// A fire-and-forget notification — `input` accepts no id.
    fn notify(&self, method: &str, params: Value) -> std::io::Result<()> {
        self.write(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    /// Run a command to completion inside the guest.
    pub fn exec(&self, argv: &[&str]) -> anyhow::Result<(String, String, i32)> {
        let result = self.call("exec", json!({ "argv": argv }))?;
        Ok((
            result
                .get("stdout")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            result
                .get("stderr")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            result
                .get("exit_code")
                .and_then(Value::as_i64)
                .unwrap_or(-1) as i32,
        ))
    }

    /// Spawn the driver-built command inside the guest.
    fn spawn(self: &Arc<Self>, command: &Command) -> anyhow::Result<DriverChild> {
        let argv: Vec<String> = std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let mut env = serde_json::Map::new();
        for (name, value) in command.get_envs() {
            let Some(value) = value else {
                continue;
            };
            let name = name.to_string_lossy();
            if env_scrubbed(&name, &self.scrub) {
                continue;
            }
            env.insert(
                name.into_owned(),
                Value::String(value.to_string_lossy().into_owned()),
            );
        }
        for (name, value) in [
            (
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/opt/mise/shims:/usr/bin:/sbin:/bin",
            ),
            ("HOME", "/root"),
            ("USER", "root"),
            ("SHELL", "/bin/sh"),
            ("TMPDIR", "/tmp"),
            // Toolchain layers install mise and its runtimes into fixed
            // prefixes — always set so a session without layers is harmless.
            ("MISE_DATA_DIR", MISE_DATA_DIR),
            ("MISE_CONFIG_DIR", MISE_CONFIG_DIR),
            ("MISE_GLOBAL_CONFIG_FILE", "/opt/mise-config/config.toml"),
        ] {
            env.insert(name.to_owned(), Value::String(value.to_owned()));
        }
        let cwd = command
            .get_current_dir()
            .map(|cwd| cwd.display().to_string());
        let result = self
            .call("spawn", json!({ "argv": argv, "env": env, "cwd": cwd }))
            .context("could not spawn the provider process in the sandbox VM")?;
        let pid = result
            .get("pid")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("shuru spawn returned no pid"))?
            .to_owned();

        let (stdout_tx, stdout_rx) = channel();
        let (stderr_tx, stderr_rx) = channel();
        let (exit_tx, exit_rx) = channel();
        self.procs.lock().insert(
            pid.clone(),
            ProcSinks {
                stdout: stdout_tx,
                stderr: stderr_tx,
                exit: exit_tx,
            },
        );
        // Flush anything the guest produced before registration.
        if let Some(events) = self.early.lock().remove(&pid) {
            for event in events {
                self.dispatch(&event.to_string());
            }
        }

        Ok(DriverChild::guest(GuestChild {
            vm: Arc::clone(self),
            pid: pid.clone(),
            exit_rx: Some(exit_rx),
            stdin: Some(GuestStdin {
                vm: Arc::clone(self),
                pid,
            }),
            stdout: Some(ChanReader::new(stdout_rx)),
            stderr: Some(ChanReader::new(stderr_rx)),
        }))
    }
}

/// Killing the `shuru` process closes the guest's vsock channel and stops the
/// VM — the whole sandbox dies with the runtime that owned it.
impl Drop for ShuruVm {
    fn drop(&mut self) {
        let _ = self.child.lock().kill();
    }
}

/// A provider process spawned inside the guest. `stdin`/`stdout`/`stderr`
/// take() and `wait()`/`kill()` mirror `std::process::Child` so drivers can
/// use either shape without branching.
struct GuestChild {
    vm: Arc<ShuruVm>,
    pid: String,
    stdin: Option<GuestStdin>,
    stdout: Option<ChanReader>,
    stderr: Option<ChanReader>,
    exit_rx: Option<Receiver<i32>>,
}

impl GuestChild {
    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let rx = self
            .exit_rx
            .take()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "already waited"))?;
        let code = rx.recv().map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "guest exit stream ended")
        })?;
        Ok(exit_status(code))
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.vm
            .call("kill", json!({ "pid": self.pid }))
            .map(|_| ())
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error.to_string()))
    }
}

impl Drop for GuestChild {
    fn drop(&mut self) {
        if self.exit_rx.is_some() {
            let _ = self.kill();
        }
    }
}

/// The spawn result drivers consume regardless of where the process lives.
pub struct DriverChild {
    pub stdin: Option<Box<dyn Write + Send>>,
    pub stdout: Option<Box<dyn Read + Send>>,
    pub stderr: Option<Box<dyn Read + Send>>,
    kind: ChildKind,
}

enum ChildKind {
    Host(Child),
    Guest(GuestChild),
}

impl DriverChild {
    fn host(mut child: Child) -> Self {
        Self {
            stdin: child.stdin.take().map(|stdin| Box::new(stdin) as _),
            stdout: child.stdout.take().map(|stdout| Box::new(stdout) as _),
            stderr: child.stderr.take().map(|stderr| Box::new(stderr) as _),
            kind: ChildKind::Host(child),
        }
    }

    fn guest(mut child: GuestChild) -> Self {
        Self {
            stdin: child.stdin.take().map(|stdin| Box::new(stdin) as _),
            stdout: child.stdout.take().map(|stdout| Box::new(stdout) as _),
            stderr: child.stderr.take().map(|stderr| Box::new(stderr) as _),
            kind: ChildKind::Guest(child),
        }
    }

    pub fn wait(&mut self) -> std::io::Result<ExitStatus> {
        match &mut self.kind {
            ChildKind::Host(child) => child.wait(),
            ChildKind::Guest(child) => child.wait(),
        }
    }

    pub fn kill(&mut self) -> std::io::Result<()> {
        match &mut self.kind {
            ChildKind::Host(child) => child.kill(),
            ChildKind::Guest(child) => child.kill(),
        }
    }

    /// A cheap handle for ending the process later — `cancel()` needs one
    /// after the child itself moved into the reader thread.
    pub fn interrupt_handle(&self) -> DriverInterrupt {
        match &self.kind {
            ChildKind::Host(child) => DriverInterrupt::Host(child.id()),
            ChildKind::Guest(child) => DriverInterrupt::Guest {
                vm: child.vm.clone(),
                pid: child.pid.clone(),
            },
        }
    }
}

/// How to end a spawned process from elsewhere — SIGINT on the host, the
/// VM's proc kill inside the guest (the guest exposes no signal path, and
/// providers with no in-stream interrupt treat either as the process
/// ending).
#[derive(Clone)]
pub enum DriverInterrupt {
    Host(u32),
    Guest { vm: Arc<ShuruVm>, pid: String },
}

impl DriverInterrupt {
    pub fn interrupt(&self) {
        match self {
            Self::Host(pid) => {
                #[cfg(unix)]
                {
                    let _ = Command::new("/bin/kill")
                        .args(["-INT", &pid.to_string()])
                        .status();
                }
                #[cfg(not(unix))]
                let _ = pid;
            }
            Self::Guest { vm, pid } => {
                let _ = vm.call("kill", json!({ "pid": pid }));
            }
        }
    }
}

/// Guest stdin rides the `input` notification — each `write` sends a frame,
/// so `flush` is a no-op.
struct GuestStdin {
    vm: Arc<ShuruVm>,
    pid: String,
}

impl Write for GuestStdin {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.vm.notify(
            "input",
            json!({
                "pid": self.pid,
                "data": base64::engine::general_purpose::STANDARD.encode(buf),
            }),
        )?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A `Read` over an mpsc stream — the demux pushes output chunks and drops
/// the sender on exit, ending the stream.
struct ChanReader {
    rx: Receiver<Vec<u8>>,
    buf: VecDeque<u8>,
}

impl ChanReader {
    fn new(rx: Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            buf: VecDeque::new(),
        }
    }
}

impl Read for ChanReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.buf.is_empty() {
            match self.rx.recv() {
                Ok(chunk) => self.buf.extend(chunk),
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len());
        for (i, byte) in self.buf.drain(..n).enumerate() {
            out[i] = byte;
        }
        Ok(n)
    }
}

/// The guest reports a bare exit code — wrap it in an `ExitStatus` so
/// `status.success()` and `status.code()` behave the same as for a host
/// process.
#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    std::os::unix::process::ExitStatusExt::from_raw(code << 8)
}

#[cfg(not(unix))]
fn exit_status(code: i32) -> ExitStatus {
    std::os::windows::process::ExitStatusExt::from_raw(code as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn env_scrub_strips_secrets_agent_and_host_only_vars() {
        let secrets = vec!["OPENAI_API_KEY".to_owned()];
        assert!(env_scrubbed("OPENAI_API_KEY", &secrets));
        assert!(env_scrubbed("GODDARD_MCP_PROXY_TOKEN", &secrets));
        assert!(env_scrubbed("GODDARD_AGENT_TOKEN", &secrets));
        assert!(env_scrubbed("SSH_AUTH_SOCK", &secrets));
        assert!(!env_scrubbed("LANG", &secrets));
        assert!(!env_scrubbed("TERM", &secrets));
        // A name that is not a configured secret survives.
        assert!(!env_scrubbed("CODEX_API_KEY", &secrets));
    }

    #[cfg(unix)]
    #[test]
    fn guest_exit_status_maps_codes() {
        assert!(exit_status(0).success());
        assert_eq!(exit_status(0).code(), Some(0));
        assert_eq!(exit_status(42).code(), Some(42));
        assert!(!exit_status(42).success());
    }

    #[test]
    fn chan_reader_streams_until_the_sender_drops() {
        let (tx, rx) = channel();
        tx.send(b"hello ".to_vec()).unwrap();
        tx.send(b"world".to_vec()).unwrap();
        drop(tx);
        let mut reader = ChanReader::new(rx);
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "hello world");
        // And again — the stream stays ended.
        assert_eq!(reader.read(&mut [0; 8]).unwrap(), 0);
    }

    #[test]
    fn detects_runtimes_from_manifests() {
        let dir = std::env::temp_dir().join(format!("waku-rt-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(detect_runtimes(&dir).is_empty());

        std::fs::write(dir.join("package.json"), "{}").unwrap();
        std::fs::write(dir.join(".nvmrc"), "v22\n").unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        std::fs::write(
            dir.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.85.0\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("go.mod"), "module x\n\ngo 1.23\n").unwrap();
        std::fs::write(dir.join(".python-version"), "3.12\n").unwrap();
        let specs = detect_runtimes(&dir);
        let tools: Vec<(&str, &str)> = specs.iter().map(|s| (s.tool, s.version.as_str())).collect();
        assert_eq!(
            tools,
            [
                ("node", "22"),
                ("python", "3.12"),
                ("rust", "1.85.0"),
                ("go", "1.23")
            ]
        );

        // .tool-versions and mise.toml pin over file-specific defaults.
        std::fs::write(dir.join(".tool-versions"), "node 20.11.0\npython 3.13\n").unwrap();
        let specs = detect_runtimes(&dir);
        let node = specs.iter().find(|s| s.tool == "node").unwrap();
        assert_eq!(node.version, "20.11.0");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mise_toml_pins_tool_versions() {
        let tools =
            parse_mise_tools("[tools]\nnode = \"lts\"\npython = \"3.13\"\n[settings]\nx = 1\n");
        assert_eq!(tools.get("node").map(String::as_str), Some("lts"));
        assert_eq!(tools.get("python").map(String::as_str), Some("3.13"));
        assert!(tools.get("x").is_none());
    }

    #[test]
    fn checkpoint_names_encode_the_layers() {
        let spec = guest_spec(ProviderKind::Codex).unwrap();
        assert_eq!(session_checkpoint(&spec, &[]), "waku-provider-codex");
        let name = session_checkpoint(
            &spec,
            &[
                RuntimeSpec {
                    tool: "node",
                    version: "lts".into(),
                },
                RuntimeSpec {
                    tool: "python",
                    version: "3.12".into(),
                },
            ],
        );
        assert_eq!(name, "waku-provider-codex-node-lts-python-3.12");
    }

    /// The protocol flag and the guest spec table answer the same question
    /// for every provider — one must never drift from the other.
    #[test]
    fn every_supported_provider_has_a_guest_spec() {
        for provider in ProviderKind::ALL {
            assert_eq!(
                guest_spec(provider).is_some(),
                provider.supports_sandbox(),
                "{provider:?}"
            );
        }
    }

    #[test]
    fn sandbox_homes_are_per_provider_under_the_data_dir() {
        let root = Path::new("/tmp/waku-data");
        assert_eq!(
            sandbox_home(ProviderKind::Devin, root),
            root.join("sandbox-homes/devin")
        );
        assert_eq!(
            sandbox_home(ProviderKind::Codex, root),
            root.join("sandbox-homes/codex")
        );
        assert_ne!(
            sandbox_home(ProviderKind::Devin, root),
            sandbox_home(ProviderKind::Claude, root),
        );
    }

    #[test]
    fn signed_in_checks_the_spec_auth_files() {
        let dir = std::env::temp_dir().join(format!("waku-auth-{}", Uuid::new_v4()));
        let spec = guest_spec(ProviderKind::Devin).unwrap();
        let home = dir.join("devin");
        assert!(!sandbox_signed_in_at(&home, &spec));
        // The credential path exists only as an empty parent — not signed in.
        std::fs::create_dir_all(home.join(".local/share/devin")).unwrap();
        assert!(!sandbox_signed_in_at(&home, &spec));
        std::fs::write(home.join(".local/share/devin/credentials.toml"), "[auth]\n").unwrap();
        assert!(sandbox_signed_in_at(&home, &spec));

        // A directory entry counts only once it has contents.
        let spec = guest_spec(ProviderKind::Amp).unwrap();
        let home = dir.join("amp");
        std::fs::create_dir_all(home.join(".config/amp")).unwrap();
        assert!(!sandbox_signed_in_at(&home, &spec));
        std::fs::write(home.join(".config/amp/auth.json"), "{}").unwrap();
        assert!(sandbox_signed_in_at(&home, &spec));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The invocation the sign-in terminal runs: a VM off the provider
    /// checkpoint, the provider home at /root, the login argv after `--`.
    #[test]
    fn sign_in_invocation_mounts_the_provider_home() {
        let dir = std::env::temp_dir().join(format!("waku-signin-{}", Uuid::new_v4()));
        // Without shuru installed this test cannot run — the argv itself is
        // what is under test, so a missing binary skips rather than fails.
        if shuru_binary().is_err() {
            return;
        }
        let (program, args, cwd) =
            sign_in_invocation(ProviderKind::Devin, &dir).expect("devin sign-in resolves");
        assert_eq!(cwd, dir.join("sandbox-homes"));
        let home = dir.join("sandbox-homes/devin");
        assert!(home.is_dir());
        assert_eq!(program, shuru_binary().unwrap(),);
        assert_eq!(args[0], "run");
        assert!(args.contains(&"--allow-host-writes".to_owned()));
        assert!(args.contains(&format!("{}:/root:rw", home.display())));
        // The guest default HOME=/ would drop credentials on the ephemeral
        // disk — sign-in env must point at the mounted provider home.
        assert!(args.contains(&"HOME=/root".to_owned()));
        assert!(args.ends_with(&[
            "/usr/local/bin/devin".to_owned(),
            "auth".to_owned(),
            "login".to_owned(),
            "--force-manual-token-flow".to_owned(),
        ]));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Real end-to-end against the installed shuru binary and the
    /// `waku-provider-codex` checkpoint: boot, exec, spawn with output,
    /// input delivery, exit code, and kill.
    #[test]
    #[ignore = "requires shuru and the waku-provider-codex checkpoint"]
    fn vm_spawn_exec_and_kill() {
        let shuru = shuru_binary().expect("shuru is not installed");
        let vm = ShuruVm::launch(GuestConfig {
            shuru,
            cwd: std::env::current_dir().unwrap(),
            checkpoint: "waku-provider-codex".to_owned(),
            mounts: vec![],
            allow_hosts: vec![],
            ports: vec![],
            secrets: vec![],
            scrub: vec![],
        })
        .expect("the VM should boot");

        // exec runs to completion and reports stdout/stderr/exit code.
        let (stdout, _stderr, code) = vm.exec(&["uname", "-m"]).expect("exec failed");
        assert_eq!(stdout.trim(), "aarch64");
        assert_eq!(code, 0);

        // spawn streams output and reports exit.
        let mut command = Command::new("/bin/sh");
        command
            .args([
                "-c",
                "echo out; echo err >&2; read line; echo got:$line; exit 7",
            ])
            .env("PATH", "/usr/bin:/bin");
        let mut child = vm.spawn(&command).expect("spawn failed");
        child.stdin.as_mut().unwrap().write_all(b"ping\n").unwrap();
        let mut out = String::new();
        child
            .stdout
            .as_mut()
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        let mut err = String::new();
        child
            .stderr
            .as_mut()
            .unwrap()
            .read_to_string(&mut err)
            .unwrap();
        assert_eq!(out, "out\ngot:ping\n");
        assert_eq!(err, "err\n");
        assert_eq!(child.wait().unwrap().code(), Some(7));

        // kill ends a live process.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 60"]);
        let mut sleeper = vm.spawn(&command).expect("spawn failed");
        sleeper.kill().unwrap();
        assert!(sleeper.wait().is_ok());
    }
}
