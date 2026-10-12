//! `.gitattributes` support for syntax highlighting — the slice of the
//! format GitHub's linguist uses to pin a language onto paths:
//!
//! ```text
//! *.wisp linguist-language=Common Lisp
//! generated/** -linguist-language
//! ```
//!
//! A declaration overrides the extension table entirely: a repo can point a
//! brand-new language's files at the closest lexer we ship, and a name the
//! lexer does not know renders plain rather than guessing from the suffix.
//!
//! Matching follows git's pattern rules (the `.gitignore` grammar minus the
//! two exceptions `.gitattributes` carries: no `!` negation, and a pattern
//! that matches a directory does not cascade into it). Layers apply shallow
//! → deep and the last matching assignment wins, the same precedence
//! `git check-attr` reports.
//!
//! Nothing here touches the filesystem: [`read_layer`] fetches through the
//! workspace channel so remote projects resolve the same way, and callers
//! decide which directories' files to load.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::md::highlight::lang_for_tag;

/// What a `linguist-language` attribute says on one line.
#[derive(Clone, Debug, PartialEq)]
pub enum LanguageAttr {
    /// `linguist-language=<name>` — the declaration highlighting uses.
    Named(String),
    /// `linguist-language` or `-linguist-language` — set or cleared with no
    /// usable name; either way it erases an earlier `Named` on the path.
    Erased,
}

/// One parsed `.gitattributes` file. Only rules that assign
/// `linguist-language` are kept — diff filters, EOL markers, and macros
/// mean nothing to a highlighter.
#[derive(Default)]
pub struct AttributeFile {
    rules: Vec<Rule>,
}

struct Rule {
    pattern: Pattern,
    attr: LanguageAttr,
}

impl AttributeFile {
    /// The last `linguist-language` assignment matching `path` — relative to
    /// the directory this file sits in. `None` means no line governs it, so
    /// an outer layer or the extension table decides.
    pub fn language_state(&self, path: &Path) -> Option<&LanguageAttr> {
        let segments: Vec<&str> = path
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect();
        let mut state = None;
        for rule in &self.rules {
            if rule.pattern.matches(&segments) {
                state = Some(&rule.attr);
            }
        }
        state
    }
}

/// Every `.gitattributes` that governs one query root, ordered shallow →
/// deep so [`Layers::declared_language`] can apply git's last-match wins.
#[derive(Default)]
pub struct Layers {
    dirs: Vec<(PathBuf, AttributeFile)>,
}

impl Layers {
    /// Whether a governing directory's rules could still change the answer.
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty()
    }

    /// The `linguist-language` name pinned on `path` (relative to the root
    /// the layers were read against). `None` means no declaration — the
    /// caller's own detection stays in charge; a `Named` that was later
    /// `Erased` collapses to `None` here because both mean "not declared".
    pub fn declared_language(&self, path: &str) -> Option<&str> {
        declared_language(
            self.dirs.iter().map(|(dir, file)| (dir.as_path(), file)),
            path,
        )
    }
}

/// The `linguist-language` pinned on `path` under `layers` — `(dir, file)`
/// pairs ordered shallow → deep, each `dir` relative to `path`'s root.
/// Layers whose directory does not contain `path` contribute nothing.
pub fn declared_language<'a>(
    layers: impl IntoIterator<Item = (&'a Path, &'a AttributeFile)>,
    path: &str,
) -> Option<&'a str> {
    let path = Path::new(path);
    let mut declared = None;
    for (dir, file) in layers {
        let Ok(rest) = path.strip_prefix(dir) else {
            continue;
        };
        declared = match file.language_state(rest) {
            Some(LanguageAttr::Named(name)) => Some(name.as_str()),
            Some(LanguageAttr::Erased) => None,
            None => declared,
        };
    }
    declared
}

