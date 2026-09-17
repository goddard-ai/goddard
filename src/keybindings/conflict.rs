//! Context-predicate overlap analysis for the keybinding manager.
//!
//! GPUI context predicates are a boolean grammar over identifiers:
//! `!`, `&&`, `||`, parentheses, and `>` (ancestor — both sides must hold,
//! so for overlap purposes it is conjunction). The analyzer normalizes a
//! predicate to DNF over identifier literals and tests pairwise
//! satisfiability. Anything the parser cannot handle degrades to
//! [`ConflictKind::UnknownOverlap`], never "no conflict".

use std::collections::BTreeSet;

use crate::keybindings::PlatformSet;

/// How two same-sequence bindings collide.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConflictKind {
    /// Same normalized predicate: registration order decides silently and the
    /// earlier binding is unreachable.
    Hard,
    /// One predicate strictly implies the other — precedence is
    /// deterministic; informational, not blocking.
    Shadowed,
    /// Predicates overlap but neither implies the other: both bindings are
    /// reachable in different subtrees, so this is informational.
    PartialOverlap,
    /// Same action bound twice under an equal predicate — redundant.
    Duplicate,
    /// A predicate used grammar the analyzer does not understand.
    UnknownOverlap,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub kind: ConflictKind,
    /// The other command involved.
    pub other: String,
    /// The other binding's context predicate source.
    pub other_context: Option<String>,
}

/// A normalized context predicate: DNF as a set of conjunctions, each
/// conjunction a map identifier → required polarity.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextDnf {
    /// Each inner set is a conjunction `(identifier, negated)`. Empty outer
    /// set = never; a conjunction with no literals = always (no predicate).
    pub terms: Vec<BTreeSet<(String, bool)>>,
    /// The predicate parsed cleanly; false → callers report UnknownOverlap.
    pub parsed: bool,
}

impl ContextDnf {
    fn always() -> Self {
        Self {
            terms: vec![BTreeSet::new()],
            parsed: true,
        }
    }

    /// `self` && `other` in DNF.
    fn and(&self, other: &Self) -> Self {
        let mut terms = Vec::new();
        for left in &self.terms {
            for right in &other.terms {
                terms.push(left.union(right).cloned().collect());
            }
        }
        Self {
            terms,
            parsed: self.parsed && other.parsed,
        }
    }

    fn or(&self, other: &Self) -> Self {
        Self {
            terms: self.terms.iter().chain(other.terms.iter()).cloned().collect(),
            parsed: self.parsed && other.parsed,
        }
    }

    /// Can a context stack satisfy both `self` and `other` at once? True when
    /// some conjunction pair has no contradictory literal.
    pub fn can_overlap(&self, other: &Self) -> bool {
        self.terms.iter().any(|left| {
            other.terms.iter().any(|right| {
                !left.iter().any(|(id, neg)| {
                    right.iter().any(|(rid, rneg)| id == rid && neg != rneg)
                })
            })
        })
    }

    /// `self` implies `other`: every term of `self` is a superset of some
    /// term of `other`. (Sufficient, not complete — implication beyond this
    /// syntactic check is reported as unknown-adjacent by callers.)
    pub fn implies(&self, other: &Self) -> bool {
        !self.terms.is_empty()
            && self
                .terms
                .iter()
                .all(|left| other.terms.iter().any(|right| left.is_superset(right)))
    }
}

/// Parse a context predicate into DNF. Grammar: identifiers, `!`, `&&`,
/// `||`, `>` (treated as conjunction — an ancestor chain requires every
/// identifier), parentheses.
pub fn parse_context(source: Option<&str>) -> ContextDnf {
    let Some(source) = source else {
        return ContextDnf::always();
    };
    let tokens = tokenize(source);
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        ok: true,
    };
    let expr = parser.or();
    if parser.pos != tokens.len() || !parser.ok {
        return ContextDnf {
            terms: Vec::new(),
            parsed: false,
        };
    }
    expr
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Tok {
    Ident,
    Not,
    And,
    Or,
    Ancestor,
    LParen,
    RParen,
}

fn tokenize(source: &str) -> Vec<(Tok, String)> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' => {}
            '!' => tokens.push((Tok::Not, String::new())),
            '&' => {
                if chars.next() == Some('&') {
                    tokens.push((Tok::And, String::new()));
                }
            }
            '|' => {
                if chars.next() == Some('|') {
                    tokens.push((Tok::Or, String::new()));
                }
            }
            '>' => tokens.push((Tok::Ancestor, String::new())),
            '(' => tokens.push((Tok::LParen, String::new())),
            ')' => tokens.push((Tok::RParen, String::new())),
            _ => {
                let mut ident = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_alphanumeric() || matches!(next, '_' | '-' | '.') {
                        ident.push(chars.next().unwrap_or_default());
                    } else {
                        break;
                    }
                }
                tokens.push((Tok::Ident, ident));
            }
        }
    }
    tokens
}

