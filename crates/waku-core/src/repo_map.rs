//! A token-budgeted structural map of a workspace.
//!
//! The daemon keeps one [`RepoMapIndex`] per workspace directory, refreshes it
//! incrementally on a background thread, and renders a bounded text map that is
//! prepended to the first prompt of a new agent session — so providers skip
//! re-exploring cold repositories.
//!
//! The map is deterministic tree-sitter extraction only: no model, no network,
//! nothing is written into the indexed repository.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use tree_sitter::{Language, Node, Parser};

/// Default rendered-map budget in estimated tokens (chars / 4).
pub const DEFAULT_TOKEN_BUDGET: usize = 1_024;
/// Files larger than this are skipped — usually generated or minified.
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Cap on indexed files per workspace.
const MAX_INDEXED_FILES: usize = 5_000;
/// Cap on member signatures listed under one container (class, impl, …).
const MAX_MEMBERS_PER_CONTAINER: usize = 10;
/// Cap on signature lines rendered per file — breadth (files covered) beats
/// depth (symbols per file) at map budgets.
const MAX_SYMBOLS_PER_FILE: usize = 8;

/// A rendered project map plus the statistics the UI reports.
#[derive(Debug, Clone, Default)]
pub struct ProjectMap {
    /// `path:` headers plus indented signature lines. No wrapper text — the
    /// caller decides how to frame it for the provider.
    pub text: String,
    /// Supported source files found under the root.
    pub indexed_files: usize,
    /// Files the rendered map covers.
    pub mapped_files: usize,
    /// Symbol-bearing files that did not fit the budget.
    pub omitted_files: usize,
    /// `text.len() / 4`.
    pub estimated_tokens: usize,
}

/// An incremental index of a workspace's top-level symbols and import edges.
///
/// Cheap to refresh: unchanged files are detected by content hash and not
/// re-parsed.
pub struct RepoMapIndex {
    root: PathBuf,
    files: BTreeMap<PathBuf, IndexedFile>,
    /// Resolved import edges: importer → targets, recomputed on each refresh.
    edges: HashMap<PathBuf, BTreeSet<PathBuf>>,
}

struct IndexedFile {
    hash: u64,
    language: usize,
    /// Signature lines: `(indent_level, text)`.
    symbols: Vec<(usize, String)>,
    /// Raw import specifiers extracted from the source, resolved against the
    /// index after each refresh.
    import_specs: Vec<String>,
}

struct LangSpec {
    language: Language,
    /// Node kinds treated as definitions wherever they appear at a collected
    /// level (top level, or one level inside a container).
    def_kinds: &'static [&'static str],
    /// Definition kinds whose bodies are descended into for member signatures.
    container_kinds: &'static [&'static str],
    /// Node kinds whose text is scanned for import specifiers (top level only).
    import_kinds: &'static [&'static str],
    /// Wrapper kinds descended into without consuming a level (export
    /// statements, decorators).
    wrapper_kinds: &'static [&'static str],
}

