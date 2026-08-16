//! String utils for JSON key gen and display names.

/// Strips PMD presentation markup: `[FT:1]`, `[CS]`, `[CLUM_SET:9]`, etc.
/// Pattern: `[` + two uppercase letters + optional `:...` + `]`.
pub fn strip_tags(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'[' {
            if let Some(end) = tag_end(bytes, i) {
                i = end;
                continue;
            }
        }
        // Not a tag; copy one full UTF-8 char
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }

    out.trim().to_string()
}

/// If a tag starts at `open`, returns the index just past its `]`.
fn tag_end(bytes: &[u8], open: usize) -> Option<usize> {
    if open + 3 >= bytes.len() {
        return None;
    }
    if !bytes[open + 1].is_ascii_uppercase() || !bytes[open + 2].is_ascii_uppercase() {
        return None;
    }
    match bytes[open + 3] {
        b']' => Some(open + 4),
        b':' => bytes[open + 4..]
            .iter()
            .position(|&b| b == b']')
            .map(|p| open + 4 + p + 1),
        _ => None,
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// Snake cases a display name into a JSON key. Returns "unnamed" if empty.
pub fn to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    let mut pending_sep = false;
    // True when the last emitted char was a lowercase letter or digit, which
    // is the left-hand side of a camelCase boundary.
    let mut prev_was_lower = false;

    // Appends a separator if one is owed and we're not at a boundary already.
    fn sep(out: &mut String, pending: &mut bool) {
        if *pending && !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
        *pending = false;
    }

    for ch in s.chars() {
        match ch {
            // Dropped outright: no separator, no character.
            '\'' | '\u{2019}' => continue,

            '♂' | '♀' => {
                sep(&mut out, &mut pending_sep);
                if !out.is_empty() && !out.ends_with('_') {
                    out.push('_');
                }
                out.push(if ch == '♂' { 'm' } else { 'f' });
                prev_was_lower = true;
            }

            'é' | 'è' | 'ê' | 'É' | 'È' | 'Ê' => {
                sep(&mut out, &mut pending_sep);
                out.push('e');
                prev_was_lower = true;
            }

            c if c.is_ascii_alphanumeric() => {
                sep(&mut out, &mut pending_sep);
                if c.is_ascii_uppercase() && prev_was_lower && !out.ends_with('_') {
                    out.push('_');
                }
                out.push(c.to_ascii_lowercase());
                prev_was_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
            }

            _ => {
                pending_sep = true;
                prev_was_lower = false;
            }
        }
    }

    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed
    }
}
