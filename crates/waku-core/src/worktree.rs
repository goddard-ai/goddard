//! Daemon-owned isolated Git worktrees for tasks.
//!
//! A worktree is named and detached: `git worktree add --detach` places it in
//! `../worktrees/<repository>/<name>/<repository>` beside the repository,
//! visible to ordinary Git tooling, with no branch until the user assigns
//! one. Repeating the repository name as the checkout's leaf — the layout Zed
//! uses — keeps its basename equal to the primary checkout's, so
//! basename-derived labels (terminal directories, editor project names) show
//! the project while the worktree name namespaces it one level up.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use anyhow::{Context as _, bail};
use uuid::Uuid;

const MAX_CANDIDATES: usize = 100;

/// The adjective and noun dictionaries generated worktree names draw from:
/// distinctive, branch-safe words of at most eight letters, easy to spell,
/// paired at random into `<adjective>-<noun>` slugs like `amber-falcon`.
/// The lists stay disjoint so a generated pair never repeats a word.
const SLUG_ADJECTIVES: &[&str] = &[
    "adept", "agile", "alert", "alive", "ample", "ancient", "apt", "ashen", "astral", "avid",
    "azure", "balmy", "blithe", "bold", "boreal", "brave", "breezy", "bright", "brisk", "broad",
    "calm", "candid", "clean", "clear", "clever", "coastal", "compact", "cool", "cosmic", "cozy",
    "crisp", "curious", "dapper", "daring", "deft", "dewy", "dreamy", "dry", "durable", "dusky",
    "dusty", "eager", "early", "earnest", "earthy", "elder", "elegant", "even", "exact", "expert",
    "fabled", "fair", "fancy", "festive", "fierce", "fine", "firm", "fleet", "fluent", "foggy",
    "frank", "free", "fresh", "frosty", "gentle", "giant", "glad", "glassy", "gleaming", "glossy",
    "golden", "grand", "gritty", "gusty", "hale", "handy", "happy", "hardy", "hazy", "hearty",
    "hollow", "honest", "humble", "hushed", "icy", "ideal", "intrepid", "jolly", "jovial",
    "joyful", "just", "keen", "kind", "kinetic", "leafy", "lean", "lemon", "level", "light",
    "limber", "little", "lively", "lofty", "lone", "loyal", "lucent", "lucid", "lucky", "lunar",
    "lush", "magic", "major", "mellow", "merry", "mild", "mint", "minty", "misty", "modest",
    "molten", "mossy", "motley", "mottled", "muted", "mystic", "native", "neat", "nimble", "noble",
    "oaken", "olive", "open", "ornate", "oval", "pale", "patient", "pearly", "peppery", "pert",
    "plaid", "plucky", "plush", "poetic", "polar", "potent", "primal", "prime", "proud", "proven",
    "pure", "quaint", "quick", "quiet", "quirky", "radiant", "rainy", "rapid", "rare", "ready",
    "regal", "rich", "ripe", "robust", "rocky", "roomy", "rosy", "round", "rousing", "royal",
    "ruddy", "rugged", "russet", "rustic", "sandy", "satin", "scenic", "serene", "shaded",
    "shadowy", "sharp", "shiny", "silken", "silky", "simple", "sleek", "slender", "smart",
    "smooth", "snappy", "snowy", "snug", "soft", "solar", "solid", "spare", "spicy", "spry",
    "stalwart", "steady", "stellar", "still", "stoic", "stony", "stormy", "stout", "strong",
    "sturdy", "summer", "sunlit", "sunny", "supple", "sure", "sweet", "swift", "sylvan", "tall",
    "tangy", "tart", "tawny", "tender", "thorny", "thrifty", "tidal", "tidy", "timeless", "toasty",
    "trim", "true", "trusty", "tuneful", "twilit", "uncanny", "upbeat", "valiant", "valid", "vast",
    "verdant", "vibrant", "vintage", "vital", "vivid", "warm", "wavy", "wild", "willowy", "windy",
    "winsome", "wintry", "wise", "wispy", "wistful", "witty", "woody", "woolly", "young", "zesty",
];