fn language_specs() -> Vec<LangSpec> {
    vec![
        LangSpec {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            def_kinds: &[
                "function_declaration",
                "generator_function_declaration",
                "class_declaration",
                "abstract_class_declaration",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "lexical_declaration",
                "variable_declaration",
                "method_definition",
                "method_signature",
                "property_signature",
                "public_field_definition",
                "enum_assignment",
            ],
            container_kinds: &[
                "class_declaration",
                "abstract_class_declaration",
                "interface_declaration",
                "enum_declaration",
            ],
            import_kinds: &["import_statement"],
            wrapper_kinds: &["export_statement"],
        },
        LangSpec {
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            def_kinds: &[
                "function_declaration",
                "generator_function_declaration",
                "class_declaration",
                "abstract_class_declaration",
                "interface_declaration",
                "type_alias_declaration",
                "enum_declaration",
                "lexical_declaration",
                "variable_declaration",
                "method_definition",
                "method_signature",
                "property_signature",
                "public_field_definition",
                "enum_assignment",
            ],
            container_kinds: &[
                "class_declaration",
                "abstract_class_declaration",
                "interface_declaration",
                "enum_declaration",
            ],
            import_kinds: &["import_statement"],
            wrapper_kinds: &["export_statement"],
        },
        LangSpec {
            language: tree_sitter_javascript::LANGUAGE.into(),
            def_kinds: &[
                "function_declaration",
                "generator_function_declaration",
                "class_declaration",
                "lexical_declaration",
                "variable_declaration",
                "method_definition",
                "field_definition",
            ],
            container_kinds: &["class_declaration"],
            import_kinds: &["import_statement"],
            wrapper_kinds: &["export_statement"],
        },
        LangSpec {
            language: tree_sitter_python::LANGUAGE.into(),
            def_kinds: &[
                "function_definition",
                "class_definition",
                "assignment",
            ],
            container_kinds: &["class_definition"],
            import_kinds: &["import_statement", "import_from_statement"],
            wrapper_kinds: &["decorated_definition"],
        },
        LangSpec {
            language: tree_sitter_rust::LANGUAGE.into(),
            def_kinds: &[
                "function_item",
                "function_signature_item",
                "struct_item",
                "enum_item",
                "union_item",
                "trait_item",
                "impl_item",
                "mod_item",
                "type_item",
                "const_item",
                "static_item",
                "macro_definition",
                "enum_variant",
            ],
            container_kinds: &["impl_item", "trait_item", "enum_item"],
            // `mod foo;` is an edge to foo.rs; `use …` paths are resolved too.
            import_kinds: &["mod_item", "use_declaration"],
            wrapper_kinds: &[],
        },
        LangSpec {
            language: tree_sitter_go::LANGUAGE.into(),
            def_kinds: &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
            ],
            container_kinds: &[],
            import_kinds: &["import_declaration"],
            wrapper_kinds: &[],
        },
        LangSpec {
            language: tree_sitter_swift::LANGUAGE.into(),
            def_kinds: &[
                "function_declaration",
                "class_declaration",
                "struct_declaration",
                "enum_declaration",
                "protocol_declaration",
                "extension_declaration",
                "typealias_declaration",
                "init_declaration",
            ],
            container_kinds: &[
                "class_declaration",
                "struct_declaration",
                "enum_declaration",
                "protocol_declaration",
                "extension_declaration",
            ],
            import_kinds: &["import_declaration"],
            wrapper_kinds: &[],
        },
    ]
}

fn spec_for_extension(extension: &str) -> Option<usize> {
    Some(match extension {
        "ts" | "mts" | "cts" => 0,
        "tsx" => 1,
        "js" | "jsx" | "mjs" | "cjs" => 2,
        "py" | "pyi" => 3,
        "rs" => 4,
        "go" => 5,
        "swift" => 6,
        _ => return None,
    })
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    // FNV-1a — content hash only, not cryptographic.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim_end().to_owned()
}

/// Node kinds that hold a container's members — class bodies, impl blocks,
/// enum variant lists, Python `block`s.
const CONTAINER_BODY_KINDS: &[&str] = &[
    "class_body",
    "interface_body",
    "enum_body",
    "enum_variant_list",
    "declaration_list",
    "object_type",
    "protocol_body",
    "block",
];

fn emit_signature(node: Node, source: &[u8], depth: usize, out: &mut Vec<(usize, String)>) {
    let Ok(text) = node.utf8_text(source) else { return };
    let signature = first_line(text);
    if !signature.is_empty() {
        out.push((depth, signature));
    }
}

fn collect_members(node: Node, source: &[u8], spec: &LangSpec, out: &mut Vec<(usize, String)>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let kind = child.kind();
        if spec.def_kinds.contains(&kind) {
            emit_signature(child, source, 1, out);
        } else if CONTAINER_BODY_KINDS.contains(&kind) || spec.wrapper_kinds.contains(&kind) {
            collect_members(child, source, spec, out);
        }
    }
}

/// The first descendant (DFS) that is itself a definition — the node a
/// wrapper's prefix belongs to, e.g. the declaration inside `export …`.
fn first_def_descendant<'tree>(node: Node<'tree>, spec: &LangSpec) -> Option<Node<'tree>> {
    if spec.def_kinds.contains(&node.kind()) {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| first_def_descendant(child, spec))
}

