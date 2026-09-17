//! Repairs AI-style escaped backticks inside inline code spans.
//!
//! Models emit `` `a\`b` `` as if a code span were a string literal.
//! CommonMark keeps the backslash and closes the span at that backtick,
//! which both corrupts the code text and strands a delimiter that can
//! swallow prose into a later span — `` `a\`b` and `c\`d` `` parses " and "
//! as code. When a single-backtick span's first `\`` is glued to more text —
//! unlike a legitimately closing `` `path\` `` — the span is rewritten with
//! a longer delimiter run so the backticks land as literal content.
//!
//! A single backtick glued to a keyboard shortcut — `Ctrl+``, `⌘`` — is
//! the same genre of miss: the model means a literal keycap, but CommonMark
//! opens a span that swallows prose up to the next backtick on the line.
//! The repair escapes the backtick so it renders literally.
//!
//! The rewrite runs on the raw source, guided by a first parse's event
//! ranges: regions where a backtick is already literal (code blocks, raw
//! HTML, math) stay byte-exact. The caller reparses the repaired source and
//! maps event ranges back through [`original_offset`].

use std::ops::Range;

use pulldown_cmark::{Event, Tag};

/// One rewritten span: its range in the original source and in the repaired
/// source. Ascending and non-overlapping.
pub type Repair = (Range<usize>, Range<usize>);

/// Repair escaped-backtick code spans in `source`. `events` is a first pass
/// over the same source; their ranges mark the regions that must stay
/// byte-exact. Returns the repaired source and the (original, repaired)
/// range pairs needed to map offsets back, or `None` — the common case —
/// when nothing needed repair.
pub fn repair_code_spans(
    source: &str,
    events: &[(Event<'_>, Range<usize>)],
) -> Option<(String, Vec<Repair>)> {
    // Neither signature can exist without its bytes.
    if !(source.contains("\\`")
        || source.contains("+`")
        || source.contains(['⌘', '⌃', '⌥', '⇧']))
    {
        return None;
    }

    let mut excluded: Vec<Range<usize>> = Vec::new();
    for (event, range) in events {
        match event {
            Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Html(_)
            | Event::InlineHtml(_) => excluded.push(range.clone()),
            _ => {}
        }
    }
    excluded.sort_by_key(|range| range.start);

    let bytes = source.as_bytes();
    let mut excluded = excluded.into_iter().peekable();
    let mut repairs: Vec<(Range<usize>, String)> = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        while let Some(range) = excluded.peek() {
            if pos >= range.end {
                excluded.next();
            } else {
                if pos >= range.start {
                    pos = range.end;
                }
                break;
            }
        }
        if pos >= bytes.len() {
            break;
        }
        if bytes[pos] != b'`' {
            pos += 1;
            continue;
        }
        let run = run_length(bytes, pos);
        if run != 1 {
            pos = skip_multi_backtick_span(source, pos, run);
            continue;
        }
        if is_shortcut_key(source, pos) {
            // Escaping the backtick renders it literally and stops it
            // opening a span that swallows prose up to the next backtick.
            repairs.push((pos..pos + 1, "\\`".to_owned()));
            pos += 1;
            continue;
        }
        match scan_single_backtick_span(source, pos) {
            SpanScan::Repair { close, end } => {
                let content = source[pos + 1..close].replace("\\`", "`");
                // The fence must outrun any backtick run in the content, and
                // padding both sides survives CommonMark's one-space strip,
                // keeping the content byte-exact even next to backticks.
                let fence = "`".repeat(longest_backtick_run(&content).max(1) + 1);
                repairs.push((pos..end, format!("{fence} {content} {fence}")));
                pos = end;
            }
            SpanScan::Leave { end } => pos = end,
        }
    }

    if repairs.is_empty() {
        return None;
    }

    let mut repaired = String::with_capacity(source.len());
    let mut ranges = Vec::with_capacity(repairs.len());
    let mut cursor = 0;
    for (range, replacement) in repairs {
        repaired.push_str(&source[cursor..range.start]);
        let start = repaired.len();
        repaired.push_str(&replacement);
        ranges.push((range.clone(), start..repaired.len()));
        cursor = range.end;
    }
    repaired.push_str(&source[cursor..]);
    Some((repaired, ranges))
}

/// Map a byte offset in the repaired source back to the original source.
/// Offsets inside a repaired span collapse to its original start.
pub fn original_offset(repairs: &[Repair], offset: usize) -> usize {
    let mut shift = 0isize;
    for (original, repaired) in repairs {
        if offset < repaired.start {
            break;
        }
        if offset < repaired.end {
            return original.start;
        }
        shift += repaired.len() as isize - original.len() as isize;
    }
    (offset as isize - shift) as usize
}

enum SpanScan {
    /// An AI-style escape: rewrite `open..end`, whose content ends at
    /// `close` — the start of the real closer's run.
    Repair { close: usize, end: usize },
    /// An ordinary or unresolvable span: resume scanning at `end`.
    Leave { end: usize },
}

/// Scan the single-backtick span opened at `open`, bounded to its line. A
/// backtick preceded by an odd run of backslashes is the model's
/// string-literal escape: glued to more text it is content, and the real
/// closer is the next truly unescaped backtick. Without that glued proof
/// the span is ordinary — `` `path\` `` closes legitimately.
fn scan_single_backtick_span(source: &str, open: usize) -> SpanScan {
    let bytes = source.as_bytes();
    let line_end = source[open..]
        .find('\n')
        .map_or(bytes.len(), |offset| open + offset);
    let mut scan = open + 1;
    let mut escaped = false;
    while let Some(offset) = source[scan..line_end].find('`') {
        let backtick = scan + offset;
        let slashes = bytes[..backtick]
            .iter()
            .rev()
            .take_while(|byte| **byte == b'\\')
            .count();
        if slashes % 2 == 1 {
            if escaped
                || bytes
                    .get(backtick + 1)
                    .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                escaped = true;
                scan = backtick + 1;
                continue;
            }
            return SpanScan::Leave { end: backtick + 1 };
        }
        let end = backtick + run_length(bytes, backtick);
        return if escaped {
            SpanScan::Repair {
                close: backtick,
                end,
            }
        } else {
            SpanScan::Leave { end }
        };
    }
    // No closer on the line. Resume past whatever was scanned — a `\`` must
    // not rescan as an opener.
    SpanScan::Leave { end: scan }
}