/// The directories whose `.gitattributes` can govern `paths` — the root
/// plus every ancestor — sorted shallow → deep for [`Layers`] precedence.
pub fn governing_dirs<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<PathBuf> {
    let mut dirs = BTreeSet::new();
    dirs.insert(PathBuf::new());
    for path in paths {
        let mut dir = Path::new(path).parent();
        while let Some(ancestor) = dir {
            dirs.insert(ancestor.to_path_buf());
            dir = ancestor.parent();
        }
    }
    let mut dirs: Vec<_> = dirs.into_iter().collect();
    dirs.sort_by_key(|dir| dir.components().count());
    dirs
}

/// The path column of a `--numstat` listing — the third tab-separated
/// field on each line. Rename and quoting quirks stay on the consumer's
/// side; an unusable line simply contributes no directory.
pub fn numstat_paths(numstat: &str) -> impl Iterator<Item = &str> {
    numstat
        .lines()
        .filter_map(|line| line.splitn(3, '\t').nth(2))
}

/// Read `dir`'s `.gitattributes` through the workspace channel, keeping the
/// round trip on the same daemon the file reads already go through — remote
/// projects resolve identically. Blocking, so callers keep it on the
/// background executor; `None` covers both "absent" and "unreadable",
/// which mean the same thing here: no rules.
pub fn read_layer(
    workspace: &waku_client::WorkspaceClient,
    root: &Path,
    dir: &Path,
) -> Option<AttributeFile> {
    match workspace.request(waku_client::WorkspaceOperation::ReadTextFile {
        root: root.to_path_buf(),
        relative_path: dir.join(".gitattributes"),
    }) {
        Ok(waku_client::WorkspaceResult::TextFile { content }) => Some(parse(&content)),
        _ => None,
    }
}

/// Fetch every `.gitattributes` that can govern `paths` — the root file and
/// each ancestor directory's — through `workspace`, in precedence order.
/// `None` when no governing directory has the file at all.
pub fn load_for_paths<'a>(
    workspace: &waku_client::WorkspaceClient,
    root: &Path,
    paths: impl IntoIterator<Item = &'a str>,
) -> Option<Layers> {
    let mut layers = Layers::default();
    for dir in governing_dirs(paths) {
        if let Some(file) = read_layer(workspace, root, &dir) {
            layers.dirs.push((dir, file));
        }
    }
    (!layers.is_empty()).then_some(layers)
}

/// The fence tag the lexer knows for a `linguist-language` value — usually
/// just the lowercased name, with a table for linguist spellings that differ
/// from ours (`Objective-C` → `objc`). A name we cannot highlight resolves
/// to `"text"`: an explicit declaration always wins, and wrong-language
/// colours are worse than none.
pub fn tag_for_linguist(name: &str) -> Cow<'static, str> {
    let tag = name.trim().to_ascii_lowercase();
    let tag = match tag.as_str() {
        "objective-c" => "objc".into(),
        "objective-c++" => "cpp".into(),
        _ => tag,
    };
    if lang_for_tag(&tag).is_some() {
        Cow::Owned(tag)
    } else {
        Cow::Borrowed("text")
    }
}

pub fn parse(text: &str) -> AttributeFile {
    AttributeFile {
        rules: text.lines().filter_map(parse_rule).collect(),
    }
}

fn parse_rule(line: &str) -> Option<Rule> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = split_fields(line).into_iter();
    let pattern = fields.next()?;
    // A pattern confined to a directory (`name/`) assigns attributes to the
    // directory itself, never to the files inside — and only files are ever
    // queried here.
    if pattern.ends_with('/') {
        return None;
    }
    let attr = fields
        .filter_map(|field| match field.as_str() {
            name if name.eq_ignore_ascii_case("linguist-language") => {
                Some(LanguageAttr::Erased)
            }
            name if name
                .strip_prefix('-')
                .is_some_and(|name| name.eq_ignore_ascii_case("linguist-language")) =>
            {
                Some(LanguageAttr::Erased)
            }
            field => field
                .split_once('=')
                .filter(|(name, _)| name.eq_ignore_ascii_case("linguist-language"))
                .map(|(_, value)| LanguageAttr::Named(value.to_owned())),
        })
        .next_back()?;
    Some(Rule {
        pattern: Pattern::compile(&pattern),
        attr,
    })
}