fn collect_defs(
    node: Node,
    source: &[u8],
    spec: &LangSpec,
    prefix: Option<String>,
    out: &mut Vec<(usize, String)>,
) {
    let mut pending = prefix;
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let kind = child.kind();
        if spec.wrapper_kinds.contains(&kind) {
            // Carry the wrapper's own prefix (`export `, `export default `)
            // down to the declaration it wraps so the signature keeps it.
            let inner_prefix = first_def_descendant(child, spec).and_then(|inner| {
                let offset = inner.start_byte().saturating_sub(child.start_byte());
                let text = child.utf8_text(source).unwrap_or("");
                Some(
                    text.get(..offset)
                        .unwrap_or("")
                        .rsplit('\n')
                        .next()
                        .unwrap_or("")
                        .to_owned(),
                )
            });
            collect_defs(child, source, spec, inner_prefix.or_else(|| pending.take()), out);
            continue;
        }
        if !spec.def_kinds.contains(&kind) {
            continue;
        }
        let signature = match pending.take() {
            Some(prefix) => format!("{prefix}{}", first_line(child.utf8_text(source).unwrap_or(""))),
            None => first_line(child.utf8_text(source).unwrap_or("")),
        };
        if signature.is_empty() || signature.starts_with("mod tests") {
            continue;
        }
        out.push((0, signature));
        if spec.container_kinds.contains(&kind) {
            let mut members = Vec::new();
            collect_members(child, source, spec, &mut members);
            let total = members.len();
            out.extend(members.into_iter().take(MAX_MEMBERS_PER_CONTAINER));
            if total > MAX_MEMBERS_PER_CONTAINER {
                out.push((1, format!("… +{} more", total - MAX_MEMBERS_PER_CONTAINER)));
            }
        }
    }
}

/// Pull import specifier strings out of an import-ish node.
///
/// Looks for string literals first (JS/Go), then dotted/relative module names
/// (Python), then path-ish identifiers (Rust `use`/`mod`).
fn collect_import_specs(node: Node, source: &[u8], out: &mut Vec<String>) {
    const STRING_KINDS: &[&str] = &[
        "string",
        "interpreted_string_literal",
        "raw_string_literal",
    ];
    const NAME_KINDS: &[&str] = &[
        "dotted_name",
        "relative_import",
        "scoped_identifier",
        "identifier",
    ];

    let mut strings = Vec::new();
    let mut names = Vec::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        let kind = current.kind();
        if STRING_KINDS.contains(&kind) {
            if let Ok(text) = current.utf8_text(source) {
                strings.push(text.trim_matches(|c| c == '"' || c == '\'').to_owned());
            }
            continue;
        }
        if NAME_KINDS.contains(&kind) {
            if let Ok(text) = current.utf8_text(source) {
                names.push(text.to_owned());
            }
            continue;
        }
        let mut cursor = current.walk();
        stack.extend(current.named_children(&mut cursor));
    }
    // A `use a::{b, c}` node contains many identifiers; only the outermost
    // scoped_identifier is the path. Keep the first name match per node.
    out.extend(strings);
    if let Some(name) = names.into_iter().next() {
        out.push(name);
    }
}

/// Resolve one raw import specifier to candidate workspace-relative paths.
fn resolve_import(file: &Path, spec: &str) -> Vec<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new(""));
    let mut candidates = Vec::new();
    let mut push_variants = |base: PathBuf| {
        candidates.push(base.clone());
        for ext in ["ts", "tsx", "js", "jsx", "mts", "cts", "py", "rs", "go", "swift"] {
            candidates.push(base.with_extension(ext));
        }
        for index in ["index.ts", "index.tsx", "index.js", "mod.rs", "__init__.py"] {
            candidates.push(base.join(index));
        }
    };

    if let Some(rest) = spec.strip_prefix("crate::") {
        // Rust: crate::a::b → src/a.rs | src/a/b.rs | src/a/b/mod.rs | …
        let segments: Vec<&str> = rest.split("::").filter(|s| !s.is_empty()).collect();
        for take in (1..=segments.len()).rev() {
            push_variants(Path::new("src").join(segments[..take].join("/")));
        }
    } else if let Some(rest) = spec.strip_prefix("self::").or_else(|| spec.strip_prefix("super::")) {
        let base = if spec.starts_with("super::") {
            dir.parent().unwrap_or(dir).to_path_buf()
        } else {
            dir.to_path_buf()
        };
        let segments: Vec<&str> = rest.split("::").filter(|s| !s.is_empty()).collect();
        for take in (1..=segments.len()).rev() {
            push_variants(base.join(segments[..take].join("/")));
        }
    } else if spec.starts_with('.') {
        // Relative: JS `./x` / `../x`, Python `.x` / `..pkg.x`. Every dot past
        // the first climbs one directory; `/` and `.` both separate segments.
        let dots = spec.chars().take_while(|c| *c == '.').count();
        let rest = spec.trim_start_matches(|c| c == '.' || c == '/');
        let mut base = dir.to_path_buf();
        for _ in 1..dots {
            if let Some(parent) = base.parent() {
                base = parent.to_path_buf();
            }
        }
        let rel = rest.replace('.', "/");
        if rel.is_empty() {
            push_variants(base);
        } else {
            push_variants(base.join(rel));
        }
    } else if spec.contains("::") {
        // Rust `mod foo` resolved earlier? Bare `use a::b` without a prefix —
        // external crate, skip. (mod items arrive as bare names below.)
    } else if spec.contains('.') && !spec.contains('/') {
        // Python absolute `a.b.c` — try root-relative.
        push_variants(PathBuf::from(spec.replace('.', "/")));
    } else if !spec.is_empty() && !spec.contains('/') {
        // Bare name: Rust `mod foo;` or Python `import sibling`.
        push_variants(dir.join(spec));
        push_variants(PathBuf::from(spec));
    } else if spec.contains('/') {
        // Go-style package path — matched by directory name at edge build.
        push_variants(PathBuf::from(spec));
    }
    candidates
}