/// Multi-backtick spans hold single backticks — escaped or not — as literal
/// content, so skip to just past the matching closer, or just the opener
/// when there is none.
fn skip_multi_backtick_span(source: &str, open: usize, run: usize) -> usize {
    let bytes = source.as_bytes();
    let mut scan = open + run;
    while let Some(offset) = source[scan..].find('`') {
        let backtick = scan + offset;
        let length = run_length(bytes, backtick);
        scan = backtick + length;
        if length == run {
            return scan;
        }
    }
    open + run
}

/// True when the single backtick at byte `pos` is glued to a keyboard-
/// shortcut token — `Ctrl+``, `⌘`` — making it a literal keycap rather
/// than a code-span opener. Requiring a letter before `+` matches
/// `Ctrl+`` and `Cmd+Shift+`` while sparing `x + `y`` (a real span) and
/// `` `a`+`b` `` (the `+` follows a backtick); the modifier symbols are
/// unambiguous on their own.
pub(super) fn is_shortcut_key(source: &str, pos: usize) -> bool {
    let mut before = source[..pos].chars();
    match before.next_back() {
        Some('+') => before
            .next_back()
            .is_some_and(|ch| ch.is_ascii_alphabetic()),
        Some(ch) => matches!(ch, '⌘' | '⌃' | '⌥' | '⇧'),
        None => false,
    }
}