struct Parser<'a> {
    tokens: &'a [(Tok, String)],
    pos: usize,
    ok: bool,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<Tok> {
        self.tokens.get(self.pos).map(|(tok, _)| *tok)
    }

    fn or(&mut self) -> ContextDnf {
        let mut expr = self.and();
        while self.peek() == Some(Tok::Or) {
            self.pos += 1;
            expr = expr.or(&self.and());
        }
        expr
    }

    fn and(&mut self) -> ContextDnf {
        let mut expr = self.atom();
        while matches!(self.peek(), Some(Tok::And) | Some(Tok::Ancestor)) {
            self.pos += 1;
            expr = expr.and(&self.atom());
        }
        expr
    }

    fn atom(&mut self) -> ContextDnf {
        match self.peek() {
            Some(Tok::Not) => {
                self.pos += 1;
                match self.atom().terms.as_slice() {
                    // Negation only normalizes cleanly on a bare identifier.
                    [term] if term.len() == 1 => {
                        let (id, neg) = term.iter().next().cloned().unwrap_or_default();
                        ContextDnf {
                            terms: vec![BTreeSet::from([(id, !neg)])],
                            parsed: true,
                        }
                    }
                    _ => ContextDnf {
                        terms: Vec::new(),
                        parsed: false,
                    },
                }
            }
            Some(Tok::Ident) => {
                let (_, ident) = self.tokens[self.pos].clone();
                self.pos += 1;
                ContextDnf {
                    terms: vec![BTreeSet::from([(ident, false)])],
                    parsed: true,
                }
            }
            Some(Tok::LParen) => {
                self.pos += 1;
                let expr = self.or();
                if self.peek() == Some(Tok::RParen) {
                    self.pos += 1;
                } else {
                    self.ok = false;
                }
                expr
            }
            _ => {
                self.ok = false;
                ContextDnf {
                    terms: Vec::new(),
                    parsed: false,
                }
            }
        }
    }
}

/// A binding as the analyzer sees it.
pub struct BindingFact<'a> {
    pub command: &'a str,
    /// Canonical sequence string (keystrokes unparsed, space-joined).
    pub sequence: &'a str,
    pub context: Option<&'a str>,
    pub platform: PlatformSet,
}

/// Pairwise conflicts across the effective keymap for one platform.
/// Returns `(command, conflicts)` for every command with at least one.
pub fn analyze_conflicts<'a>(
    facts: &'a [BindingFact<'a>],
    platform: PlatformSet,
) -> Vec<(&'a str, Vec<Conflict>)> {
    let mut result: Vec<(&'a str, Vec<Conflict>)> = Vec::new();
    let applicable: Vec<&BindingFact> = facts
        .iter()
        .filter(|fact| fact.platform.applies(platform))
        .collect();
    for (i, a) in applicable.iter().enumerate() {
        for b in &applicable[i + 1..] {
            if a.sequence != b.sequence {
                continue;
            }
            if a.command == b.command {
                continue;
            }
            let (Some(ctx_a), Some(_ctx_b)) = (a.context, b.context) else {
                // Both None, or one None: a context-free binding overlaps
                // everything beneath it.
                let kind = if a.context.is_none() && b.context.is_none() {
                    ConflictKind::Hard
                } else {
                    ConflictKind::Shadowed
                };
                push(&mut result, a.command, b, kind);
                push(&mut result, b.command, a, kind);
                continue;
            };
            let dnf_a = parse_context(Some(ctx_a));
            let dnf_b = parse_context(b.context);
            if !dnf_a.parsed || !dnf_b.parsed {
                push(&mut result, a.command, b, ConflictKind::UnknownOverlap);
                push(&mut result, b.command, a, ConflictKind::UnknownOverlap);
                continue;
            }
            if !dnf_a.can_overlap(&dnf_b) {
                continue;
            }
            let kind = if dnf_a == dnf_b {
                ConflictKind::Hard
            } else if dnf_a.implies(&dnf_b) || dnf_b.implies(&dnf_a) {
                ConflictKind::Shadowed
            } else {
                ConflictKind::PartialOverlap
            };
            push(&mut result, a.command, b, kind);
            push(&mut result, b.command, a, kind);
        }
    }
    result
}

fn push<'a>(
    result: &mut Vec<(&'a str, Vec<Conflict>)>,
    command: &'a str,
    other: &'a BindingFact<'a>,
    kind: ConflictKind,
) {
    let index = match result.iter().position(|(id, _)| *id == command) {
        Some(index) => index,
        None => {
            result.push((command, Vec::new()));
            result.len() - 1
        }
    };
    if let Some((_, conflicts)) = result.get_mut(index) {
        conflicts.push(Conflict {
            kind,
            other: other.command.to_string(),
            other_context: other.context.map(str::to_string),
        });
    }
}