impl RepoMapIndex {
    /// Build an index over `root`, parsing every supported file.
    pub fn scan(root: &Path) -> anyhow::Result<Self> {
        let mut index = Self {
            root: root.to_path_buf(),
            files: BTreeMap::new(),
            edges: HashMap::new(),
        };
        index.refresh()?;
        Ok(index)
    }

    /// Files currently in the index — what `Ready` announcements report.
    pub fn indexed_files(&self) -> usize {
        self.files.len()
    }

    /// Re-walk the workspace; re-parse only files whose content hash changed.
    pub fn refresh(&mut self) -> anyhow::Result<()> {
        let specs = language_specs();
        let mut parsers: Vec<Option<Parser>> = (0..specs.len()).map(|_| None).collect();
        let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
        let mut parsed_any = false;

        let mut walk = WalkBuilder::new(&self.root);
        walk.hidden(true)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .require_git(false)
            .max_filesize(Some(MAX_FILE_BYTES));

        for entry in walk.build().flatten() {
            if seen.len() >= MAX_INDEXED_FILES {
                break;
            }
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let path = entry.path();
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            let Some(lang_idx) = spec_for_extension(ext) else {
                continue;
            };
            let Ok(rel) = path.strip_prefix(&self.root) else {
                continue;
            };
            let rel = rel.to_path_buf();
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            let hash = hash_bytes(&bytes);
            seen.insert(rel.clone());

            let needs_parse = self
                .files
                .get(&rel)
                .is_none_or(|file| file.hash != hash || file.language != lang_idx);
            if !needs_parse {
                continue;
            }
            if parsers[lang_idx].is_none() {
                let mut parser = Parser::new();
                parser
                    .set_language(&specs[lang_idx].language)
                    .map_err(|e| anyhow::anyhow!("tree-sitter language load failed: {e}"))?;
                parsers[lang_idx] = Some(parser);
            }
            let spec = &specs[lang_idx];
            let parser = parsers[lang_idx].as_mut().unwrap();
            let Some(tree) = parser.parse(&bytes, None) else {
                continue;
            };
            let mut symbols = Vec::new();
            collect_defs(tree.root_node(), &bytes, spec, None, &mut symbols);
            if symbols.len() > MAX_SYMBOLS_PER_FILE {
                let extra = symbols.len() - MAX_SYMBOLS_PER_FILE;
                symbols.truncate(MAX_SYMBOLS_PER_FILE);
                symbols.push((0, format!("… +{extra} more")));
            }

            let mut raw_specs = Vec::new();
            let root_node = tree.root_node();
            let mut cursor = root_node.walk();
            for child in root_node.named_children(&mut cursor) {
                if spec.import_kinds.contains(&child.kind()) {
                    collect_import_specs(child, &bytes, &mut raw_specs);
                }
            }
            self.files.insert(
                rel,
                IndexedFile {
                    hash,
                    language: lang_idx,
                    symbols,
                    import_specs: raw_specs,
                },
            );
            parsed_any = true;
        }

        let removed: Vec<PathBuf> = self
            .files
            .keys()
            .filter(|p| !seen.contains(*p))
            .cloned()
            .collect();
        for rel in removed {
            self.files.remove(&rel);
        }

        // Edges are cheap enough to rebuild wholesale after any refresh; they
        // depend on the complete file set, not on which files changed.
        if parsed_any || !self.edges.is_empty() {
            self.rebuild_edges();
        }
        Ok(())
    }