fn run_length(bytes: &[u8], pos: usize) -> usize {
    bytes[pos..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

/// Longest backtick run in `text`, so a repaired span's fence always
/// outruns its content.
fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut current = 0;
    for byte in text.bytes() {
        if byte == b'`' {
            current += 1;
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark::{Options, Parser};

    fn repair(source: &str) -> Option<String> {
        let events = Parser::new_ext(source, Options::all())
            .into_offset_iter()
            .collect::<Vec<_>>();
        repair_code_spans(source, &events).map(|(repaired, _)| repaired)
    }

    #[test]
    fn repairs_escaped_backticks_in_code_spans() {
        assert_eq!(
            repair("use `a\\`b` here").as_deref(),
            Some("use `` a`b `` here")
        );
        assert_eq!(
            repair("use `a\\`b` and `c\\`d` end").as_deref(),
            Some("use `` a`b `` and `` c`d `` end")
        );
        assert_eq!(
            repair("the `the \\`ls\\` command` ran").as_deref(),
            Some("the `` the `ls` command `` ran")
        );
    }

    #[test]
    fn escapes_shortcut_backticks() {
        assert_eq!(
            repair("press Ctrl+` to open `terminal`").as_deref(),
            Some("press Ctrl+\\` to open `terminal`")
        );
        assert_eq!(
            repair("press ⌘` to focus").as_deref(),
            Some("press ⌘\\` to focus")
        );
        assert_eq!(
            repair("Ctrl+Shift+` cycles panes").as_deref(),
            Some("Ctrl+Shift+\\` cycles panes")
        );
    }

    #[test]
    fn leaves_non_shortcut_backticks_alone() {
        for source in [
            "x + `y` is code",
            "the `a`+`b` pair",
            "`x+`",
            "2+`x`",
        ] {
            assert_eq!(repair(source), None, "unexpected repair for {source:?}");
        }
    }

    #[test]
    fn leaves_legitimate_spans_alone() {
        for source in [
            "plain text",
            "`C:\\path\\` is a dir",
            "`code` and `` `x` ``",
            "\\`escaped opener\\`",
        ] {
            assert_eq!(repair(source), None, "unexpected repair for {source:?}");
        }
    }

    #[test]
    fn leaves_code_blocks_and_math_byte_exact() {
        let source = "para `a\\`b`\n\n```\n`x\\`y`\n```\n\nmath $`m\\`n`$";
        let repaired = repair(source).expect("the paragraph span should repair");
        assert!(repaired.contains("`` a`b ``"));
        assert!(
            repaired.contains("`x\\`y`"),
            "fenced code changed: {repaired}"
        );
        assert!(repaired.contains("$`m\\`n`$"), "math changed: {repaired}");
    }

    #[test]
    fn offsets_map_back_through_repairs() {
        let source = "a `x\\`y` b `p\\`q` c";
        let events = Parser::new_ext(source, Options::all())
            .into_offset_iter()
            .collect::<Vec<_>>();
        let (repaired, repairs) = repair_code_spans(source, &events).expect("repairs");
        assert_eq!(repaired, "a `` x`y `` b `` p`q `` c");
        // Repaired span edges map to the original span's edges.
        for (original, repaired_range) in &repairs {
            assert_eq!(
                original_offset(&repairs, repaired_range.start),
                original.start
            );
            assert_eq!(original_offset(&repairs, repaired_range.end), original.end);
        }
        // Every byte outside a repair maps to itself.
        let mut shift = 0isize;
        let mut original_pos = 0;
        for (original, repaired_range) in &repairs {
            while original_pos < original.start {
                let repaired_pos = (original_pos as isize + shift) as usize;
                assert_eq!(original_offset(&repairs, repaired_pos), original_pos);
                original_pos += 1;
            }
            shift += repaired_range.len() as isize - original.len() as isize;
            original_pos = original.end;
        }
        while original_pos < source.len() {
            let repaired_pos = (original_pos as isize + shift) as usize;
            assert_eq!(original_offset(&repairs, repaired_pos), original_pos);
            original_pos += 1;
        }
    }
}
