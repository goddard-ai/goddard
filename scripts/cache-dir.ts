// Shared build cache outside the checkout: every worktree and clone uses the
// same codesigning identity, downloaded SDKs, and compiled helpers instead of
// resolving (and caching) them per checkout. Everything under it is
// re-derivable, so it lives in the OS cache directory; WAKU_CACHE_DIR
// overrides the location (CI, sandboxed builds).
import { homedir } from "node:os";
import { join, resolve } from "node:path";

export function wakuCacheDir(): string {
  if (process.env.WAKU_CACHE_DIR) return resolve(process.env.WAKU_CACHE_DIR);
  const home = homedir();
  // Not Caches/Waku: on case-insensitive filesystems that is the app's own
  // runtime cache directory.
  if (process.platform === "darwin")
    return join(home, "Library", "Caches", "waku-build");
  if (process.platform === "win32")
    return join(
      process.env.LOCALAPPDATA ?? join(home, "AppData", "Local"),
      "waku-build",
    );
  return join(process.env.XDG_CACHE_HOME ?? join(home, ".cache"), "waku-build");
}
