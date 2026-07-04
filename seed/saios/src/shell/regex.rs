#[derive(Copy, Clone)]
enum Atom<'a> {
    Byte(u8),
    Any,
    Class { negated: bool, body: &'a [u8] },
}

pub fn is_match(pattern: &str, text: &str) -> Result<bool, &'static str> {
    validate_pattern(pattern)?;

    let p = pattern.as_bytes();
    let t = text.as_bytes();

    if let Some(rest) = p.strip_prefix(b"^") {
        return Ok(match_here(rest, t));
    }

    let mut idx = 0usize;
    loop {
        if match_here(p, &t[idx..]) {
            return Ok(true);
        }
        if idx >= t.len() {
            break;
        }
        idx = idx.saturating_add(1);
    }

    Ok(false)
}

fn validate_pattern(pattern: &str) -> Result<(), &'static str> {
    let p = pattern.as_bytes();
    let mut i = 0usize;
    let mut prev_was_atom = false;

    if p.first() == Some(&b'^') {
        i = 1;
    }

    while i < p.len() {
        if p[i] == b'$' && i + 1 == p.len() {
            return Ok(());
        }

        if is_quantifier(p[i]) {
            if !prev_was_atom {
                return Err("grep: invalid regex (quantifier without atom)");
            }
            prev_was_atom = false;
            i = i.saturating_add(1);
            continue;
        }

        let atom_len = parse_atom(&p[i..])
            .map(|(_, n)| n)
            .ok_or("grep: invalid regex (bad atom)")?;
        prev_was_atom = true;
        i = i.saturating_add(atom_len);
    }

    Ok(())
}

fn match_here(pattern: &[u8], text: &[u8]) -> bool {
    if pattern.is_empty() {
        return true;
    }

    if pattern[0] == b'$' && pattern.len() == 1 {
        return text.is_empty();
    }

    let Some((atom, atom_len)) = parse_atom(pattern) else {
        return false;
    };

    let rest = &pattern[atom_len..];
    if let Some(&q) = rest.first()
        && is_quantifier(q)
    {
        let after = &rest[1..];
        return match q {
            b'*' => match_repeat(atom, 0, after, text),
            b'+' => match_repeat(atom, 1, after, text),
            b'?' => {
                if match_here(after, text) {
                    true
                } else if !text.is_empty() && atom_matches(atom, text[0]) {
                    match_here(after, &text[1..])
                } else {
                    false
                }
            }
            _ => false,
        };
    }

    if !text.is_empty() && atom_matches(atom, text[0]) {
        return match_here(rest, &text[1..]);
    }

    false
}

fn match_repeat(atom: Atom<'_>, min_count: usize, rest: &[u8], text: &[u8]) -> bool {
    let mut consumed = 0usize;
    while consumed < min_count {
        if consumed >= text.len() || !atom_matches(atom, text[consumed]) {
            return false;
        }
        consumed = consumed.saturating_add(1);
    }

    let mut max = consumed;
    while max < text.len() && atom_matches(atom, text[max]) {
        max = max.saturating_add(1);
    }

    let mut i = max;
    loop {
        if i < consumed {
            return false;
        }
        if match_here(rest, &text[i..]) {
            return true;
        }
        if i == 0 {
            return false;
        }
        i = i.saturating_sub(1);
    }
}

fn parse_atom(pattern: &[u8]) -> Option<(Atom<'_>, usize)> {
    let first = *pattern.first()?;
    match first {
        b'.' => Some((Atom::Any, 1)),
        b'\\' => {
            let ch = *pattern.get(1)?;
            Some((Atom::Byte(ch), 2))
        }
        b'[' => {
            let (negated, body, len) = parse_class(pattern)?;
            Some((Atom::Class { negated, body }, len))
        }
        b'*' | b'+' | b'?' => None,
        ch => Some((Atom::Byte(ch), 1)),
    }
}

fn parse_class(pattern: &[u8]) -> Option<(bool, &[u8], usize)> {
    if pattern.first().copied() != Some(b'[') {
        return None;
    }

    let mut i = 1usize;
    let mut negated = false;
    if pattern.get(i).copied() == Some(b'^') {
        negated = true;
        i = i.saturating_add(1);
    }

    let body_start = i;
    let mut has_member = false;

    while i < pattern.len() {
        match pattern[i] {
            b'\\' => {
                i = i.saturating_add(1);
                if i >= pattern.len() {
                    return None;
                }
                has_member = true;
                i = i.saturating_add(1);
            }
            b']' => {
                if !has_member {
                    return None;
                }
                let body = &pattern[body_start..i];
                return Some((negated, body, i.saturating_add(1)));
            }
            _ => {
                has_member = true;
                i = i.saturating_add(1);
            }
        }
    }

    None
}

fn is_quantifier(ch: u8) -> bool {
    ch == b'*' || ch == b'+' || ch == b'?'
}

fn atom_matches(atom: Atom<'_>, ch: u8) -> bool {
    match atom {
        Atom::Byte(b) => b == ch,
        Atom::Any => true,
        Atom::Class { negated, body } => {
            let contains = class_contains(body, ch);
            if negated { !contains } else { contains }
        }
    }
}

fn class_contains(body: &[u8], ch: u8) -> bool {
    let mut i = 0usize;
    while i < body.len() {
        let Some((first, first_len)) = parse_class_char(body, i) else {
            return false;
        };
        i = i.saturating_add(first_len);

        if i + 1 < body.len() && body[i] == b'-' {
            let Some((last, last_len)) = parse_class_char(body, i + 1) else {
                return false;
            };
            let lo = core::cmp::min(first, last);
            let hi = core::cmp::max(first, last);
            if ch >= lo && ch <= hi {
                return true;
            }
            i = i.saturating_add(1 + last_len);
            continue;
        }

        if ch == first {
            return true;
        }
    }
    false
}

fn parse_class_char(body: &[u8], at: usize) -> Option<(u8, usize)> {
    let ch = *body.get(at)?;
    if ch == b'\\' {
        Some((*body.get(at + 1)?, 2))
    } else {
        Some((ch, 1))
    }
}