/// Whitespace-separated fields; a backslash quotes the next character so a
/// pattern can carry a literal space or a leading `#`.
fn split_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => field.extend(chars.next()),
            character if character.is_whitespace() => {
                if !field.is_empty() {
                    fields.push(std::mem::take(&mut field));
                }
            }
            _ => field.push(character),
        }
    }
    if !field.is_empty() {
        fields.push(field);
    }
    fields
}

struct Pattern {
    /// Anchored patterns contain a `/` and match the whole path from the
    /// file's directory; floaters match the basename at any depth below it.
    anchored: bool,
    segments: Vec<Vec<char>>,
}

impl Pattern {
    fn compile(text: &str) -> Pattern {
        // A leading or interior `/` anchors the pattern to its directory —
        // check before the anchor is stripped off.
        let anchored = text.contains('/');
        let text = text.strip_prefix('/').unwrap_or(text);
        Pattern {
            anchored,
            segments: text.split('/').map(|s| s.chars().collect()).collect(),
        }
    }

    fn matches(&self, path: &[&str]) -> bool {
        if self.anchored {
            return match_segments(&self.segments, path);
        }
        let Some(basename) = path.last() else {
            return false;
        };
        self.segments
            .first()
            .is_some_and(|segment| match_component(segment, basename))
    }
}

fn match_segments(pattern: &[Vec<char>], path: &[&str]) -> bool {
    let Some((segment, rest)) = pattern.split_first() else {
        return path.is_empty();
    };
    // A `**` component matches zero or more whole directories.
    if segment.len() == 2 && segment.iter().all(|c| *c == '*') {
        return (0..=path.len()).any(|skip| match_segments(rest, &path[skip..]));
    }
    match path.split_first() {
        Some((head, tail)) => {
            match_component(segment, head) && match_segments(rest, tail)
        }
        None => false,
    }
}

fn match_component(pattern: &[char], name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    match_chars(pattern, &name)
}

fn match_chars(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('?', rest)) => !text.is_empty() && match_chars(rest, &text[1..]),
        Some(('*', rest)) => {
            match_chars(rest, text) || (!text.is_empty() && match_chars(pattern, &text[1..]))
        }
        Some(('[', _)) => match parse_class(pattern) {
            Some(class) => {
                let Some(&character) = text.first() else {
                    return false;
                };
                let inside = class
                    .ranges
                    .iter()
                    .any(|(low, high)| *low <= character && character <= *high);
                inside != class.negated
                    && match_chars(&pattern[class.consumed..], &text[1..])
            }
            // An unterminated class makes `[` a literal like any other char.
            None => text.first() == Some(&'[') && match_chars(&pattern[1..], &text[1..]),
        },
        Some(('\\', rest)) => match rest.split_first() {
            Some((escaped, rest)) => {
                text.first() == Some(escaped) && match_chars(rest, &text[1..])
            }
            None => text.is_empty(),
        },
        Some((literal, rest)) => text.first() == Some(literal) && match_chars(rest, &text[1..]),
    }
}

/// A parsed `[...]` class: how many pattern chars it spans, whether it is
/// negated, and the inclusive character ranges inside.
struct CharClass {
    consumed: usize,
    negated: bool,
    ranges: Vec<(char, char)>,
}