const SLUG_NOUNS: &[&str] = &[
    "acacia", "acorn", "adder", "agate", "agave", "alder", "aloe", "alpaca", "amber", "anchor",
    "anchovy", "anemone", "antler", "anvil", "arch", "arrow", "aspen", "aster", "atlas", "atoll",
    "aurora", "axle", "badger", "bamboo", "banner", "basalt", "basin", "bat", "bayou", "beach",
    "beacon", "beaver", "beech", "beetle", "bell", "berry", "birch", "bison", "blizzard",
    "bluebird", "bluff", "boar", "bobcat", "bolt", "bonsai", "borax", "boulder", "bramble",
    "breeze", "briar", "brook", "buoy", "burro", "burrow", "butte", "cactus", "cairn", "camel",
    "canoe", "canopy", "canyon", "cape", "capybara", "cardinal", "caribou", "cascade", "cave",
    "cavern", "cedar", "cheetah", "chestnut", "chipmunk", "chisel", "cicada", "cinder", "cirrus",
    "citadel", "citrus", "clam", "cliff", "cloud", "clover", "coast", "cobalt", "cobra", "cocoon",
    "cod", "colt", "comet", "compass", "conch", "condor", "copper", "coral", "cosmos", "cove",
    "coyote", "crab", "crag", "crane", "crater", "creek", "crest", "cricket", "crocus", "crystal",
    "cub", "cuckoo", "curio", "cypress", "daffodil", "dahlia", "daisy", "dale", "dawn", "daybreak",
    "decoy", "deer", "dell", "delta", "dew", "diamond", "dingo", "dodo", "dolphin", "dove",
    "dowel", "dragon", "duck", "dune", "dusk", "eagle", "eclipse", "egret", "elk", "elm", "ember",
    "emerald", "emu", "equinox", "ermine", "estuary", "falcon", "fen", "fern", "ferret", "fig",
    "finch", "fir", "firefly", "flamingo", "flare", "flint", "floe", "flume", "flute", "foothill",
    "ford", "forge", "fox", "foxglove", "frog", "frond", "frost", "furrow", "gable", "galaxy",
    "gale", "garnet", "gate", "gator", "gazelle", "gecko", "gem", "geyser", "gibbon", "ginger",
    "ginkgo", "giraffe", "glacier", "glade", "glen", "glow", "glyph", "goat", "goose", "gorge",
    "gorilla", "gourd", "granite", "grouse", "grove", "halo", "hamster", "harbor", "hare", "harp",
    "hawk", "hawthorn", "haze", "hazel", "heath", "heather", "hedge", "hedgehog", "helm", "heron",
    "hickory", "hippo", "holly", "hoodoo", "horn", "hornet", "hound", "husky", "hyacinth", "hyena",
    "ibex", "ibis", "iceberg", "impala", "indigo", "inlet", "iris", "iron", "isle", "ivory", "ivy",
    "jackal", "jade", "jaguar", "jasmine", "jasper", "jay", "jet", "jewel", "juniper", "kayak",
    "keel", "kelp", "kestrel", "keystone", "kiln", "kite", "kitten", "kiwi", "koala", "koi",
    "krill", "ladybug", "lagoon", "lantern", "larch", "lark", "larkspur", "lathe", "laurel",
    "lava", "lavender", "ledge", "lemming", "lemur", "lens", "leopard", "lichen", "lilac", "lily",
    "lime", "lion", "lizard", "llama", "loam", "lodestar", "lodge", "loom", "loon", "lotus",
    "lumen", "lynx", "magnolia", "magpie", "mahogany", "mallard", "mammoth", "manatee", "mango",
    "mantis", "maple", "marble", "mare", "marigold", "marlin", "marmot", "marsh", "marten", "mast",
    "mayfly", "meadow", "meerkat", "merlin", "mesa", "meteor", "mica", "mink", "minnow", "mist",
    "mole", "mongoose", "monkey", "monsoon", "moor", "moose", "moss", "moth", "mulberry",
    "mushroom", "mustard", "myrtle", "narwhal", "nettle", "newt", "nimbus", "notch", "nova",
    "nutmeg", "oak", "oar", "oasis", "obsidian", "ocelot", "octopus", "onyx", "opal", "opossum",
    "orange", "orca", "orchard", "orchid", "oriole", "osprey", "ostrich", "otter", "owl", "oxbow",
    "paddle", "palm", "panda", "pansy", "panther", "papaya", "parrot", "peach", "peanut", "pearl",
    "pebble", "pecan", "pelican", "penguin", "pennant", "peony", "petal", "petunia", "pewter",
    "pillar", "pine", "pinnacle", "piston", "plateau", "plow", "pond", "poppy", "prairie", "prism",
    "puffin", "pulley", "puma", "python", "quadrant", "quagmire", "quail", "quarry", "quartz",
    "quasar", "quill", "quiver", "raccoon", "rainbow", "rapids", "ratchet", "raven", "redwood",
    "reed", "reef", "relay", "ridge", "riffle", "rime", "river", "rivulet", "rocket", "rook",
    "rosette", "rowan", "ruby", "rune", "sable", "saddle", "sage", "sail", "sandbar", "sapling",
    "sapphire", "sardine", "savanna", "scepter", "scroll", "scythe", "seal", "seedling", "serpent",
    "sextant", "shale", "shell", "shoal", "shore", "sigil", "skiff", "slate", "sloop", "snipe",
    "snow", "sparrow", "spire", "spring", "sprocket", "spruce", "spur", "squirrel", "starling",
    "steel", "steppe", "stone", "stork", "storm", "strait", "stratus", "summit", "talon", "tapir",
    "teak", "teal", "tern", "terrace", "thicket", "thistle", "thorn", "thresher", "thrush", "tide",
    "tiller", "timber", "toad", "topaz", "tortoise", "toucan", "tower", "trout", "trowel", "tulip",
    "tundra", "twine", "umbra", "urchin", "vale", "valley", "vault", "vellum", "velvet", "violet",
    "viper", "vista", "vixen", "volcano", "vole", "vortex", "vulture", "wallaby", "walnut",
    "walrus", "warbler", "warthog", "wasabi", "weasel", "wedge", "wetland", "whale", "wheat",
    "wheel", "willow", "wisteria", "wombat", "yak", "yew", "yoke", "yucca", "zebra", "zenith",
    "zephyr", "zinc", "zinnia", "zircon", "zither",
];

pub use waku_protocol::git::CreatedWorktree;

