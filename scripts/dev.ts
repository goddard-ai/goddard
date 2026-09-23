#!/usr/bin/env bun

import { $ } from "bun";
import { bundleComputerUse } from "./cua-driver";
import { startDevServe, type DevServe } from "./dev-serve";
import {
  mkdirSync,
  readFileSync,
  watch,
  writeFileSync,
  type FSWatcher,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import readline from "node:readline";
import { WakuClient } from "../packages/waku-client/src/client";

const root = resolve(import.meta.dir, "..");
const isMacOS = process.platform === "darwin";
// --serve: the watcher builds a signed release bundle and publishes it as the
// Dev channel's appcast (see dev-serve.ts) instead of a debug app.
const serveMode = Bun.argv.slice(2).includes("--serve");
if (serveMode && !isMacOS) {
  console.error("[goddard-dev] --serve requires macOS.");
  process.exit(2);
}
const profile = serveMode ? "release" : "debug";
const appName = serveMode ? "Goddard" : "Goddard Debug";
const targetDir = resolve(root, process.env.CARGO_TARGET_DIR || "target");
const executableSuffix = process.platform === "win32" ? ".exe" : "";
const appPath = isMacOS
  ? join(targetDir, `${profile}/${appName}.app`)
  : join(targetDir, `debug/goddard${executableSuffix}`);
const daemonPath = join(
  targetDir,
  `debug/goddard-debug-daemon${executableSuffix}`,
);
const appExecutablePath = isMacOS
  ? join(appPath, "Contents/MacOS", appName)
  : appPath;
// The app's "auto-restart" command palette toggle lands here; the app only
// offers it when the watcher hands it this path.
const devStatePath = join(targetDir, "debug", "goddard-dev.json");
// The daemon token is stable across watcher restarts so mobile and external
// clients keep working: GODDARD_DAEMON_TOKEN wins, then a persisted token
// file, then a fresh random one.
const daemonTokenPath = join(targetDir, "debug", "goddard-daemon-token");
// Written next to the token so scripts (and humans) can read the current
// daemon address + token after the watcher log has scrolled away.
const daemonInfoPath = join(targetDir, "debug", "goddard-daemon.json");
const externalDaemonAddress = process.env.GODDARD_DAEMON_ADDRESS;
// Bind host for the spawned daemon. Default loopback; set to 0.0.0.0 (or a
// Tailscale/LAN address) to make the dev daemon reachable from a phone.
const daemonBindHost = process.env.GODDARD_DAEMON_BIND ?? "127.0.0.1";
const daemonBindIsLoopback = ["127.0.0.1", "localhost", "::1"].includes(
  daemonBindHost,
);
const interactive = process.stdin.isTTY === true;

function resolveDaemonToken(): string {
  if (process.env.GODDARD_DAEMON_TOKEN)
    return process.env.GODDARD_DAEMON_TOKEN;
  try {
    const existing = readFileSync(daemonTokenPath, "utf8").trim();
    if (existing) return existing;
  } catch {
    // No persisted token yet; generate and persist one below.
  }
  const generated = crypto.randomUUID().replaceAll("-", "");
  try {
    mkdirSync(dirname(daemonTokenPath), { recursive: true });
    writeFileSync(daemonTokenPath, `${generated}\n`, { mode: 0o600 });
  } catch {
    // A transient target dir problem only means the token stays per-run.
  }
  return generated;
}

const daemonToken = resolveDaemonToken();
const stdoutIsTTY = process.stdout.isTTY === true;

// Bun's `$` pipes child stdio even when it echoes to a TTY parent, so cargo's
// `term.color = auto` would strip colors. Advertise the TTY explicitly.
if (stdoutIsTTY) {
  process.env.CARGO_TERM_COLOR ??= "always";
}

const paint =
  (open: string, close: string) =>
  (text: string): string =>
    stdoutIsTTY ? `${open}${text}${close}` : text;
const bold = paint("\x1b[1m", "\x1b[22m");
const dim = paint("\x1b[2m", "\x1b[22m");
const green = paint("\x1b[32m", "\x1b[39m");
const daemonPortBase = 34_123;
const daemonPortScanLimit = 20;
const daemonReadyTimeoutMs = 15_000;
const daemonShutdownTimeoutMs = 1_000;
const daemonIdlePollMs = 2_000;
const liveSessionStatuses = new Set([
  "connecting",
  "working",
  "waiting",
  "background",
]);
const watchedDirectories = [
  "src",
  "crates",
  "assets",
  "resources",
  "locales",
  "scripts",
];
const watchedFiles = ["Cargo.toml", "Cargo.lock", "build.rs"];
const rebuildDebounceMs = 1_000;
type BuildTarget = "app" | "daemon";
type HyprlandWorkspace = {
  id: number;
  name: string;
  selector: string;
};
type HyprlandContext = {
  workspace: HyprlandWorkspace;
  anchorSelector?: string;
};

$.cwd(root);

let app: ReturnType<typeof Bun.spawn> | undefined;
let daemon: ReturnType<typeof Bun.spawn> | undefined;
let serve: DevServe | undefined;
let daemonBind: string | undefined;
let daemonAddress: string | undefined;
let daemonRestartPending = false;
let daemonRestartWhenIdle = false;
let daemonRestarting = false;
let protocolDirty = false;
let forceDaemonRestart = false;
let relaunchAfterBuild = false;
let daemonBuildDirty = false;
let controlClient: WakuClient | undefined;
let lastDeferredLiveCount: number | undefined;
let lastDaemonSpawnAt = 0;
let commandInput: readline.Interface | undefined;
let stopping = false;
let building = false;
// Build output stays collapsed behind the progress bar. 'e' toggles live
// expansion during a build and replays the last build's output afterward;
// failures always dump everything.
let buildLogExpanded = false;
let liveBuildLog: { expand(): void } | undefined;
let lastBuildLog: { label: string; lines: string[] } | undefined;
let queuedBuild: BuildTarget | undefined;
let debouncedBuild: BuildTarget | undefined;
let appChangeRevision = 0;
let daemonChangeRevision = 0;
let rebuildTimer: ReturnType<typeof setTimeout> | undefined;
const watchers: FSWatcher[] = [];
const hyprlandRuleKeys = [
  "goddard_dev_workspace_rule",
  "goddard_dev_background_rule",
] as const;
const hyprlandSubscriptionKey = "goddard_dev_window_open_subscription";
const hyprlandLaunchArmedKey = "goddard_dev_launch_armed";
const hyprlandOwnerKey = "goddard_dev_owner";
let hyprlandRulesInstalled = false;
let hyprlandWarningShown = false;

function luaString(value: string): string {
  const bytes = new TextEncoder().encode(value);
  const escaped = Array.from(
    bytes,
    (byte) => `\\${byte.toString().padStart(3, "0")}`,
  ).join("");
  return `"${escaped}"`;
}

async function activeHyprlandContext(): Promise<HyprlandContext | undefined> {
  if (
    process.platform !== "linux" ||
    process.env.HYPRLAND_INSTANCE_SIGNATURE === undefined
  ) {
    return undefined;
  }

  const [workspaceResult, windowResult] = await Promise.all([
    $`hyprctl -j activeworkspace`.quiet().nothrow(),
    $`hyprctl -j activewindow`.quiet().nothrow(),
  ]);
  if (workspaceResult.exitCode !== 0) return undefined;

  try {
    const workspace = JSON.parse(workspaceResult.stdout.toString()) as {
      id?: unknown;
      name?: unknown;
    };
    if (
      typeof workspace.id !== "number" ||
      !Number.isInteger(workspace.id) ||
      typeof workspace.name !== "string" ||
      workspace.name.length === 0
    ) {
      return undefined;
    }
    const context: HyprlandContext = {
      workspace: {
        id: workspace.id,
        name: workspace.name,
        selector:
          workspace.id > 0 ? workspace.id.toString() : `name:${workspace.name}`,
      },
    };

    if (windowResult.exitCode === 0) {
      try {
        const window = JSON.parse(windowResult.stdout.toString()) as {
          stableId?: unknown;
          workspace?: { id?: unknown; name?: unknown };
        };
        if (
          typeof window.stableId === "string" &&
          /^[0-9a-f]+$/i.test(window.stableId) &&
          window.workspace?.id === workspace.id &&
          window.workspace.name === workspace.name
        ) {
          context.anchorSelector = `stableid:${window.stableId.toLowerCase()}`;
        }
      } catch {
        // The workspace rule still works when there is no usable anchor.
      }
    }

    return context;
  } catch {
    return undefined;
  }
}

// Hyprland normally maps a new window onto whichever workspace is active and,
// in the scrolling layout, inserts it after the focused window. Remember the
// watcher terminal as well as its workspace so background rebuilds can retain
// both the destination and the neighboring column.
const hyprlandContext = await activeHyprlandContext();

async function prepareHyprlandLaunch(): Promise<void> {
  if (hyprlandContext === undefined) return;

  const { workspace: hyprlandWorkspace, anchorSelector } = hyprlandContext;
  const [workspaceRuleKey, backgroundRuleKey] = hyprlandRuleKeys;
  const isAnotherWorkspaceActive =
    hyprlandWorkspace.id > 0
      ? `active == nil or active.id ~= ${hyprlandWorkspace.id}`
      : `active == nil or active.name ~= ${luaString(hyprlandWorkspace.name)}`;
  const code = `
    local workspace_key = ${luaString(workspaceRuleKey)}
    local background_key = ${luaString(backgroundRuleKey)}
    local subscription_key = ${luaString(hyprlandSubscriptionKey)}
    local armed_key = ${luaString(hyprlandLaunchArmedKey)}
    local owner_key = ${luaString(hyprlandOwnerKey)}
    local owner = ${process.pid}

    if _G[owner_key] ~= owner then
      if _G[workspace_key] ~= nil then
        _G[workspace_key]:set_enabled(false)
      end
      if _G[background_key] ~= nil then
        _G[background_key]:set_enabled(false)
      end
      if _G[subscription_key] ~= nil then
        _G[subscription_key]:remove()
      end
      _G[workspace_key] = nil
      _G[background_key] = nil
      _G[subscription_key] = nil
      _G[owner_key] = owner
    end

    if _G[workspace_key] == nil then
      _G[workspace_key] = hl.window_rule({
        name = "goddard-dev-workspace",
        match = { initial_class = "org[.]goddardai[.]app[.]debug" },
        workspace = ${luaString(`${hyprlandWorkspace.selector} silent`)},
      })
    end
    if _G[background_key] == nil then
      _G[background_key] = hl.window_rule({
        name = "goddard-dev-background",
        match = { initial_class = "org[.]goddardai[.]app[.]debug" },
        no_initial_focus = true,
        suppress_event = "activate activatefocus",
      })
    end
    ${
      anchorSelector === undefined
        ? ""
        : `
    if _G[subscription_key] == nil then
      local anchor_selector = ${luaString(anchorSelector)}
      _G[subscription_key] = hl.on("window.open", function(window)
        if not _G[armed_key] or window.initial_class ~= "org.goddardai.app.debug" then
          return
        end
        _G[armed_key] = false

        local anchor = hl.get_window(anchor_selector)
        if anchor == nil or anchor.workspace == nil or window.workspace ~= anchor.workspace then
          return
        end

        local anchor_layout = anchor.layout
        local window_layout = window.layout
        if anchor_layout == nil or window_layout == nil or
            anchor_layout.name ~= "scrolling" or window_layout.name ~= "scrolling" or
            anchor_layout.column == nil or window_layout.column == nil then
          return
        end

        local desired_index = anchor_layout.column.index + 1
        local current_index = window_layout.column.index
        if current_index <= desired_index or #window_layout.column.windows ~= 1 then
          return
        end

        -- Swapping with each preceding singleton column rotates Goddard into the
        -- desired slot while preserving the order of all intervening columns.
        -- A stacked or custom-width column cannot be rotated through this API
        -- without changing its membership or sizing, so leave it untouched.
        local columns = {}
        for _, candidate in ipairs(hl.get_workspace_windows(anchor.workspace)) do
          local layout = candidate.layout
          local column = layout ~= nil and layout.name == "scrolling" and layout.column or nil
          if column ~= nil and column.index >= desired_index and column.index < current_index then
            if #column.windows ~= 1 or math.abs(column.width - window_layout.column.width) > 0.0001 then
              return
            end
            columns[column.index] = column.windows[1]
          end
        end
        for index = desired_index, current_index - 1 do
          if columns[index] == nil then
            return
          end
        end

        -- Hyprland's swap action warps the pointer to its source window. Hold
        -- mouse focus steady and restore the exact pointer position afterward.
        local cursor = hl.get_cursor_pos()
        local follow_mouse = hl.get_config("input.follow_mouse")
        if cursor == nil or type(follow_mouse) ~= "number" then
          return
        end

        hl.config({ input = { follow_mouse = 0 } })
        pcall(function()
          for index = current_index - 1, desired_index, -1 do
            hl.dispatch(hl.dsp.window.swap({ window = window, target = columns[index] }))
          end
        end)
        hl.dispatch(hl.dsp.cursor.move({ x = cursor.x, y = cursor.y }))
        hl.config({ input = { follow_mouse = follow_mouse } })
      end)
    end
    `
    }
    local active = hl.get_active_workspace()
    _G[background_key]:set_enabled(${isAnotherWorkspaceActive})
    _G[armed_key] = true
  `;
  const result = await $`hyprctl eval ${code}`.quiet().nothrow();
  if (result.exitCode !== 0) {
    if (!hyprlandWarningShown) {
      const detail =
        result.stderr.toString().trim() || result.stdout.toString().trim();
      console.warn(
        `[goddard-dev] Could not pin Goddard to its Hyprland workspace${detail ? `: ${detail}` : "."}`,
      );
      hyprlandWarningShown = true;
    }
    return;
  }

  if (!hyprlandRulesInstalled) {
    console.log(
      `[goddard-dev] Keeping Goddard beside the watcher on Hyprland workspace ${hyprlandWorkspace.name}.`,
    );
  }
  hyprlandRulesInstalled = true;
}

async function releaseHyprlandRules(): Promise<void> {
  if (!hyprlandRulesInstalled) return;
  hyprlandRulesInstalled = false;
  const code = `
    local owner_key = ${luaString(hyprlandOwnerKey)}
    if _G[owner_key] == ${process.pid} then
      for _, key in ipairs({ ${hyprlandRuleKeys.map(luaString).join(", ")} }) do
        if _G[key] ~= nil then
          _G[key]:set_enabled(false)
          _G[key] = nil
        end
      end
      local subscription_key = ${luaString(hyprlandSubscriptionKey)}
      if _G[subscription_key] ~= nil then
        _G[subscription_key]:remove()
        _G[subscription_key] = nil
      end
      _G[${luaString(hyprlandLaunchArmedKey)}] = false
      _G[owner_key] = nil
    end
  `;
  await $`hyprctl eval ${code}`.quiet().nothrow();
}

// Cargo reports units only as they finish — never a total — so the bar's
// denominator is the previous build's dirty-unit count for the same label,
// persisted across runs. Tiny or unknown counts render an animated
// indeterminate bar instead of a fake percentage.
const buildStatsPath = join(targetDir, "debug", "goddard-build-stats.json");
const progressBarWidth = 16;

function readBuildStats(): Record<string, number> {
  try {
    return JSON.parse(readFileSync(buildStatsPath, "utf8")) as Record<
      string,
      number
    >;
  } catch {
    return {};
  }
}

function recordBuildStats(label: string, dirtyUnits: number): void {
  try {
    const stats = readBuildStats();
    stats[label] = dirtyUnits;
    mkdirSync(dirname(buildStatsPath), { recursive: true });
    writeFileSync(buildStatsPath, `${JSON.stringify(stats)}\n`);
  } catch {
    // The stats file only refines the bar; a failed write changes nothing.
  }
}

function elapsedLabel(since: number): string {
  const seconds = Math.floor((Date.now() - since) / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function progressLine(
  label: string,
  done: number,
  expected: number | undefined,
  crate: string,
  startedAt: number,
  warnings: number,
  errors: number,
): string {
  let bar: string;
  let count: string;
  if (expected !== undefined && expected > 2) {
    const fraction = Math.min(done / expected, 0.99);
    const filled = Math.round(fraction * progressBarWidth);
    bar =
      "█".repeat(filled) + "░".repeat(progressBarWidth - filled);
    count = `${done}/${expected}`;
  } else {
    const block = 5;
    const head =
      Math.floor((Date.now() - startedAt) / 90) %
      (progressBarWidth + block);
    bar = Array.from({ length: progressBarWidth }, (_, index) =>
      index <= head && index > head - block ? "█" : "░",
    ).join("");
    count = `${done}`;
  }
  const detail = crate ? ` · ${crate}` : "";
  const issues = [
    errors > 0 ? `${errors} error${errors === 1 ? "" : "s"}` : "",
    warnings > 0 ? `${warnings} warning${warnings === 1 ? "" : "s"}` : "",
  ]
    .filter(Boolean)
    .join(", ");
  const hint = issues ? " · press e + enter to expand" : "";
  return `[goddard-dev] Compiling ${label} ${bar} ${count} crate${done === 1 ? "" : "s"}${detail}${issues ? ` · ${issues}${hint}` : ""} · ${elapsedLabel(startedAt)}`;
}

// Runs cargo with a live one-line progress bar on a TTY by parsing the
// compiler's JSON stream. Diagnostics stay buffered behind the bar — the
// line reports warning/error counts — and 'e' expands them live or replays
// them afterward. A failed build always dumps its full output. Off a TTY
// cargo's own output passes straight through.
async function cargoBuild(label: string, args: string[]): Promise<boolean> {
  if (!stdoutIsTTY) {
    console.log(`[goddard-dev] Building ${label}...`);
    const result = await $`cargo build ${args}`.nothrow();
    return result.exitCode === 0;
  }

  const expected = readBuildStats()[label];
  const child = Bun.spawn(
    [
      "cargo",
      "build",
      "--message-format=json-diagnostic-rendered-ansi",
      ...args,
    ],
    {
      cwd: root,
      env: { ...process.env, CARGO_TERM_COLOR: "always" },
      stdout: "pipe",
      stderr: "inherit",
    },
  );
  const startedAt = Date.now();
  let done = 0;
  let crate = "";
  let warnings = 0;
  let errors = 0;
  const log: string[] = [];
  let printed = 0;
  const draw = () =>
    process.stdout.write(
      `\r\x1b[K${progressLine(label, done, expected, crate, startedAt, warnings, errors)}`,
    );
  const emit = (text: string) => {
    const entry = text.endsWith("\n") ? text : `${text}\n`;
    log.push(entry);
    if (buildLogExpanded) {
      process.stdout.write(`\r\x1b[K${entry}`);
      printed += 1;
      draw();
    }
  };
  liveBuildLog = {
    expand() {
      while (printed < log.length) {
        process.stdout.write(`\r\x1b[K${log[printed]}`);
        printed += 1;
      }
      draw();
    },
  };
  const ticker = setInterval(draw, 100);
  draw();
  try {
    const stdout = child.stdout;
    if (stdout !== null && typeof stdout !== "number") {
      const reader = stdout.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      for (;;) {
        const chunk = await reader.read();
        if (chunk.done) break;
        buffer += decoder.decode(chunk.value, { stream: true });
        let newline = buffer.indexOf("\n");
        while (newline !== -1) {
          const line = buffer.slice(0, newline);
          buffer = buffer.slice(newline + 1);
          newline = buffer.indexOf("\n");
          let parsed: {
            reason?: string;
            fresh?: boolean;
            target?: { name?: string };
            message?: {
              rendered?: string | null;
              message?: string;
              level?: string;
            };
            text?: string;
          };
          try {
            parsed = JSON.parse(line);
          } catch {
            emit(line);
            continue;
          }
          if (parsed.reason === "compiler-artifact") {
            if (parsed.fresh === false) {
              done += 1;
              crate = parsed.target?.name ?? crate;
              draw();
            }
          } else if (parsed.reason === "build-script-executed") {
            done += 1;
          } else if (parsed.reason === "compiler-message") {
            const rendered =
              parsed.message?.rendered ?? parsed.message?.message;
            if (parsed.message?.level === "warning") warnings += 1;
            if (
              parsed.message?.level === "error" ||
              parsed.message?.level === "error: internal compiler error"
            ) {
              errors += 1;
            }
            if (rendered) emit(rendered);
          } else if (parsed.reason === "text" && parsed.text) {
            emit(parsed.text);
          }
        }
      }
    }
  } finally {
    clearInterval(ticker);
    liveBuildLog = undefined;
    buildLogExpanded = false;
  }
  const exitCode = await child.exited;
  process.stdout.write("\r\x1b[K");
  lastBuildLog = { label, lines: log };
  if (exitCode !== 0) {
    while (printed < log.length) {
      process.stdout.write(log[printed]);
      printed += 1;
    }
    return false;
  }
  recordBuildStats(label, done);
  const issues = [
    errors > 0 ? `${errors} error${errors === 1 ? "" : "s"}` : "",
    warnings > 0 ? `${warnings} warning${warnings === 1 ? "" : "s"}` : "",
  ]
    .filter(Boolean)
    .join(", ");
  console.log(
    `[goddard-dev] ${label === "app" ? "App" : "Daemon"} compiled in ${elapsedLabel(startedAt)}${done === 0 ? " (all fresh)" : ""}${issues ? ` with ${issues} — press e + enter to show them` : ""}.`,
  );
  return true;
}

async function build(target: BuildTarget): Promise<boolean> {
  if (target === "daemon") {
    return buildDaemon();
  }

  console.log(`[goddard-dev] Building ${isMacOS ? "app bundle" : "app"}...`);
  if (!(await buildDaemon())) {
    console.error(
      "[goddard-dev] Daemon build failed; keeping the current app open.",
    );
    return false;
  }
  const appArgs = isMacOS
    ? [
        ...(serveMode ? ["--release"] : []),
        "--package",
        "waku",
        "--bin",
        "goddard",
        "--bin",
        "goddard_js_repl",
        // The release bundle embeds its own daemon next to the executable.
        ...(serveMode
          ? ["--package", "waku-daemon", "--bin", "goddard-daemon"]
          : []),
        "--package",
        "waku-agent",
        "--bin",
        "goddard-agent",
      ]
    : [
        "--package",
        "waku",
        "--bin",
        "goddard",
        "--bin",
        "goddard_js_repl",
        "--package",
        "waku-computer-use",
        "--bin",
        "goddard_computer_use",
        "--package",
        "waku-agent",
        "--bin",
        "goddard-agent",
      ];
  if (!(await cargoBuild("app", appArgs))) {
    console.error("[goddard-dev] Build failed; keeping the current app open.");
    return false;
  }
  if (isMacOS) {
    // The watcher already ran cargo itself so it could draw progress;
    // bundle.sh only packages and signs the binaries it just produced.
    const result =
      await $`env GODDARD_SKIP_CARGO_BUILD=1 ${join(root, "scripts/bundle.sh")} ${profile}`.nothrow();
    if (result.exitCode !== 0) {
      console.error(
        "[goddard-dev] Bundle failed; keeping the current app open.",
      );
      return false;
    }
    if (serveMode && serve !== undefined && !(await serve.deploy(appPath))) {
      console.error("[goddard-dev] Deploy failed; the update feed is stale.");
    }
  } else {
    try {
      await bundleComputerUse(
        join(targetDir, "debug"),
        join(targetDir, "debug", "resources"),
        "debug",
      );
    } catch (error) {
      console.error("[goddard-dev] Computer Use SDK packaging failed:", error);
      return false;
    }
  }
  return true;
}

async function buildDaemon(): Promise<boolean> {
  if (
    !(await cargoBuild("daemon", [
      "--package",
      "waku-daemon",
      "--features",
      "dev-binary",
      "--bin",
      "goddard-debug-daemon",
      "--package",
      "waku-agent",
      "--bin",
      "goddard-agent",
    ]))
  ) {
    console.error(
      "[goddard-dev] Daemon build failed; keeping the current daemon running.",
    );
    return false;
  }
  daemonBuildDirty = true;
  return true;
}

// The watcher owns the daemon and passes `--parent-pid` pointing at itself, so
// the daemon outlives every app relaunch but still dies with the watcher. The
// app connects over the socket like a remote client, which is what lets it
// re-attach to live sessions and reconnect on its own after a daemon restart.
type DaemonReady = { address: string };

function writeDaemonInfo(): void {
  if (daemonAddress === undefined) return;
  try {
    mkdirSync(dirname(daemonInfoPath), { recursive: true });
    writeFileSync(
      daemonInfoPath,
      `${JSON.stringify({ address: daemonAddress, token: daemonToken }, null, 2)}\n`,
      { mode: 0o600 },
    );
  } catch {
    // The info file is a convenience; a failed write changes nothing.
  }
}

async function spawnDaemon(bind: string): Promise<void> {
  const command = [
    daemonPath,
    "--bind",
    bind,
    "--parent-pid",
    String(process.pid),
  ];
  if (!daemonBindIsLoopback) command.push("--allow-non-loopback");
  const child = Bun.spawn(command, {
      cwd: root,
      env: {
        ...process.env,
        GODDARD_DAEMON_TOKEN: daemonToken,
        GODDARD_APP_EXECUTABLE: appExecutablePath,
      },
      stdout: "pipe",
      stderr: "inherit",
    });
  let ready: DaemonReady;
  try {
    ready = await readDaemonReady(child);
  } catch (error) {
    child.kill();
    throw error;
  }
  daemon = child;
  daemonBind = bind;
  daemonAddress = ready.address;
  writeDaemonInfo();
  // A freshly spawned daemon runs the just-built binary, so its protocol is
  // never behind the sources on disk.
  protocolDirty = false;
  controlClient = undefined;
  lastDaemonSpawnAt = Date.now();
  void watchDaemonExit(child);
}

// The app connects as a remote client, so nothing else notices a dead daemon.
// A daemon that survived a while is restarted immediately; one that died fast
// is left for a manual 'd' so a crashing build cannot loop.
function watchDaemonExit(child: ReturnType<typeof Bun.spawn>): void {
  void child.exited.then(async (code) => {
    if (daemon !== child || stopping) return;
    daemon = undefined;
    console.error(`[goddard-dev] Daemon exited unexpectedly (${code}).`);
    if (Date.now() - lastDaemonSpawnAt > 30_000) {
      await restartDaemon("unexpected exit");
    } else {
      daemonRestartPending = true;
      console.log("[goddard-dev] Press d + enter to restart the daemon.");
    }
  });
}

// The daemon announces its bound address as one JSON line on stdout. Keep
// draining the stream afterward so a chatty daemon can never block on a full
// pipe.
function readDaemonReady(
  child: ReturnType<typeof Bun.spawn>,
): Promise<DaemonReady> {
  return new Promise((resolveReady, rejectReady) => {
    let settled = false;
    const timer = setTimeout(() => {
      settled = true;
      rejectReady(new Error("timed out waiting for the daemon to start"));
    }, daemonReadyTimeoutMs);
    void child.exited.then((code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      rejectReady(
        new Error(`Goddard daemon exited before becoming ready (${code})`),
      );
    });
    void (async () => {
      const stdout = child.stdout;
      if (stdout === null || typeof stdout === "number") {
        settled = true;
        clearTimeout(timer);
        rejectReady(new Error("daemon stdout was not piped"));
        return;
      }
      const reader = stdout.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      try {
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          if (settled) continue;
          buffer += decoder.decode(value, { stream: true });
          const newline = buffer.indexOf("\n");
          if (newline === -1) continue;
          settled = true;
          clearTimeout(timer);
          resolveReady(JSON.parse(buffer.slice(0, newline)) as DaemonReady);
        }
      } catch (error) {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        rejectReady(error instanceof Error ? error : new Error(String(error)));
      }
    })();
  });
}

// A pre-set GODDARD_DAEMON_ADDRESS keeps working: the watcher adopts that daemon
// instead of spawning its own, and never restarts it.
async function ensureDaemon(): Promise<void> {
  if (externalDaemonAddress) {
    if (process.env.GODDARD_DAEMON_TOKEN === undefined) {
      console.warn(
        "[goddard-dev] GODDARD_DAEMON_TOKEN is unset; the external daemon will likely reject the app.",
      );
    }
    daemonAddress = externalDaemonAddress;
    writeDaemonInfo();
    console.log(
      `[goddard-dev] Using external daemon at ${externalDaemonAddress}; the watcher will not restart it.`,
    );
    return;
  }
  let lastError: unknown;
  for (let offset = 0; offset < daemonPortScanLimit; offset++) {
    try {
      await spawnDaemon(`${daemonBindHost}:${daemonPortBase + offset}`);
      console.log(
        `[goddard-dev] Daemon listening on ${daemonAddress}; it stays up across app relaunches.`,
      );
      return;
    } catch (error) {
      lastError = error;
      // The port was taken or the daemon refused the bind; try the next one.
    }
  }
  throw new Error("could not bind the Goddard daemon to a loopback port", {
    cause: lastError,
  });
}

async function stopDaemon(): Promise<void> {
  const child = daemon;
  daemon = undefined;
  if (child === undefined || child.exitCode !== null) return;
  try {
    if (controlClient?.connected) controlClient.shutdownDaemon();
  } catch {
    // Fall through to signal-based shutdown.
  }
  const finished = await Promise.race([
    child.exited.then(() => true),
    Bun.sleep(daemonShutdownTimeoutMs).then(() => false),
  ]);
  if (!finished) child.kill();
  await child.exited.catch(() => {});
  controlClient?.disconnect();
  controlClient = undefined;
}

// A live session is one with a provider runtime a daemon restart would kill.
// Persisted statuses are stale after any daemon restart, so sessions that look
// live are confirmed through AttachSession — it observes the actor without
// mutating it and returns a null runtimeId when no process is running.
async function liveSessionCount(): Promise<number | undefined> {
  if (daemonAddress === undefined) return undefined;
  try {
    controlClient ??= new WakuClient({
      address: daemonAddress,
      token: daemonToken,
      requestTimeoutMs: 5_000,
    });
    if (!controlClient.connected) await controlClient.connect();
    const response = await controlClient.request(
      { type: "loadTaskState" },
      undefined,
      undefined,
      { timeoutMs: 5_000 },
    );
    if (response.type !== "taskState") return undefined;
    const candidates = response.sessions.filter((session) =>
      liveSessionStatuses.has(session.status),
    );
    let live = 0;
    for (const session of candidates.slice(0, 50)) {
      const attach = await controlClient.request(
        { type: "attachSession" },
        session.id,
      );
      if (attach.type === "sessionRuntime" && attach.runtimeId !== null) {
        live += 1;
      }
    }
    return live;
  } catch {
    return undefined;
  }
}

async function restartDaemon(reason: string): Promise<void> {
  if (daemonRestarting || stopping) return;
  if (daemonBind === undefined) {
    console.log(
      "[goddard-dev] The daemon is externally managed; restart it yourself.",
    );
    daemonRestartPending = false;
    return;
  }
  daemonRestarting = true;
  try {
    console.log(
      `[goddard-dev] Restarting the daemon (${reason}); the app will reconnect on its own.`,
    );
    await stopDaemon();
    if (stopping) return;
    try {
      await spawnDaemon(daemonBind);
      daemonRestartPending = false;
      daemonRestartWhenIdle = false;
      console.log(`[goddard-dev] Daemon restarted on ${daemonAddress}.`);
    } catch (error) {
      console.error("[goddard-dev] Daemon restart failed:", error);
      daemonRestartPending = true;
    }
  } finally {
    daemonRestarting = false;
  }
}

function armDaemonRestartWhenIdle(): void {
  if (!daemonRestartPending) {
    console.log("[goddard-dev] No pending daemon restart.");
    return;
  }
  if (daemonRestartWhenIdle) return;
  daemonRestartWhenIdle = true;
  lastDeferredLiveCount = undefined;
  console.log(
    "[goddard-dev] Daemon restart armed; it fires once no live sessions remain.",
  );
  void pollForDaemonIdle();
}

async function pollForDaemonIdle(): Promise<void> {
  while (daemonRestartWhenIdle && daemonRestartPending && !stopping) {
    if (!building && !daemonRestarting) {
      const live = await liveSessionCount();
      if (live === 0) {
        await restartDaemon("sessions idle");
        return;
      }
      if (live !== undefined && live !== lastDeferredLiveCount) {
        console.log(
          `[goddard-dev] ${live} live session${live === 1 ? "" : "s"} still running.`,
        );
        lastDeferredLiveCount = live;
      }
    }
    await Bun.sleep(daemonIdlePollMs);
  }
  daemonRestartWhenIdle = false;
}

async function commandStdout(command: string[]): Promise<string | undefined> {
  const result = await $`${command}`.quiet().nothrow();
  if (result.exitCode !== 0) return undefined;
  const output = result.stdout.toString().trim();
  return output || undefined;
}

// Everything the mobile app needs to reach this daemon: the ws:// address to
// paste into a daemon profile, plus the token. Loopback daemons are reachable
// over USB via adb reverse; Tailscale needs GODDARD_DAEMON_BIND=0.0.0.0.
async function printMobileInfo(): Promise<void> {
  if (daemonAddress === undefined) {
    console.log("[goddard-dev] The daemon is not running yet.");
    return;
  }
  const port = daemonAddress.split(":").pop();
  const lines = [`  ${green("➜")}  ${dim("token")}   ${bold(daemonToken)}`];
  if (daemonBindIsLoopback && externalDaemonAddress === undefined) {
    if (port !== undefined) {
      const adb = await $`adb reverse ${`tcp:${port}`} ${`tcp:${port}`}`
        .quiet()
        .nothrow();
      lines.push(
        adb.exitCode === 0
          ? `  ${green("➜")}  ${dim("usb")}     ws://127.0.0.1:${port} ${dim("(adb reverse set up — connect the phone over USB)")}`
          : `  ${green("➜")}  ${dim("usb")}     ws://127.0.0.1:${port} ${dim("(run `adb reverse tcp:" + port + " tcp:" + port + "` first)")}`,
      );
    }
    lines.push(
      `  ${dim("tailscale/lan")}  restart the watcher with GODDARD_DAEMON_BIND=0.0.0.0`,
    );
  } else {
    const tailscaleIp = await commandStdout(["tailscale", "ip", "-4"]);
    const lanIp = isMacOS
      ? await commandStdout(["ipconfig", "getifaddr", "en0"])
      : (await commandStdout(["hostname", "-I"]))?.split(" ")[0];
    if (tailscaleIp !== undefined) {
      lines.push(
        `  ${green("➜")}  ${dim("tailscale")} ws://${tailscaleIp}:${port}`,
      );
    }
    if (lanIp !== undefined) {
      lines.push(`  ${green("➜")}  ${dim("lan")}      ws://${lanIp}:${port}`);
    }
    if (tailscaleIp === undefined && lanIp === undefined) {
      lines.push(
        `  ${green("➜")}  ${dim("address")}  ws://<this machine's ip>:${port}`,
      );
    }
  }
  console.log(`\n  ${bold("mobile daemon profile")}\n${lines.join("\n")}\n`);
}

function shortcutLine(key: string, description: string): string {
  return `  ${green("➜")}  ${dim("press")} ${bold(key)} ${dim(`+ enter to ${description}`)}`;
}

function printShortcuts(): void {
  console.log(
    [
      "",
      `  ${dim("Shortcuts")}`,
      shortcutLine("a", "relaunch the app"),
      shortcutLine("b", "restart the daemon, then relaunch the app"),
      shortcutLine("d", "restart the daemon now"),
      shortcutLine("D", "restart the daemon once sessions go idle"),
      shortcutLine("m", "show mobile daemon address and token"),
      shortcutLine("e", "expand build output (toggle live, replay after)"),
      shortcutLine("q", "quit the watcher, app, and daemon"),
      shortcutLine("h", "show this help"),
      "",
    ].join("\n"),
  );
}

function printBanner(): void {
  const daemonDetail =
    externalDaemonAddress === undefined
      ? "survives app relaunches and quits"
      : "external — not restarted by the watcher";
  console.log(
    `\n  ${bold("goddard dev")} ${dim("— watching for changes")}\n\n` +
      `  ${green("➜")}  ${dim("app")}     ${appName}${isMacOS ? ".app" : ""}\n` +
      `  ${green("➜")}  ${dim("daemon")}  ${daemonAddress} ${dim(`(${daemonDetail})`)}` +
      (serve === undefined
        ? ""
        : `\n  ${green("➜")}  ${dim("feed")}    ${serve.appcastUrl} ${dim(`(serving ${serve.updatesDir})`)}`),
  );
}

async function handleCommand(command: string): Promise<void> {
  switch (command) {
    case "":
      return;
    case "d":
      if (building) {
        console.log(
          "[goddard-dev] A build is in progress; try again when it finishes.",
        );
        return;
      }
      await restartDaemon("requested");
      return;
    case "D":
      armDaemonRestartWhenIdle();
      return;
    case "m":
      await printMobileInfo();
      return;
    case "e":
      if (liveBuildLog !== undefined) {
        buildLogExpanded = !buildLogExpanded;
        if (buildLogExpanded) liveBuildLog.expand();
        return;
      }
      if (lastBuildLog === undefined || lastBuildLog.lines.length === 0) {
        console.log("[goddard-dev] No build output to show.");
        return;
      }
      console.log(
        `[goddard-dev] Output from the last ${lastBuildLog.label} build:`,
      );
      process.stdout.write(lastBuildLog.lines.join(""));
      return;
    case "a":
      if (protocolDirty) {
        console.log(
          "[goddard-dev] The protocol changed; press b + enter to restart the daemon before relaunching.",
        );
        return;
      }
      if (
        building ||
        queuedBuild !== undefined ||
        debouncedBuild !== undefined
      ) {
        relaunchAfterBuild = true;
        console.log(
          "[goddard-dev] Will relaunch the app once the current build finishes.",
        );
        return;
      }
      await relaunchApp();
      return;
    case "b":
      if (
        building ||
        queuedBuild !== undefined ||
        debouncedBuild !== undefined
      ) {
        forceDaemonRestart = true;
        relaunchAfterBuild = true;
        console.log(
          "[goddard-dev] Will restart the daemon and relaunch the app once the current build finishes.",
        );
        return;
      }
      await restartDaemon("requested");
      if (!stopping) await relaunchApp();
      return;
    case "q":
      await cleanup();
      return;
    case "h":
      printShortcuts();
      return;
    default:
      console.log(
        "[goddard-dev] Unknown command — press h + enter to show shortcuts.",
      );
  }
}

function startCommandLoop(): void {
  if (!interactive) return;
  commandInput = readline.createInterface({ input: process.stdin });
  commandInput.on("line", (line) => void handleCommand(line.trim()));
}

function closeCommandLoop(): void {
  commandInput?.close();
  commandInput = undefined;
}

// Read on each build so a palette toggle mid-build takes effect immediately.
function autoRestartEnabled(): boolean {
  try {
    const state = JSON.parse(readFileSync(devStatePath, "utf8")) as {
      auto_restart?: unknown;
    };
    return state.auto_restart === true;
  } catch {
    return false;
  }
}

async function stopApp(): Promise<void> {
  const waiter = app;
  app = undefined;
  if (isMacOS) {
    // SIGTERM never reaches the app's quit hooks, and they are what flush UI
    // state to disk for the next launch. Ask for a graceful quit first and
    // only fall back to the kill if the app hangs. The `is running` guard
    // matters: a bare `tell application ... to quit` would launch it. Naming
    // the bundle by path keeps other worktrees' "Goddard Debug" instances
    // from being quit — a bare name hits whichever copy LaunchServices picks.
    await $`osascript -e 'if application "${appPath}" is running then tell application "${appPath}" to quit'`
      .quiet()
      .nothrow();
    if (waiter?.exitCode === null) {
      const exited = await Promise.race([
        waiter.exited.then(() => true),
        Bun.sleep(3_000).then(() => false),
      ]);
      if (!exited) {
        await $`pkill -TERM -f ${appExecutablePath}`.quiet().nothrow();
      }
    }
  } else if (waiter?.exitCode === null) {
    waiter.kill("SIGTERM");
  }
  if (waiter?.exitCode === null) {
    await waiter.exited;
  }
}

function launchApp(): ReturnType<typeof Bun.spawn> | undefined {
  if (daemonAddress === undefined) {
    console.error("[goddard-dev] The daemon is not running; cannot launch the app.");
    return undefined;
  }
  console.log(`[goddard-dev] Launching ${appPath}`);
  const command = isMacOS ? ["open", "-n", "-W", appPath] : [appPath];
  const launchedApp = Bun.spawn(command, {
    cwd: root,
    env: {
      ...process.env,
      GODDARD_DAEMON_PATH: daemonPath,
      GODDARD_DAEMON_ADDRESS: daemonAddress,
      GODDARD_DAEMON_TOKEN: daemonToken,
      // Marks this launch as watcher-owned; the app writes its auto-restart
      // toggle to this file and the watcher reads it after each build.
      GODDARD_DEV_STATE: devStatePath,
    },
    stdout: "inherit",
    stderr: "inherit",
  });
  void launchedApp.exited.then((exitCode) => {
    if (stopping || app !== launchedApp) return;
    app = undefined;
    // The daemon owns session state, so it and the watcher stay up when the
    // app exits; 'a' relaunches, 'q' shuts everything down.
    console.log(
      `[goddard-dev] App exited (${exitCode}); the daemon is still running — press a + enter to relaunch.`,
    );
  });
  return launchedApp;
}

async function relaunchApp(): Promise<void> {
  relaunchAfterBuild = false;
  await stopApp();
  if (stopping) return;
  await prepareHyprlandLaunch();
  if (!stopping) app = launchApp();
}

function clearRebuildTimer(): void {
  if (rebuildTimer === undefined) return;
  clearTimeout(rebuildTimer);
  rebuildTimer = undefined;
}

function closeWatchers(): void {
  for (const watcher of watchers.splice(0)) watcher.close();
}

function reportWatcherError(error: Error): void {
  console.error("[goddard-dev] File watcher failed:", error);
  process.exitCode = 1;
  void cleanup();
}

function mergedTarget(
  current: BuildTarget | undefined,
  next: BuildTarget,
): BuildTarget {
  return current === "app" || next === "app" ? "app" : "daemon";
}

function targetForChange(
  directory: string,
  filename: string | Buffer | null,
): BuildTarget {
  if (directory !== "crates" || filename === null) return "app";
  const relativePath = filename.toString().replaceAll("\\", "/");
  if (
    relativePath.startsWith("waku-daemon/") ||
    relativePath.startsWith("waku-agent/") ||
    relativePath.startsWith("waku-core/")
  ) {
    return "daemon";
  }
  // The wire protocol is shared by both sides, so the running daemon must be
  // replaced before the rebuilt app launches or the handshake mismatches.
  if (relativePath.startsWith("waku-protocol/")) protocolDirty = true;
  return "app";
}

function scheduleBuild(target: BuildTarget): void {
  if (stopping) return;
  daemonChangeRevision += 1;
  if (target === "app") appChangeRevision += 1;
  debouncedBuild = mergedTarget(debouncedBuild, target);
  clearRebuildTimer();
  rebuildTimer = setTimeout(() => {
    rebuildTimer = undefined;
    if (debouncedBuild !== undefined) {
      queuedBuild = mergedTarget(queuedBuild, debouncedBuild);
      debouncedBuild = undefined;
    }
    void drainBuildQueue();
  }, rebuildDebounceMs);
}

function startWatchers(): void {
  for (const directory of watchedDirectories) {
    const watcher = watch(
      join(root, directory),
      { recursive: true },
      (_eventType, filename) =>
        scheduleBuild(targetForChange(directory, filename)),
    );
    watcher.on("error", reportWatcherError);
    watchers.push(watcher);
  }

  const rootWatcher = watch(root, (_eventType, filename) => {
    if (filename && watchedFiles.includes(filename.toString()))
      scheduleBuild("app");
  });
  rootWatcher.on("error", reportWatcherError);
  watchers.push(rootWatcher);

  // The app's auto-restart palette toggle lands in this file; echo flips so
  // the terminal shows the same state the palette does.
  let lastAutoRestart: boolean | undefined;
  try {
    const devStateWatcher = watch(
      dirname(devStatePath),
      (_eventType, filename) => {
        if (filename?.toString() !== basename(devStatePath)) return;
        const enabled = autoRestartEnabled();
        if (enabled === lastAutoRestart) return;
        lastAutoRestart = enabled;
        console.log(
          enabled
            ? "[goddard-dev] Auto-restart enabled — the app relaunches after each successful build."
            : "[goddard-dev] Auto-restart disabled — press a + enter to relaunch.",
        );
      },
    );
    devStateWatcher.on("error", reportWatcherError);
    watchers.push(devStateWatcher);
  } catch {
    // target/debug does not exist until the first build; the toggle simply
    // stays quiet until the app's own write creates the file there.
  }
}

async function drainBuildQueue(): Promise<void> {
  if (building || stopping) return;
  building = true;
  try {
    while (queuedBuild !== undefined && !stopping) {
      const target = queuedBuild;
      queuedBuild = undefined;
      const buildAppRevision = appChangeRevision;
      const buildDaemonRevision = daemonChangeRevision;
      if (!(await build(target)) || stopping) continue;
      const daemonRebuilt = daemonBuildDirty;
      daemonBuildDirty = false;

      if (target === "daemon") {
        if (daemonChangeRevision === buildDaemonRevision) {
          daemonRestartPending = true;
          const live = await liveSessionCount();
          const detail =
            live === undefined
              ? ""
              : live === 0
                ? "; no live sessions"
                : `; ${live} live session${live === 1 ? "" : "s"} would be interrupted`;
          console.log(
            `[goddard-dev] Daemon rebuilt${detail} — press d + enter to restart, D + enter once sessions go idle.`,
          );
          // Non-interactive runs cannot press 'd'; keep the previous
          // rebuild-and-swap behavior so the daemon never goes stale.
          if (!interactive) {
            await restartDaemon("non-interactive rebuild");
          } else if (forceDaemonRestart) {
            await restartDaemon("requested");
          } else if (daemonRestartWhenIdle && live === 0) {
            await restartDaemon("sessions idle");
          }
          forceDaemonRestart = false;
          if (relaunchAfterBuild && !stopping) await relaunchApp();
        }
        continue;
      }

      // App changes make a bundle compiled from an older revision stale. A
      // daemon-only edit does not: the daemon survives the relaunch, so
      // sessions keep running while the window reloads.
      if (appChangeRevision !== buildAppRevision) {
        console.log(
          "[goddard-dev] More changes arrived during the build; waiting to rebuild.",
        );
        continue;
      }

      // Non-interactive runs have nobody to press 'a'/'b'; keep the previous
      // rebuild-and-relaunch behavior so neither side goes stale.
      if (!interactive) {
        if (forceDaemonRestart || protocolDirty) {
          await restartDaemon(
            forceDaemonRestart ? "requested" : "protocol changed",
          );
          forceDaemonRestart = false;
        }
        await relaunchApp();
        continue;
      }

      // The daemon survives app relaunches, so a finished build only needs a
      // hint. A protocol change is the exception: the rebuilt app cannot
      // safely attach to the stale daemon, so 'b' must restart it first —
      // even auto-restart defers, since that restart would kill live sessions.
      if (protocolDirty) {
        daemonRestartPending = true;
        console.log(
          "[goddard-dev] App rebuilt, but the protocol changed — press b + enter to restart the daemon and relaunch.",
        );
      } else if (autoRestartEnabled() && app !== undefined) {
        // The app's "auto-restart" palette toggle is on and it is still
        // running; an app the user quit stays quit.
        console.log(
          daemonRebuilt
            ? "[goddard-dev] App rebuilt — auto-restarting (the daemon also rebuilt; press b + enter to restart it)."
            : "[goddard-dev] App rebuilt — auto-restarting.",
        );
        await relaunchApp();
      } else {
        console.log(
          daemonRebuilt
            ? "[goddard-dev] App and daemon rebuilt — press b + enter to restart both, a + enter to relaunch the app only."
            : "[goddard-dev] App rebuilt — press a + enter to relaunch.",
        );
      }

      if (relaunchAfterBuild) {
        relaunchAfterBuild = false;
        if (forceDaemonRestart) {
          await restartDaemon("requested");
          if (!stopping) await relaunchApp();
        } else if (!protocolDirty) {
          await relaunchApp();
        }
      }
      forceDaemonRestart = false;
    }
  } finally {
    building = false;
    if (queuedBuild !== undefined && !stopping) void drainBuildQueue();
  }
}

async function cleanup(): Promise<void> {
  if (stopping) return;
  stopping = true;
  console.log("[goddard-dev] Stopping watcher, app, and daemon...");
  closeWatchers();
  clearRebuildTimer();
  closeCommandLoop();
  await stopApp();
  await stopDaemon();
  await serve?.stop();
  await releaseHyprlandRules();
}

process.on("SIGINT", () => void cleanup());
process.on("SIGTERM", () => void cleanup());

startWatchers();
if (serveMode) {
  try {
    serve = await startDevServe({ root, targetDir });
  } catch (error) {
    console.error("[goddard-dev] Could not start the update feed:", error);
    closeWatchers();
    process.exit(1);
  }
}
building = true;
const initialAppRevision = appChangeRevision;
const initialBuildSucceeded = await build("app");
daemonBuildDirty = false;
building = false;
if (!initialBuildSucceeded) {
  closeWatchers();
  process.exit(1);
}

try {
  await ensureDaemon();
} catch (error) {
  console.error("[goddard-dev]", error);
  closeWatchers();
  process.exit(1);
}

if (appChangeRevision === initialAppRevision) {
  await relaunchApp();
} else {
  console.log(
    "[goddard-dev] Changes arrived during the initial build; waiting to rebuild.",
  );
  if (queuedBuild !== undefined) void drainBuildQueue();
}

startCommandLoop();
printBanner();
if (interactive) printShortcuts();