/// `[...]` at `pattern`'s head. Unterminated classes are not a class at all.
fn parse_class(pattern: &[char]) -> Option<CharClass> {
    let mut index = 1;
    let negated = matches!(pattern.get(index), Some('!') | Some('^'));
    if negated {
        index += 1;
    }
    let mut ranges = Vec::new();
    // A `]` in first position is a literal member of the class.
    if pattern.get(index) == Some(&']') {
        ranges.push((']', ']'));
        index += 1;
    }
    while let Some(&character) = pattern.get(index) {
        if character == ']' {
            return (!ranges.is_empty()).then_some(CharClass {
                consumed: index + 1,
                negated,
                ranges,
            });
        }
        match (pattern.get(index + 1), pattern.get(index + 2)) {
            (Some('-'), Some(&high)) if high != ']' => {
                ranges.push((character, high));
                index += 3;
            }
            _ => {
                ranges.push((character, character));
                index += 1;
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_rules(text: &str) -> AttributeFile {
        parse(text)
    }

    fn state(file: &AttributeFile, path: &str) -> Option<LanguageAttr> {
        file.language_state(Path::new(path)).cloned()
    }

    #[test]
    fn parses_linguist_language_lines_and_skips_the_rest() {
        let file = parse_rules(
            "# comment\n\
             *.wisp linguist-language=Wisp-Lang\n\
             *.lisp linguist-language=Common Lisp\n\
             *.gen -linguist-language\n\
             *.flag linguist-language\n\
             *.bin binary -diff\n\
             \n\
             docs/*.md linguist-generated linguist-language=Markdown export-ignore\n",
        );
        assert_eq!(
            state(&file, "a.wisp"),
            Some(LanguageAttr::Named("Wisp-Lang".into()))
        );
        // Attribute values end at whitespace, exactly as `git check-attr`
        // reports them — `Common Lisp` declares the name "Common" and a
        // bare `Lisp` attribute.
        assert_eq!(
            state(&file, "a.lisp"),
            Some(LanguageAttr::Named("Common".into()))
        );
        assert_eq!(state(&file, "a.gen"), Some(LanguageAttr::Erased));
        assert_eq!(state(&file, "a.flag"), Some(LanguageAttr::Erased));
        assert_eq!(state(&file, "a.bin"), None, "no language attribute");
        assert_eq!(
            state(&file, "docs/page.md"),
            Some(LanguageAttr::Named("Markdown".into()))
        );
        assert_eq!(state(&file, "other/page.md"), None, "anchored to docs/");
    }

    #[test]
    fn floating_patterns_match_basenames_at_any_depth() {
        let file = parse_rules("*.wisp linguist-language=Rust\n");
        for path in ["a.wisp", "src/deep/nested/a.wisp"] {
            assert_eq!(
                state(&file, path),
                Some(LanguageAttr::Named("Rust".into())),
                "{path}"
            );
        }
        assert_eq!(state(&file, "a.wispy"), None);
    }

    #[test]
    fn anchored_patterns_bind_the_files_directory() {
        let file = parse_rules(
            "src/*.wisp linguist-language=Rust\n\
             /exact.wisp linguist-language=Python\n",
        );
        assert_eq!(
            state(&file, "src/a.wisp"),
            Some(LanguageAttr::Named("Rust".into()))
        );
        assert_eq!(state(&file, "other/src/a.wisp"), None);
        assert_eq!(
            state(&file, "exact.wisp"),
            Some(LanguageAttr::Named("Python".into()))
        );
        assert_eq!(state(&file, "sub/exact.wisp"), None);
    }

    #[test]
    fn double_star_spans_directories() {
        let file = parse_rules(
            "src/** linguist-language=Go\n\
             gen/**/keep.wisp linguist-language=Rust\n\
             **/*.tsrx linguist-language=TSX\n",
        );
        assert_eq!(
            state(&file, "src/a.wisp"),
            Some(LanguageAttr::Named("Go".into()))
        );
        assert_eq!(
            state(&file, "src/deep/nested/a.wisp"),
            Some(LanguageAttr::Named("Go".into()))
        );
        assert_eq!(
            state(&file, "gen/x/keep.wisp"),
            Some(LanguageAttr::Named("Rust".into()))
        );
        assert_eq!(
            state(&file, "gen/keep.wisp"),
            Some(LanguageAttr::Named("Rust".into())),
            "** matches zero directories"
        );
        assert_eq!(
            state(&file, "anywhere/card.tsrx"),
            Some(LanguageAttr::Named("TSX".into()))
        );
    }

    #[test]
    fn question_mark_and_classes_match_single_characters() {
        let file = parse_rules(
            "mod?.wisp linguist-language=Rust\n\
             part[0-9].wisp linguist-language=Go\n\
             keep[!ab].wisp linguist-language=Python\n",
        );
        assert_eq!(
            state(&file, "mod1.wisp"),
            Some(LanguageAttr::Named("Rust".into()))
        );
        assert_eq!(state(&file, "modx.wisp"), Some(LanguageAttr::Named("Rust".into())));
        assert_eq!(state(&file, "mod.wisp"), None);
        assert_eq!(
            state(&file, "part7.wisp"),
            Some(LanguageAttr::Named("Go".into()))
        );
        assert_eq!(state(&file, "parta.wisp"), None);
        assert_eq!(
            state(&file, "keepz.wisp"),
            Some(LanguageAttr::Named("Python".into()))
        );
        assert_eq!(state(&file, "keepa.wisp"), None);
    }

    #[test]
    fn last_matching_line_wins() {
        let file = parse_rules(
            "*.wisp linguist-language=Rust\n\
             special.wisp linguist-language=Go\n\
             gone.wisp -linguist-language\n",
        );
        assert_eq!(
            state(&file, "special.wisp"),
            Some(LanguageAttr::Named("Go".into()))
        );
        assert_eq!(state(&file, "gone.wisp"), Some(LanguageAttr::Erased));
    }

    #[test]
    fn escaped_spaces_and_hashes_stay_in_the_pattern() {
        let file = parse_rules("my\\ file.wisp linguist-language=Rust\n\\#tag linguist-language=Go\n");
        assert_eq!(
            state(&file, "my file.wisp"),
            Some(LanguageAttr::Named("Rust".into()))
        );
        assert_eq!(
            state(&file, "#tag"),
            Some(LanguageAttr::Named("Go".into()))
        );
    }

    #[test]
    fn directory_patterns_do_not_cascade() {
        let file = parse_rules("docs/ linguist-language=Markdown\n");
        assert_eq!(state(&file, "docs"), None);
        assert_eq!(state(&file, "docs/page.md"), None);
    }

    #[test]
    fn deeper_layers_override_shallower_ones() {
        let root = parse_rules("*.wisp linguist-language=Rust\n");
        let nested = parse_rules("*.wisp linguist-language=Go\n*.wisp -linguist-language\n");
        let mut layers = Layers::default();
        layers.dirs.push((PathBuf::new(), root));
        layers.dirs.push((PathBuf::from("src"), nested));

        assert_eq!(layers.declared_language("a.wisp"), Some("Rust"));
        // src's second line erases what its first set — and the root's.
        assert_eq!(layers.declared_language("src/a.wisp"), None);
        // A directory beside the layer is untouched by it.
        assert_eq!(layers.declared_language("other/a.wisp"), Some("Rust"));
    }

    #[test]
    fn governing_dirs_lists_root_then_ancestors() {
        let dirs = governing_dirs(["a/b/c.rs", "a/d.rs", "top.rs"].into_iter());
        assert_eq!(
            dirs,
            vec![
                PathBuf::new(),
                PathBuf::from("a"),
                PathBuf::from("a/b"),
            ]
        );
    }

    #[test]
    fn numstat_paths_reads_the_third_column() {
        let paths: Vec<_> = numstat_paths("2\t1\tsrc/lib.rs\n-\t-\tassets/logo.png\n").collect();
        assert_eq!(paths, ["src/lib.rs", "assets/logo.png"]);
    }

    #[test]
    fn linguist_names_normalize_to_lexer_tags() {
        assert_eq!(tag_for_linguist("Rust"), "rust");
        assert_eq!(tag_for_linguist("TypeScript"), "typescript");
        assert_eq!(tag_for_linguist("C++"), "c++");
        assert_eq!(tag_for_linguist("Objective-C"), "objc");
        assert_eq!(tag_for_linguist("Objective-C++"), "cpp");
        // Unknown names degrade to plain rather than a guessed language.
        assert_eq!(tag_for_linguist("Brainfuck"), "text");
        assert_eq!(tag_for_linguist("Common Lisp"), "text");
    }
}