/// Whether `path` sits inside a linked Git worktree rather than a primary
/// checkout. A linked worktree's own `.git` lives under the main
/// repository's `worktrees/` directory, which is what separates the two
/// `--git-dir` and `--git-common-dir` answers. Non-Git paths report false.
pub fn is_linked_worktree(path: &Path) -> bool {
    let resolve = |dir: &str| {
        let dir = Path::new(dir);
        let dir = if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            path.join(dir)
        };
        fs::canonicalize(&dir).unwrap_or(dir)
    };
    let (Ok(Some(git_dir)), Ok(Some(common_dir))) = (
        git_optional_stdout(path, &["rev-parse", "--git-dir"]),
        git_optional_stdout(path, &["rev-parse", "--git-common-dir"]),
    ) else {
        return false;
    };
    resolve(&git_dir) != resolve(&common_dir)
}

/// Create a detached linked worktree named `name` — or given a generated
/// name when `name` is `None` — based on `base_ref` or the repository's
/// default branch. The returned path is project-relative, preserving a
/// project that points at a subdirectory of its repository.
pub fn create(
    project_path: &Path,
    name: Option<&str>,
    base_ref: Option<&str>,
) -> anyhow::Result<CreatedWorktree> {
    let (_, repository, project_relative) = resolve_repository(project_path)?;
    let base_ref = base_ref
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
        .map(str::to_owned)
        .map(Ok)
        .unwrap_or_else(|| default_base_ref(&repository))?;
    git_stdout(
        &repository,
        &["rev-parse", "--verify", &format!("{base_ref}^{{commit}}")],
    )
    .with_context(|| format!("base ref `{base_ref}` is unavailable"))?;
    add_named(&repository, &project_relative, name, &base_ref, None)
}

/// Create a detached linked worktree that adopts the checkout's current
/// state: based on its HEAD commit with uncommitted — including untracked —
/// files carried over, so a session moving out of the ordinary checkout keeps
/// its work in progress. The source checkout keeps its own copy; the move
/// destroys nothing. Carried files arrive unstaged, matching how the session's
/// edits look between checkpoints.
pub fn create_from_checkout(
    project_path: &Path,
    name: Option<&str>,
) -> anyhow::Result<CreatedWorktree> {
    let (_, repository, project_relative) = resolve_repository(project_path)?;
    let head = git_optional_stdout(&repository, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    // A dangling commit of the whole working state — ignored files stay out,
    // same as a turn checkpoint.
    let snapshot = crate::checkpoint::capture_worktree_commit(&repository)?;
    let (base, carried) = match head {
        Some(head) => {
            // `git diff --quiet` exits 1 on differences; a snapshot that
            // reproduces HEAD exactly needs no application.
            let dirty =
                git_optional_stdout(&repository, &["diff", "--quiet", &head, &snapshot, "--"])?
                    .is_none();
            (head, dirty.then_some(snapshot))
        }
        // An unborn branch has no commit to detach at; the snapshot, a root
        // commit of the full working state, is the base.
        None => (snapshot, None),
    };
    add_named(
        &repository,
        &project_relative,
        name,
        &base,
        carried.as_deref(),
    )
}

/// Resolve `project_path` to its canonical form, its repository root, and the
/// project's path relative to that root.
fn resolve_repository(project_path: &Path) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
    let project_path = fs::canonicalize(project_path)
        .with_context(|| format!("could not open project {}", project_path.display()))?;
    let repository = git_stdout(&project_path, &["rev-parse", "--show-toplevel"])
        .context("worktrees require a Git repository")?;
    let repository = fs::canonicalize(PathBuf::from(repository.trim()))
        .context("could not resolve the Git repository root")?;
    let project_relative = project_path
        .strip_prefix(&repository)
        .context("the project is outside its Git repository root")?
        .to_owned();
    Ok((project_path, repository, project_relative))
}

/// The name-allocation and checkout loop both creation entry points share.
/// `carried_state`, when set, is a commit whose difference from `base_ref` is
/// replayed into the new checkout as unstaged work.
fn add_named(
    repository: &Path,
    project_relative: &Path,
    name: Option<&str>,
    base_ref: &str,
    carried_state: Option<&str>,
) -> anyhow::Result<CreatedWorktree> {
    let (worktree_root, repository_name) = worktree_root(repository)?;
    fs::create_dir_all(&worktree_root).with_context(|| {
        format!(
            "could not create the worktree directory {}",
            worktree_root.display()
        )
    })?;
    let registered = registered_worktree_paths(&repository)?;

    // The name directory itself is the claim: a leftover entry must block
    // reuse whether it still holds a checkout (`<name>/<repository-name>`)
    // or was registered under the former flat layout (`<name>`).
    let taken = |name: &str| {
        let dir = worktree_root.join(name);
        dir.exists()
            || registered.contains(&dir)
            || registered.contains(&dir.join(&repository_name))
    };
    let checkout = |name: &str| worktree_root.join(name).join(&repository_name);
    let add = |path: &Path| -> anyhow::Result<()> {
        add_detached(repository, path, base_ref)?;
        if let Some(state) = carried_state {
            carry_state(path, state)?;
        }
        Ok(())
    };

    match name.and_then(sanitize_name) {
        Some(name) => {
            if taken(&name) {
                bail!("a worktree named `{name}` already exists");
            }
            let path = checkout(&name);
            add(&path)?;
            materialized(path, project_relative, name)
        }
        None => {
            // Each attempt draws a fresh random pair rather than suffixing
            // one slug, so a collision costs nothing but another roll.
            for _ in 0..MAX_CANDIDATES {
                let name = worktree_slug();
                if taken(&name) {
                    continue;
                }
                let path = checkout(&name);
                add(&path)?;
                return materialized(path, project_relative, name);
            }
            // A UUID fallback keeps the last resort independent of
            // human-readable name collisions.
            let name = format!(
                "{}-{}",
                worktree_slug(),
                &Uuid::new_v4().simple().to_string()[..8]
            );
            if taken(&name) {
                bail!("could not allocate a unique Git worktree name");
            }
            let path = checkout(&name);
            add(&path)?;
            materialized(path, project_relative, name)
        }
    }
}

