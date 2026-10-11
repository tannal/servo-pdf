/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod cmap;
pub mod gbk;
pub mod win_ansi;

use std::collections::HashMap;

/// Unified text decoding pipeline.
pub fn decode_pdf_text(
    bytes: &[u8],
    font_name: &str,
    cmaps: &HashMap<String, HashMap<u16, String>>,
    differences: &HashMap<String, HashMap<u8, char>>,
) -> String {
    // 1. ToUnicode CMap
    if let Some(cmap) = cmaps.get(font_name) {
        let mut out = String::new();
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
        for &b in bytes {
            if let Some(s) = cmap.get(&(b as u16)) {
                out.push_str(s);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    // 2. /Differences array mapping
    if let Some(diff) = differences.get(font_name) {
        let mut out = String::new();
        for &b in bytes {
            if let Some(&ch) = diff.get(&b) {
                out.push(ch);
            } else if b >= 0x20 && b <= 0x7E {
                out.push(b as char);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    // 3. Explicit UTF-16BE with BOM
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&u16s);
    }

    // 4. GBK / CP936 for Chinese government / WPS / Word forms
    if let Some(s) = gbk::decode_gbk(bytes) {
        return s;
    }

    // 5. UTF-16BE heuristic
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

    // 6. Standard UTF-8
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }

    // 7. Clean ASCII
    bytes
        .iter()
        .filter_map(|&b| {
            if (0x20..=0x7E).contains(&b) {
                Some(b as char)
            } else {
                None
            }
        })
        .collect()
}
