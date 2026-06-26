//! Shell glob expansion: `*`, `?`, `[abc]`, `[a-z]`.
//!
//! The globber is a *token-level* transformer.  It is called from
//! `run_pipeline` (src/shell/mod.rs) once per pipeline segment, after
//! arithmetic expansion (`$((…))`) and before the redirection parser
//! (`<`, `>`, `>>` consume tokens that look like filenames, so
//! expansion must happen first).
//!
//! Quoting: single-quoted strings bypass the globber entirely
//! (`'*.txt'` is the literal four characters).  Double-quoted
//! strings preserve whitespace but still expand globs
//! (`"/tmp/*.log"` works).  Backslash escapes a single metachar.
//!
//! No regex yet — see `src/shell/regex.rs` for `=~` and the
//! `regex` builtin.  Globs are a tiny language on purpose: they
//! have to be cheap enough to run on every keystroke.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// True if the token contains an unquoted glob metacharacter and is
/// not entirely quoted.  Used by `run_pipeline` to decide whether
/// the token is worth expanding.
pub fn has_metachar(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut saw_meta = false;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '\\' if !in_single => {
                i += 2;
                continue;
            } // skip escaped char
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '*' | '?' if !in_single => {
                saw_meta = true;
            }
            '[' if !in_single => {
                saw_meta = true;
            }
            _ => {}
        }
        i += 1;
    }
    saw_meta && !in_single
}

/// Strip a single layer of surrounding quotes ('…' or "…") and
/// process backslash escapes, returning the inner string.  Used
/// only by the test helpers; the real `expand` works on the
/// original string.
fn unquote(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        return String::from(&trimmed[1..trimmed.len() - 1]);
    }
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return String::from(&trimmed[1..trimmed.len() - 1]);
    }
    String::from(s)
}

/// Expand a single token.  If the token contains no metacharacters
/// it is returned unchanged.  If expansion yields zero matches the
/// token is still returned (POSIX behaviour: a non-matching glob
/// stays as-is, the command sees the literal pattern and errors).
/// On error, the original token is returned.
pub fn expand_token(token: &str) -> Vec<String> {
    if !has_metachar(token) {
        return vec![String::from(token)];
    }
    let pat = unquote(token);
    match expand_pattern(&pat) {
        Some(mut v) if !v.is_empty() => {
            v.sort();
            v.dedup();
            v
        }
        _ => vec![String::from(token)], // no match → pass through
    }
}

/// Pattern is of the form `<dir>/<rest>` where `<rest>` may itself
/// contain `/` if there are multiple path components.  We expand
/// left-to-right: at each `/`, the next component is matched
/// against the directory entries of the prefix.
fn expand_pattern(pat: &str) -> Option<Vec<String>> {
    // Special cases.
    if pat == "*" {
        return Some(glob_in_dir("/", "*", true));
    }
    // Split into prefix and tail.
    let (dir, rest) = match pat.rfind('/') {
        None => (String::from("."), pat),
        Some(0) => (String::from("/"), &pat[1..]),
        Some(i) => (String::from(&pat[..i]), &pat[i + 1..]),
    };
    if rest.contains('/') {
        // Multi-component glob: expand each directory level.
        return expand_recursive(&dir, rest);
    }
    Some(glob_in_dir(&dir, rest, true))
}

fn expand_recursive(dir: &str, rest: &str) -> Option<Vec<String>> {
    // Find next component in `rest`.
    let (comp, tail) = match rest.find('/') {
        None => (rest, ""),
        Some(i) => (&rest[..i], &rest[i + 1..]),
    };
    let entries = glob_in_dir(dir, comp, false);
    let mut out = Vec::new();
    for entry in entries {
        let full = if dir == "/" {
            format!("/{}", entry)
        } else {
            format!("{}/{}", dir, entry)
        };
        if tail.is_empty() {
            out.push(full);
        } else if let Some(more) = expand_recursive(&full, tail) {
            for m in more {
                out.push(m);
            }
        }
    }
    Some(out)
}

/// List entries in `dir` that match `pat` (a single-component
/// pattern, no slashes).  `prepend_dir` controls whether the
/// matching basenames are returned with the directory prefix
/// attached — true for the leaf level, false for intermediate
/// directory components (which need their own matching step).
fn glob_in_dir(dir: &str, pat: &str, prepend_dir: bool) -> Vec<String> {
    let abs = if dir == "." {
        let cwd = crate::shell::current_cwd();
        if cwd == "/" { String::from("/") } else { cwd }
    } else if dir.starts_with('/') {
        String::from(dir)
    } else {
        let cwd = crate::shell::current_cwd();
        if cwd == "/" {
            format!("/{}", dir)
        } else {
            format!("{}/{}", cwd, dir)
        }
    };
    let entries = match crate::vfs_contract::VfsContract::read_dir(&abs) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let matcher = Pattern::compile(pat);
    entries
        .into_iter()
        .map(|e| e.name)
        .filter(|n| !n.starts_with('.') || pat.starts_with('.')) // dotfiles opt-in
        .filter(|n| matcher.matches(n))
        .map(|n| {
            if prepend_dir {
                if abs == "/" {
                    format!("/{}", n)
                } else {
                    format!("{}/{}", abs, n)
                }
            } else {
                n
            }
        })
        .collect()
}

// -- Pattern matching: *  ?  [set]  ----------------------------------------