/// Remove a linked worktree. `path` may be a project subdirectory inside the
/// worktree; the worktree root is what Git removes. `git worktree remove`
/// refuses to delete a worktree with modifications or untracked files, so
/// callers can invoke this on abandoned drafts without risking work. `force`
/// overrides that refusal — only for worktrees whose content is known to be
/// a discardable copy, or whose state has been captured into a ref as
/// archived-session cleanup does; never a session's own live checkout.
pub fn remove(path: &Path, force: bool) -> anyhow::Result<()> {
    let path = fs::canonicalize(path)
        .with_context(|| format!("could not open worktree {}", path.display()))?;
    let worktree_root = git_stdout(&path, &["rev-parse", "--show-toplevel"])
        .context("the worktree is not a Git repository")?;
    let worktree_root = fs::canonicalize(PathBuf::from(worktree_root.trim()))
        .context("could not resolve the Git worktree root")?;
    let common = git_stdout(
        &worktree_root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .context("the worktree is not a Git repository")?;
    let repository = PathBuf::from(common.trim())
        .parent()
        .map(Path::to_owned)
        .context("could not resolve the Git repository root")?;
    let worktree_root_arg = worktree_root.to_string_lossy().into_owned();
    let mut arguments = vec!["worktree", "remove"];
    if force {
        arguments.push("--force");
    }
    arguments.push(&worktree_root_arg);
    git_stdout(&repository, &arguments)
        .with_context(|| format!("could not remove worktree {}", worktree_root.display()))?;
    // A nested checkout leaves its name directory empty once Git removes it;
    // drop it so the name frees up, but never remove the repository's
    // `worktrees/<repository-name>` namespace itself — a former flat-layout
    // checkout's parent *is* that namespace.
    if let Some(parent) = worktree_root.parent() {
        let namespace = repository
            .parent()
            .zip(repository.file_name())
            .map(|(root, name)| root.join("worktrees").join(name));
        let is_namespace =
            namespace.is_some_and(|root| fs::canonicalize(&root).unwrap_or(root) == parent);
        if !is_namespace {
            let _ = fs::remove_dir(parent);
        }
    }
    Ok(())
}

/// Recreate the worktree containing `path` when its directory was deleted
/// outside the app.
///
/// `path` is the project directory stored on the session — possibly a
/// subdirectory inside the worktree — so the worktree root is recovered by
/// stripping the project's repository-relative suffix. `Ok(None)` means the
/// directory is already there; a failed stat counts as present, since a
/// transient filesystem error must never trigger a recreate over a healthy
/// worktree. A recreated worktree checks out `branch` when it still exists
/// and otherwise comes up detached at `base_ref` — typically the session's
/// newest checkpoint — falling back to the repository's default branch. The
/// `Some` return reports the checkout the worktree came up in.
pub fn ensure(
    project_path: &Path,
    path: &Path,
    branch: Option<&str>,
    base_ref: Option<&str>,
) -> anyhow::Result<Option<Option<String>>> {
    if path.try_exists().unwrap_or(true) {
        return Ok(None);
    }
    let project_path = fs::canonicalize(project_path)
        .with_context(|| format!("could not open project {}", project_path.display()))?;
    let repository = git_stdout(&project_path, &["rev-parse", "--show-toplevel"])
        .context("worktrees require a Git repository")?;
    let repository = fs::canonicalize(PathBuf::from(repository.trim()))
        .context("could not resolve the Git repository root")?;
    let project_relative = project_path
        .strip_prefix(&repository)
        .context("the project is outside its Git repository root")?;
    // `path` ends with the project-relative suffix; dropping those
    // components recovers the worktree root `git worktree add` targets.
    let mut worktree_path = path.to_path_buf();
    if !project_relative.as_os_str().is_empty() {
        if !worktree_path.ends_with(project_relative) {
            bail!(
                "{} is not inside a worktree of {}",
                path.display(),
                repository.display()
            );
        }
        for _ in 0..project_relative.components().count() {
            worktree_path.pop();
        }
    }
    // A directory deleted by hand leaves a registration that would make
    // `git worktree add` refuse the path.
    git_stdout(&repository, &["worktree", "prune"])
        .context("could not prune stale Git worktree registrations")?;
    let branch = branch.map(str::trim).filter(|branch| !branch.is_empty());
    // A snapshot captured by `checkpoint::capture_ref` records the checkout's
    // HEAD as its first parent. Restoring detaches at that real commit and
    // replays the snapshot's difference as uncommitted — including untracked —
    // work, so an archived session comes back with its history and dirty
    // state intact instead of a historyless commit and a clean tree.
    // Parentless `base_ref`s (plain checkpoint commits) keep their old
    // meaning: detach at the commit itself.
    let snapshot = match base_ref
        .map(str::trim)
        .filter(|reference| !reference.is_empty())
    {
        Some(reference) if ref_is_commit(&repository, reference)? => {
            let commit = git_stdout(
                &repository,
                &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
            )?;
            // `rev-list --parents` prints "<commit> <parent>..."; a root
            // commit lists itself alone, which `--verify <commit>^1` would
            // report as a hard error instead.
            let parent = git_stdout(
                &repository,
                &["rev-list", "--parents", "-n", "1", &commit],
            )?
            .split_whitespace()
            .nth(1)
            .map(str::to_owned);
            Some((commit, parent))
        }
        _ => None,
    };
    let checked_out = match branch {
        Some(branch) if local_branch_exists(&repository, branch)? => {
            add_branch(&repository, &worktree_path, branch).with_context(|| {
                format!("could not recreate worktree {}", worktree_path.display())
            })?;
            Some(branch.to_owned())
        }
        _ => {
            // Detached restore: the snapshot's recorded HEAD when there is
            // one, the snapshot itself when not, else the same default base
            // a fresh worktree gets.
            let base = match &snapshot {
                Some((commit, parent)) => parent.clone().unwrap_or_else(|| commit.clone()),
                None => default_base_ref(&repository)?,
            };
            add_detached(&repository, &worktree_path, &base).with_context(|| {
                format!("could not recreate worktree {}", worktree_path.display())
            })?;
            None
        }
    };
    if let Some((commit, Some(_))) = &snapshot {
        carry_state(&worktree_path, commit).with_context(|| {
            format!(
                "could not restore the worktree's uncommitted state in {}",
                worktree_path.display()
            )
        })?;
    }
    if !path.is_dir() {
        bail!(
            "Git recreated the worktree, but its project directory is missing: {}",
            path.display()
        );
    }
    Ok(Some(checked_out))
}

/// `<repository>/../worktrees/<repository-name>` — beside the checkout so the
/// worktrees are visible to ordinary Git tooling, namespaced by the
/// repository's directory name so sibling repositories cannot collide. Also
/// returns that name: each worktree's checkout repeats it as the leaf.
fn worktree_root(repository: &Path) -> anyhow::Result<(PathBuf, OsString)> {
    let name = repository
        .file_name()
        .context("could not name worktrees after the repository directory")?;
    let root = repository
        .parent()
        .context("the Git repository root has no parent directory")?
        .join("worktrees")
        .join(name);
    Ok((root, name.to_owned()))
}

/// The worktree paths Git already knows about. A registered entry whose
/// directory was deleted out from under it would otherwise collide only
/// inside `git worktree add`, with a worse error.
fn registered_worktree_paths(repository: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let list = git_stdout(repository, &["worktree", "list", "--porcelain"])?;
    Ok(list
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|line| {
            let path = PathBuf::from(line.trim());
            fs::canonicalize(&path).unwrap_or(path)
        })
        .collect())
}