    fn rebuild_edges(&mut self) {
        self.edges.clear();
        let indexed: BTreeSet<PathBuf> = self.files.keys().cloned().collect();
        // Directory-name index for Go-style package imports.
        let mut dirs: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for path in &indexed {
            if let Some(parent) = path.parent() {
                if let Some(name) = parent.file_name().and_then(|n| n.to_str()) {
                    dirs.entry(name.to_owned())
                        .or_default()
                        .push(parent.to_path_buf());
                }
            }
        }

        for (file, entry) in &self.files {
            let mut targets = BTreeSet::new();
            for spec in &entry.import_specs {
                for candidate in resolve_import(file, spec) {
                    if indexed.contains(&candidate) && candidate != *file {
                        targets.insert(candidate);
                        break;
                    }
                }
                // Go package imports: `a/b/pkgname` → files in dirs named
                // `pkgname`, when exactly one such dir exists.
                if spec.contains('/') && !spec.starts_with('.') {
                    if let Some(last) = spec.rsplit('/').next() {
                        if let Some(matching) = dirs.get(last) {
                            if matching.len() == 1 {
                                let prefix = &matching[0];
                                for path in &indexed {
                                    if path.starts_with(prefix) && path != file {
                                        targets.insert(path.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !targets.is_empty() {
                self.edges.insert(file.clone(), targets);
            }
        }
    }

    /// Render the map body within `max_tokens` (estimated as chars / 4).
    ///
    /// Files are ranked by inbound import edges, then by path. Files with no
    /// extracted symbols are never rendered.
    pub fn render(&self, max_tokens: usize) -> ProjectMap {
        let mut in_degree: HashMap<PathBuf, usize> = HashMap::new();
        for targets in self.edges.values() {
            for target in targets {
                *in_degree.entry(target.clone()).or_default() += 1;
            }
        }

        let mut ranked: Vec<(&PathBuf, &IndexedFile)> = self
            .files
            .iter()
            .filter(|(_, file)| !file.symbols.is_empty())
            .collect();
        ranked.sort_by(|(a_path, _a), (b_path, _b)| {
            let a_deg = in_degree.get(*a_path).copied().unwrap_or(0);
            let b_deg = in_degree.get(*b_path).copied().unwrap_or(0);
            b_deg.cmp(&a_deg).then_with(|| a_path.cmp(b_path))
        });

        let max_chars = max_tokens * 4;
        let mut text = String::new();
        let mut mapped = 0usize;
        let mut omitted = 0usize;
        for (path, file) in ranked {
            let mut block = String::new();
            // Provider-facing paths keep forward slashes on every platform —
            // `Path::display` would emit `\` on Windows.
            block.push_str(&path.to_string_lossy().replace('\\', "/"));
            block.push_str(":\n");
            for (depth, signature) in &file.symbols {
                let indent = 2 + depth * 2;
                block.push_str(&" ".repeat(indent));
                block.push_str(signature);
                block.push('\n');
            }
            if text.len() + block.len() > max_chars {
                omitted += 1;
                continue;
            }
            text.push_str(&block);
            mapped += 1;
        }
        if omitted > 0 {
            text.push_str(&format!("… {omitted} more files (map budget)\n"));
        }
        let estimated_tokens = text.len() / 4;
        ProjectMap {
            text,
            indexed_files: self.files.len(),
            mapped_files: mapped,
            omitted_files: omitted,
            estimated_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "repo-map-test-{}",
            uuid::Uuid::new_v4()
        ));
        for (rel, contents) in files {
            let path = dir.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        dir
    }

    #[test]
    fn extracts_top_level_symbols_per_language() {
        let dir = fixture(&[
            ("src/util.ts", "export function helper(a: number): string {\n  return '';\n}\nexport const LIMIT = 3;\n"),
            ("src/app.py", "class App:\n    def run(self):\n        pass\n\ndef main():\n    pass\n"),
            ("src/lib.rs", "pub fn entry() {}\nstruct State { x: i32 }\nimpl State {\n    fn go(&self) {}\n}\n"),
            ("main.go", "package main\n\nfunc main() {}\nfunc helper() {}\n"),
            ("App.swift", "class App {\n    func launch() {}\n}\nstruct Config {}\n"),
            ("README.md", "# not code\n"),
        ]);
        let index = RepoMapIndex::scan(&dir).unwrap();
        let map = index.render(10_000);
        assert_eq!(map.indexed_files, 5);
        assert!(map.text.contains("src/util.ts:"));
        assert!(map.text.contains("export function helper(a: number): string {"));
        assert!(map.text.contains("class App:"));
        assert!(map.text.contains("def run(self):"));
        assert!(map.text.contains("impl State {"));
        assert!(map.text.contains("fn go(&self) {}"));
        assert!(map.text.contains("func main() {}"));
        assert!(map.text.contains("func launch() {}"));
    }

    #[test]
    fn ranks_imported_files_first() {
        let dir = fixture(&[
            ("src/z_unrelated.ts", "export const z = 1;\n"),
            ("src/core.ts", "export function core() {}\n"),
            ("src/app.ts", "import { core } from './core';\nexport function app() { core(); }\n"),
        ]);
        let index = RepoMapIndex::scan(&dir).unwrap();
        let map = index.render(10_000);
        let core_pos = map.text.find("src/core.ts:").unwrap();
        let app_pos = map.text.find("src/app.ts:").unwrap();
        let z_pos = map.text.find("src/z_unrelated.ts:").unwrap();
        // core.ts has an inbound edge; app.ts and z do not.
        assert!(core_pos < app_pos);
        assert!(core_pos < z_pos);
    }

    #[test]
    fn respects_the_token_budget() {
        let files: Vec<(String, String)> = (0..30)
            .map(|i| {
                (
                    format!("src/file_{i:02}.ts"),
                    format!("export function f{i}() {{}}\n"),
                )
            })
            .collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
        let dir = fixture(&refs);
        let index = RepoMapIndex::scan(&dir).unwrap();
        let map = index.render(100); // 400 chars
        assert!(map.omitted_files > 0);
        assert!(map.mapped_files < 30);
        assert!(map.text.contains("more files (map budget)"));
        assert!(map.estimated_tokens <= 110);
    }

    #[test]
    fn refresh_reparses_only_changed_files() {
        let dir = fixture(&[("a.ts", "export const a = 1;\n"), ("b.ts", "export const b = 1;\n")]);
        let mut index = RepoMapIndex::scan(&dir).unwrap();
        fs::write(dir.join("a.ts"), "export function renamed() {}\n").unwrap();
        index.refresh().unwrap();
        let map = index.render(10_000);
        assert!(map.text.contains("export function renamed() {}"));
        // Removing a file drops it.
        fs::remove_file(dir.join("b.ts")).unwrap();
        index.refresh().unwrap();
        assert!(!index.render(10_000).text.contains("b.ts:"));
    }

    #[test]
    fn resolves_rust_mod_and_use_edges() {
        let dir = fixture(&[
            ("src/lib.rs", "mod util;\nuse crate::deep::thing;\npub fn root() {}\n"),
            ("src/util.rs", "pub fn helper() {}\n"),
            ("src/deep/mod.rs", "pub mod thing;\n"),
            ("src/deep/thing.rs", "pub fn stuff() {}\n"),
        ]);
        let index = RepoMapIndex::scan(&dir).unwrap();
        let edges = &index.edges[&PathBuf::from("src/lib.rs")];
        assert!(edges.contains(&PathBuf::from("src/util.rs")));
        assert!(edges.contains(&PathBuf::from("src/deep/thing.rs"))
            || edges.contains(&PathBuf::from("src/deep/mod.rs")));
    }

    #[test]
    fn ignores_files_listed_in_gitignore() {
        let dir = fixture(&[
            (".gitignore", "generated/\n"),
            ("generated/out.ts", "export const gen = 1;\n"),
            ("src/real.ts", "export const real = 1;\n"),
        ]);
        let index = RepoMapIndex::scan(&dir).unwrap();
        let map = index.render(10_000);
        assert_eq!(map.indexed_files, 1);
        assert!(map.text.contains("src/real.ts:"));
        assert!(!map.text.contains("generated"));
    }
}