#[derive(Debug, Clone)]
enum Pattern {
    Literal(char),
    AnyChar,              // ?
    Star,                 // *
    CharClass(Vec<bool>), // [abc] / [a-z] — 256-entry bitmap over byte values
    End,
}

impl Pattern {
    fn compile(pat: &str) -> Compiled {
        let mut p = Compiled {
            tokens: Vec::new(),
            has_star: false,
        };
        let bytes = pat.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i] as char;
            match c {
                '*' => {
                    p.tokens.push(Pattern::Star);
                    p.has_star = true;
                    i += 1;
                }
                '?' => {
                    p.tokens.push(Pattern::AnyChar);
                    i += 1;
                }
                '[' => {
                    if let Some((cls, end)) = parse_class(&bytes[i + 1..]) {
                        p.tokens.push(Pattern::CharClass(cls));
                        i = i + 1 + end;
                    } else {
                        p.tokens.push(Pattern::Literal('['));
                        i += 1;
                    }
                }
                '\\' if i + 1 < bytes.len() => {
                    p.tokens.push(Pattern::Literal(bytes[i + 1] as char));
                    i += 2;
                }
                _ => {
                    p.tokens.push(Pattern::Literal(c));
                    i += 1;
                }
            }
        }
        p.tokens.push(Pattern::End);
        p
    }
}

struct Compiled {
    tokens: Vec<Pattern>,
    has_star: bool,
}

impl Compiled {
    /// Anchor: pattern must match the *entire* name (no leading/
    /// trailing substrings).  `*` does the substring work.
    fn matches(&self, s: &str) -> bool {
        if !self.has_star {
            // Fast path: pure literal / ? / [set] pattern.
            return match_one(&self.tokens, s.as_bytes(), 0, 0);
        }
        // Slow path with backtracking.
        let b = s.as_bytes();
        match_star(&self.tokens, b, 0, 0)
    }
}

fn match_one(p: &[Pattern], s: &[u8], mut pi: usize, mut si: usize) -> bool {
    while pi < p.len() {
        match p[pi] {
            Pattern::End => return si == s.len(),
            Pattern::Literal(c) => {
                if si >= s.len() || s[si] != c as u8 {
                    return false;
                }
                pi += 1;
                si += 1;
            }
            Pattern::AnyChar => {
                if si >= s.len() {
                    return false;
                }
                pi += 1;
                si += 1;
            }
            Pattern::CharClass(ref cls) => {
                if si >= s.len() || !cls[s[si] as usize] {
                    return false;
                }
                pi += 1;
                si += 1;
            }
            Pattern::Star => {
                // Handled by match_star; should not appear here.
                return match_star(p, s, pi, si);
            }
        }
    }
    si == s.len()
}

fn match_star(p: &[Pattern], s: &[u8], pi: usize, si: usize) -> bool {
    // p[pi] is Star.  Try every suffix.
    if pi + 1 >= p.len() {
        return true;
    } // trailing *
    for start in si..=s.len() {
        if match_one(p, s, pi + 1, start) {
            return true;
        }
    }
    false
}

/// Parse `[abc]` / `[a-z]` / `[!abc]` (negation).  Returns the
/// 256-bit bitmap and the number of source bytes consumed (not
/// counting the leading `[`).
fn parse_class(bytes: &[u8]) -> Option<(Vec<bool>, usize)> {
    if bytes.is_empty() || bytes[0] == b']' {
        return None;
    }
    let mut cls = alloc::vec![false; 256];
    let mut negate = false;
    let mut i = 0;
    if bytes[0] == b'!' {
        negate = true;
        i = 1;
    }
    let mut found_close = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b']' && i > 0 {
            found_close = true;
            break;
        }
        if i + 2 < bytes.len() && bytes[i + 1] == b'-' && bytes[i + 2] != b']' {
            let lo = c;
            let hi = bytes[i + 2];
            for b in lo..=hi {
                cls[b as usize] = true;
            }
            i += 3;
        } else {
            cls[c as usize] = true;
            i += 1;
        }
    }
    if !found_close {
        return None;
    }
    if negate {
        for b in cls.iter_mut() {
            *b = !*b;
        }
    }
    Some((cls, i + 1)) // +1 for the closing ']'
}

#[cfg(test)]
mod tests {
    use super::Pattern;
    fn m(pat: &str, s: &str) -> bool {
        Pattern::compile(pat).matches(s)
    }
    #[test]
    fn literal() {
        assert!(m("foo", "foo"));
        assert!(!m("foo", "bar"));
    }
    #[test]
    fn star_mid() {
        assert!(m("f*o", "foo"));
        assert!(!m("f*o", "f_o"));
    }
    #[test]
    fn star_alone() {
        assert!(m("*", "anything"));
    }
    #[test]
    fn question() {
        assert!(m("?ello", "hello"));
        assert!(!m("?ello", "ello"));
    }
    #[test]
    fn char_class() {
        assert!(m("[abc]", "b"));
        assert!(!m("[abc]", "d"));
    }
    #[test]
    fn char_range() {
        assert!(m("[a-z]", "m"));
        assert!(!m("[a-z]", "M"));
    }
    #[test]
    fn negate_class() {
        assert!(m("[!a-z]", "5"));
        assert!(!m("[!a-z]", "m"));
    }
    #[test]
    fn star_class() {
        assert!(m("[ab]*", "apple"));
        assert!(m("[ab]*", "banana"));
    }
}