/// Every candidate name is checked against registered worktrees by canonical
/// path, so compare the creation target in canonical form too.
fn materialized(
    path: PathBuf,
    project_relative: &Path,
    name: String,
) -> anyhow::Result<CreatedWorktree> {
    let project_path = path.join(project_relative);
    if !project_path.is_dir() {
        bail!(
            "Git created the worktree, but its project directory is missing: {}",
            project_path.display()
        );
    }
    Ok(CreatedWorktree {
        path: project_path,
        name,
    })
}

/// Replay `snapshot`'s difference from HEAD into a fresh worktree, leaving
/// every carried change unstaged. A two-tree `read-tree` merge advances the
/// index and files together — deletions included — then a mixed reset drops
/// the index back to HEAD so nothing arrives committed or staged.
fn carry_state(worktree: &Path, snapshot: &str) -> anyhow::Result<()> {
    git_stdout(worktree, &["read-tree", "-m", "-u", "HEAD", snapshot])
        .context("could not carry the checkout's changes into the worktree")?;
    git_stdout(worktree, &["reset", "--quiet"]).context("could not unstage the carried changes")?;
    Ok(())
}

fn add_detached(repository: &Path, path: &Path, base_ref: &str) -> anyhow::Result<()> {
    let output = crate::command_env::plain_command("git")
        .args(["worktree", "add", "--detach"])
        .arg(path)
        .arg(base_ref)
        .current_dir(repository)
        .output()
        .context("failed to execute git worktree add")?;
    if !output.status.success() {
        bail!("{}", command_error(&output));
    }
    Ok(())
}

/// `git worktree add <path> <branch>` checks the branch out rather than
/// detaching, restoring a worktree the user had placed on a branch. The
/// branch must be verified first: a missing name would silently create one.
fn add_branch(repository: &Path, path: &Path, branch: &str) -> anyhow::Result<()> {
    let output = crate::command_env::plain_command("git")
        .args(["worktree", "add"])
        .arg(path)
        .arg(branch)
        .current_dir(repository)
        .output()
        .context("failed to execute git worktree add")?;
    if !output.status.success() {
        bail!("{}", command_error(&output));
    }
    Ok(())
}

/// Whether `reference` resolves to a commit — `rev-parse --verify` exits
/// non-zero both for absent refs and for ambiguous ones, which both mean
/// "cannot restore from this".
fn ref_is_commit(repository: &Path, reference: &str) -> anyhow::Result<bool> {
    let output = crate::command_env::plain_command("git")
        .args(["rev-parse", "--verify", &format!("{reference}^{{commit}}")])
        .current_dir(repository)
        .output()
        .context("failed to execute git")?;
    Ok(output.status.success())
}

