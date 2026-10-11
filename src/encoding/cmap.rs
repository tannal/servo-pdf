/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::HashMap;

/// Parses /ToUnicode CMap stream.
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
