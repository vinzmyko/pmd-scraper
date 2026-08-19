//! String utils for JSON key gen and display names.

/// CP1252 mappings for 0x80..=0xFF. `\u{FFFD}` marks bytes with no CP1252
/// assignment. If these show up in output the ROM is using that slot for a
/// game glyph and needs an override here.
const HIGH: [char; 128] = [
    '€', '\u{FFFD}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{FFFD}', 'Ž',
    '\u{FFFD}', '\u{FFFD}', '\u{2018}', '\u{2019}', '“', '”', '•', '–', '—', '˜', '™', 'š', '›',
    'œ', '\u{FFFD}', 'ž', 'Ÿ', '\u{A0}', '¡', '¢', '£', '¤', '¥', '¦', '§', '¨', '©', 'ª', '«',
    '¬', '\u{AD}', '®', '¯', '°', '±', '²', '³', '´', 'µ', '¶', '·', '¸', '¹', 'º', '»', '¼', '½',
    '¾', '¿', 'À', 'Á', 'Â', 'Ã', 'Ä', 'Å', 'Æ', 'Ç', 'È', 'É', 'Ê', 'Ë', 'Ì', 'Í', 'Î', 'Ï', 'Ð',
    'Ñ', 'Ò', 'Ó', 'Ô', 'Õ', 'Ö', '×', 'Ø', 'Ù', 'Ú', 'Û', 'Ü', 'Ý', 'Þ', 'ß', 'à', 'á', 'â', 'ã',
    'ä', 'å', 'æ', 'ç', 'è', 'é', 'ê', 'ë', 'ì', 'í', 'î', 'ï', 'ð', 'ñ', 'ò', 'ó', 'ô', 'õ', 'ö',
    '÷', 'ø', 'ù', 'ú', 'û', 'ü', 'ý', 'þ', 'ÿ',
];

/// Decodes a PMD text-table string. Single-byte, ASCII-transparent.
pub fn decode_pmd(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b < 0x80 {
                b as char
            } else {
                HIGH[(b - 0x80) as usize]
            }
        })
        .collect()
}

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
        // Not a tag. Copy one full UTF-8 char
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }

    out.trim().to_string()
}

/// If a tag starts at `open`, returns the index just past its `]`.
fn tag_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut i = open + 1;

    // Tag name.
    let name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_uppercase() || bytes[i] == b'_') {
        i += 1;
    }
    if i == name_start {
        return None;
    }

    match bytes.get(i)? {
        b']' => Some(i + 1),
        b':' => bytes[i + 1..]
            .iter()
            .position(|&b| b == b']')
            .map(|p| i + 1 + p + 1),
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