/// An explicit worktree name must become a single path segment — separators
/// would escape the worktree root and dot-names are not directories.
fn sanitize_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return None;
    }
    Some(name.to_owned())
}

/// Prefer the local branch named by `origin/HEAD`, so a worktree starts from
/// the default branch without fetching or mutating the user's ordinary
/// checkout. Repositories without that metadata fall back to their current
/// branch, then detached `HEAD`.
fn default_base_ref(repository: &Path) -> anyhow::Result<String> {
    if let Some(remote_default) = git_optional_stdout(
        repository,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )? {
        if let Some(local_default) = remote_default.trim().strip_prefix("origin/")
            && local_branch_exists(repository, local_default)?
        {
            return Ok(local_default.to_owned());
        }
        return Ok(remote_default.trim().to_owned());
    }
    if let Some(branch) = git_optional_stdout(repository, &["branch", "--show-current"])?
        && !branch.trim().is_empty()
    {
        return Ok(branch.trim().to_owned());
    }
    git_stdout(repository, &["rev-parse", "--verify", "HEAD"])?;
    Ok("HEAD".to_owned())
}

fn local_branch_exists(repository: &Path, branch: &str) -> anyhow::Result<bool> {
    let output = crate::command_env::plain_command("git")
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .current_dir(repository)
        .output()
        .context("failed to inspect Git branches")?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!("{}", command_error(&output)),
    }
}

fn git_stdout(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = crate::command_env::plain_command("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to execute git")?;
    if !output.status.success() {
        bail!("{}", command_error(&output));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn git_optional_stdout(cwd: &Path, args: &[&str]) -> anyhow::Result<Option<String>> {
    let output = crate::command_env::plain_command("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to execute git")?;
    if output.status.success() {
        return Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        ));
    }
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    bail!("{}", command_error(&output))
}

fn command_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("git exited with {}", output.status)
    } else {
        stderr
    }
}

