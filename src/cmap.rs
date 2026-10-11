use std::collections::HashMap;

/// Parses a /ToUnicode CMap stream into Glyph ID -> Unicode mapping
pub fn parse_to_unicode_cmap(bytes: &[u8]) -> HashMap<u16, String> {
    let mut map = HashMap::new();
    let text = String::from_utf8_lossy(bytes);

    let mut in_bfchar = false;
    let mut in_bfrange = false;

    for line in text.lines() {
        let line = line.trim();
        if line.contains("beginbfchar") {
            in_bfchar = true;
            continue;
        }
        if line.contains("endbfchar") {
            in_bfchar = false;
            continue;
        }
        if line.contains("beginbfrange") {
            in_bfrange = true;
            continue;
        }
        if line.contains("endbfrange") {
            in_bfrange = false;
            continue;
        }

        if in_bfchar {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let src_hex = parts[0].trim_matches(|c| c == '<' || c == '>');
                let dst_hex = parts[1].trim_matches(|c| c == '<' || c == '>');
                if let Ok(src) = u16::from_str_radix(src_hex, 16) {
                    let mut decoded = String::new();
                    for chunk in dst_hex.as_bytes().chunks(4) {
                        if let Ok(s) = std::str::from_utf8(chunk) {
                            if let Ok(u) = u16::from_str_radix(s, 16) {
                                if let Some(ch) = char::from_u32(u as u32) {
                                    decoded.push(ch);
                                }
                            }
                        }
                    }
                    if !decoded.is_empty() {
                        map.insert(src, decoded);
                    }
                }
            }
        } else if in_bfrange {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let start_hex = parts[0].trim_matches(|c| c == '<' || c == '>');
                let end_hex = parts[1].trim_matches(|c| c == '<' || c == '>');
                let dst_hex = parts[2].trim_matches(|c| c == '<' || c == '>');
                if let (Ok(start), Ok(end), Ok(dst)) = (
                    u16::from_str_radix(start_hex, 16),
                    u16::from_str_radix(end_hex, 16),
                    u16::from_str_radix(dst_hex, 16),
                ) {
                    let mut cur_dst = dst as u32;
                    for code in start..=end {
                        if let Some(ch) = char::from_u32(cur_dst) {
                            map.insert(code, ch.to_string());
                        }
                        cur_dst += 1;
                    }
                }
            }
        }
    }

    map
}

/// Normalizes font names (e.g. "/F1" -> "F1")
pub fn normalize_font_name(name: &str) -> &str {
    name.trim_start_matches('/')
}

/// Decodes text bytes through CMap, UTF-16BE (Chinese/WPS/Word), or WinAnsi
pub fn decode_text(
    bytes: &[u8],
    font_name: &str,
    cmaps: &HashMap<String, HashMap<u16, String>>,
) -> String {
    let key = normalize_font_name(font_name);

    // 1. Check font's ToUnicode CMap
    if let Some(cmap) = cmaps.get(key).or_else(|| cmaps.get(font_name)) {
        let mut out = String::new();
        // Try 2-byte glyph codes
        if bytes.len() >= 2 {
            for chunk in bytes.chunks_exact(2) {
                let code = u16::from_be_bytes([chunk[0], chunk[1]]);
                if let Some(s) = cmap.get(&code) {
                    out.push_str(s);
                } else if let Some(ch) = char::from_u32(code as u32) {
                    if !ch.is_control() {
                        out.push(ch);
                    }
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        // Try 1-byte glyph codes
        for &b in bytes {
            let code = b as u16;
            if let Some(s) = cmap.get(&code) {
                out.push_str(s);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    // 2. Explicit UTF-16BE with BOM
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }

    // 3. Chinese/CJK UTF-16BE without BOM (WPS / Word / Chinese government forms)
    if bytes.len() >= 2 && bytes.len() % 2 == 0 {
        let is_utf16 = bytes.chunks_exact(2).any(|c| {
            let u = u16::from_be_bytes([c[0], c[1]]);
            (0x4E00..=0x9FFF).contains(&u) || (c[0] == 0x00 && c[1] >= 0x20 && c[1] <= 0x7E)
        });

        if is_utf16 {
            let u16s: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            let decoded = String::from_utf16_lossy(&u16s);
            if !decoded.trim().is_empty() {
                return decoded;
            }
        }
    }

    // 4. Standard UTF-8 fallback
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }

    // 5. Windows-1252 / WinAnsi fallback (standard Western contracts)
    bytes
        .iter()
        .filter_map(|&b| {
            if b >= 0x20 && b <= 0x7E {
                Some(b as char)
            } else {
                None
            }
        })
        .collect()
}