/// A generated worktree name: a dictionary adjective and noun joined by a
/// hyphen. Entropy comes from a UUID, already this module's source of
/// last-resort uniqueness.
fn worktree_slug() -> String {
    let entropy = Uuid::new_v4().into_bytes();
    let adjective = SLUG_ADJECTIVES[entropy[0] as usize % SLUG_ADJECTIVES.len()];
    let noun = SLUG_NOUNS[entropy[1] as usize % SLUG_NOUNS.len()];
    format!("{adjective}-{noun}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(cwd: &Path, args: &[&str]) {
        let output = crate::command_env::plain_command("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", command_error(&output));
    }

    fn repository() -> PathBuf {
        let root = std::env::temp_dir().join(format!("waku-worktree-test-{}", Uuid::new_v4()));
        let repository = root.join("repository");
        let project = repository.join("packages/app");
        fs::create_dir_all(&project).unwrap();
        run_git(&repository, &["init", "-b", "main"]);
        run_git(&repository, &["config", "core.autocrlf", "false"]);
        fs::write(project.join("README.md"), "main\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git(
            &repository,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "initial",
            ],
        );
        run_git(
            &repository,
            &["update-ref", "refs/remotes/origin/main", "refs/heads/main"],
        );
        run_git(
            &repository,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        run_git(&repository, &["checkout", "-b", "feature"]);
        fs::write(project.join("README.md"), "feature\n").unwrap();
        run_git(&repository, &["add", "."]);
        run_git(
            &repository,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "feature",
            ],
        );
        // `create` reports canonicalized paths; match them on macOS, where
        // the temporary directory lives behind `/var` -> `/private/var`.
        fs::canonicalize(&repository).unwrap()
    }

    #[test]
    fn creates_detached_worktrees_beside_the_repository() {
        let repository = repository();
        let project = repository.join("packages/app");

        let named = create(&project, Some("My Worktree"), None).unwrap();
        assert_eq!(named.name, "My Worktree");
        assert_eq!(
            named.path,
            repository
                .parent()
                .unwrap()
                .join("worktrees/repository/My Worktree/repository/packages/app")
        );
        // Created from the default branch even though `feature` is checked
        // out, and detached: no branch owns the worktree.
        assert_eq!(
            fs::read_to_string(named.path.join("README.md")).unwrap(),
            "main\n"
        );
        let head = git_stdout(&named.path, &["branch", "--show-current"]).unwrap();
        assert!(head.is_empty());
        fs::write(named.path.join("README.md"), "worktree\n").unwrap();
        assert_eq!(
            fs::read_to_string(project.join("README.md")).unwrap(),
            "feature\n"
        );

        // An explicit name collides with the directory it just made.
        assert!(create(&project, Some("My Worktree"), None).is_err());

        // Generated names are random dictionary pairs; a rolled collision
        // retries with a fresh pair, so two creates never share a name.
        let generated = create(&project, None, Some("feature")).unwrap();
        let (first, second) = generated.name.split_once('-').unwrap();
        assert!(SLUG_ADJECTIVES.contains(&first));
        assert!(SLUG_NOUNS.contains(&second));
        assert_eq!(
            fs::read_to_string(generated.path.join("README.md")).unwrap(),
            "feature\n"
        );
        let other = create(&project, None, None).unwrap();
        assert_ne!(other.name, generated.name);

        // Removal runs in the repository and frees the name.
        remove(&named.path, false).unwrap_err();
        run_git(&named.path, &["add", "."]);
        run_git(
            &named.path,
            &[
                "-c",
                "user.name=Goddard Tests",
                "-c",
                "user.email=waku@example.com",
                "commit",
                "-m",
                "worktree",
            ],
        );
        remove(&named.path, false).unwrap();
        let recreated = create(&project, Some("My Worktree"), None).unwrap();
        assert_eq!(recreated.name, "My Worktree");

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn creates_a_worktree_from_the_checkout_state() {
        let repository = repository();
        let project = repository.join("packages/app");

        // A clean checkout moves with nothing to carry.
        let clean = create_from_checkout(&project, Some("Clean Task")).unwrap();
        assert_eq!(
            git_stdout(&clean.path, &["rev-parse", "HEAD"]).unwrap(),
            git_stdout(&repository, &["rev-parse", "HEAD"]).unwrap(),
            "the worktree is based on the checkout's HEAD, not the default branch"
        );
        assert!(
            git_stdout(&clean.path, &["status", "--porcelain"])
                .unwrap()
                .is_empty(),
            "a clean checkout produces a clean worktree"
        );
        assert!(
            git_stdout(&clean.path, &["branch", "--show-current"])
                .unwrap()
                .is_empty(),
            "and stays detached"
        );

        // Uncommitted work of every kind: modified, staged, untracked.
        fs::write(project.join("README.md"), "edits\n").unwrap();
        fs::write(project.join("staged.txt"), "staged\n").unwrap();
        run_git(&repository, &["add", "packages/app/staged.txt"]);
        fs::write(project.join("scratch.txt"), "scratch\n").unwrap();

        let moved = create_from_checkout(&project, Some("Moved Task")).unwrap();
        assert_eq!(moved.name, "Moved Task");
        assert_eq!(
            fs::read_to_string(moved.path.join("README.md")).unwrap(),
            "edits\n"
        );
        assert_eq!(
            fs::read_to_string(moved.path.join("staged.txt")).unwrap(),
            "staged\n"
        );
        assert_eq!(
            fs::read_to_string(moved.path.join("scratch.txt")).unwrap(),
            "scratch\n"
        );
        // Carried work reads as ordinary unstaged/untracked changes —
        // nothing arrives committed or staged.
        let unstaged = git_stdout(&moved.path, &["diff", "--name-only"]).unwrap();
        assert_eq!(unstaged, "packages/app/README.md");
        let staged = git_stdout(&moved.path, &["diff", "--cached", "--name-only"]).unwrap();
        assert!(staged.is_empty(), "nothing arrives staged: {staged}");
        let untracked =
            git_stdout(&moved.path, &["ls-files", "--others", "--exclude-standard"]).unwrap();
        assert!(untracked.contains("staged.txt"), "{untracked}");
        assert!(untracked.contains("scratch.txt"), "{untracked}");

        // The source checkout keeps its own copy — nothing was destroyed.
        assert_eq!(
            fs::read_to_string(project.join("README.md")).unwrap(),
            "edits\n"
        );
        assert_eq!(
            fs::read_to_string(project.join("staged.txt")).unwrap(),
            "staged\n"
        );

        // The carried worktree is dirty by design, so only a forced removal
        // takes it away.
        remove(&moved.path, false).unwrap_err();
        remove(&moved.path, true).unwrap();
        assert!(!moved.path.exists());

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn ensure_recreates_a_deleted_worktree() {
        let repository = repository();
        let project = repository.join("packages/app");
        let created = create(&project, Some("Restore Me"), Some("feature")).unwrap();
        // The stored path is the project subdirectory; the worktree root is
        // its grandparent.
        let worktree_dir = created
            .path
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .to_path_buf();

        // A directory deleted by hand keeps its registration; `ensure`
        // prunes it before re-adding or `git worktree add` would refuse.
        fs::remove_dir_all(&worktree_dir).unwrap();
        let ensured = ensure(&project, &created.path, None, Some("feature")).unwrap();
        assert_eq!(ensured, Some(None));
        assert_eq!(
            fs::read_to_string(created.path.join("README.md")).unwrap(),
            "feature\n"
        );
        assert!(
            git_stdout(&created.path, &["branch", "--show-current"])
                .unwrap()
                .is_empty(),
            "a ref-based restore stays detached"
        );

        // A present worktree is a no-op.
        assert_eq!(ensure(&project, &created.path, None, None).unwrap(), None);

        // A surviving branch is checked out rather than detached.
        run_git(&repository, &["branch", "restore-branch"]);
        fs::remove_dir_all(&worktree_dir).unwrap();
        let ensured = ensure(&project, &created.path, Some("restore-branch"), None).unwrap();
        assert_eq!(ensured, Some(Some("restore-branch".to_owned())));
        assert_eq!(
            git_stdout(&created.path, &["branch", "--show-current"]).unwrap(),
            "restore-branch"
        );

        // A deleted branch falls back to the detached base ref.
        fs::remove_dir_all(&worktree_dir).unwrap();
        run_git(&repository, &["worktree", "prune"]);
        run_git(&repository, &["branch", "-D", "restore-branch"]);
        let ensured = ensure(
            &project,
            &created.path,
            Some("restore-branch"),
            Some("main"),
        )
        .unwrap();
        assert_eq!(ensured, Some(None));
        assert_eq!(
            fs::read_to_string(created.path.join("README.md")).unwrap(),
            "main\n"
        );

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn force_removes_a_dirty_worktree() {
        let repository = repository();
        let project = repository.join("packages/app");
        let created = create(&project, Some("Dirty"), None).unwrap();
        fs::write(created.path.join("README.md"), "dirty\n").unwrap();
        fs::write(created.path.join("scratch.txt"), "untracked\n").unwrap();

        remove(&created.path, false).unwrap_err();
        remove(&created.path, true).unwrap();
        assert!(!created.path.exists());
        // The name frees up for reuse.
        let recreated = create(&project, Some("Dirty"), None).unwrap();
        assert_eq!(recreated.name, "Dirty");

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn a_removed_worktree_restores_from_its_archive_ref() {
        let repository = repository();
        let project = repository.join("packages/app");
        let created = create(&project, Some("Archived"), Some("feature")).unwrap();
        fs::write(created.path.join("README.md"), "dirty\n").unwrap();
        fs::write(created.path.join("notes.txt"), "untracked\n").unwrap();

        let git_ref = crate::checkpoint::archive_ref(Uuid::new_v4());
        crate::checkpoint::capture_ref(&created.path, &git_ref).unwrap();
        remove(&created.path, true).unwrap();
        assert!(!created.path.exists());

        let ensured = ensure(&project, &created.path, None, Some(&git_ref)).unwrap();
        assert_eq!(ensured, Some(None));
        assert_eq!(
            fs::read_to_string(created.path.join("README.md")).unwrap(),
            "dirty\n"
        );
        assert_eq!(
            fs::read_to_string(created.path.join("notes.txt")).unwrap(),
            "untracked\n"
        );

        // The restore lands on the commit the worktree had — real history,
        // not the snapshot — with the carried state left uncommitted.
        let worktree_root = created
            .path
            .ancestors()
            .find(|ancestor| ancestor.join(".git").exists())
            .unwrap()
            .to_path_buf();
        assert_eq!(
            git_stdout(&worktree_root, &["rev-parse", "HEAD"]).unwrap(),
            git_stdout(&repository, &["rev-parse", "feature"]).unwrap()
        );
        let status = git_stdout(&worktree_root, &["status", "--porcelain"]).unwrap();
        assert!(
            status.lines().any(|line| line.ends_with("README.md")),
            "the modified file stays modified: {status}"
        );
        assert!(
            status
                .lines()
                .any(|line| line.starts_with("??") && line.ends_with("notes.txt")),
            "the untracked file stays untracked: {status}"
        );
        assert_eq!(
            git_stdout(&worktree_root, &["log", "--format=%s", "HEAD"]).unwrap(),
            "feature\ninitial"
        );

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn a_removed_worktree_on_a_branch_restores_its_dirty_state() {
        let repository = repository();
        let project = repository.join("packages/app");
        let created = create(&project, Some("Branched"), None, Some("feature")).unwrap();
        // The session checked its worktree out onto a branch, then dirtied it.
        run_git(&created.path, &["checkout", "-b", "session-branch"]);
        fs::write(created.path.join("README.md"), "dirty\n").unwrap();
        fs::write(created.path.join("notes.txt"), "untracked\n").unwrap();

        let git_ref = crate::checkpoint::archive_ref(Uuid::new_v4());
        crate::checkpoint::capture_ref(&created.path, &git_ref).unwrap();
        remove(&created.path, true).unwrap();

        let ensured =
            ensure(&project, &created.path, Some("session-branch"), Some(&git_ref)).unwrap();
        assert_eq!(ensured, Some(Some("session-branch".to_owned())));
        let worktree_root = created
            .path
            .ancestors()
            .find(|ancestor| ancestor.join(".git").exists())
            .unwrap()
            .to_path_buf();
        let status = git_stdout(&worktree_root, &["status", "--porcelain"]).unwrap();
        assert!(status.lines().any(|line| line.ends_with("README.md")));
        assert!(
            status
                .lines()
                .any(|line| line.starts_with("??") && line.ends_with("notes.txt"))
        );

        fs::remove_dir_all(repository.parent().unwrap()).ok();
    }

    #[test]
    fn generated_slugs_are_branch_safe_adjective_noun_pairs() {
        for _ in 0..64 {
            let slug = worktree_slug();
            let (first, second) = slug.split_once('-').unwrap();
            assert!(SLUG_ADJECTIVES.contains(&first), "{slug}");
            assert!(SLUG_NOUNS.contains(&second), "{slug}");
            assert_ne!(first, second, "{slug}");
        }
    }

    #[test]
    fn slug_dictionaries_stay_branch_safe_sorted_and_unique() {
        for (label, words) in [("adjective", SLUG_ADJECTIVES), ("noun", SLUG_NOUNS)] {
            let mut seen = std::collections::BTreeSet::new();
            for word in words {
                assert!(!word.is_empty());
                assert!(word.len() <= 8, "{label} {word}");
                assert!(
                    word.chars().all(|c| c.is_ascii_lowercase()),
                    "{label} {word}"
                );
                assert!(seen.insert(word), "duplicate {label} {word}");
            }
            assert!(
                words.is_sorted(),
                "{label} list is kept sorted for reviewability"
            );
        }
        // The lists stay disjoint so a pair can never repeat a word.
        assert!(
            SLUG_ADJECTIVES
                .iter()
                .all(|word| !SLUG_NOUNS.contains(word))
        );
    }
}
