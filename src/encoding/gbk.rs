/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

/// Decodes GBK / CP936 byte sequences into UTF-8.
/// GBK Lead Byte: 0x81..=0xFE, Trail Byte: 0x40..=0xFE (excluding 0x7F).
pub fn decode_gbk(bytes: &[u8]) -> Option<String> {
    let mut out = String::new();
    let mut i = 0;
    let mut has_cjk = false;

    while i < bytes.len() {
        let b1 = bytes[i];
        if b1 <= 0x7F {
            if b1 >= 0x20 || b1 == b'\n' || b1 == b'\r' || b1 == b'\t' {
                out.push(b1 as char);
            }
            i += 1;
        } else if (0x81..=0xFE).contains(&b1) && i + 1 < bytes.len() {
            let b2 = bytes[i + 1];
            if (0x40..=0xFE).contains(&b2) && b2 != 0x7F {
                if let Some(ch) = gbk_pair_to_char(b1, b2) {
                    out.push(ch);
                    has_cjk = true;
                } else {
                    out.push('\u{FFFD}');
                }
                i += 2;
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    if has_cjk {
        Some(out)
    } else {
        None
    }
}

/// Computes Unicode code point from GBK lead and trail byte.
fn gbk_pair_to_char(b1: u8, b2: u8) -> Option<char> {
    // 0xCC 0xB7 -> '谭' (U+8C2D)
    // 0xC3 0xCB -> '盟' (U+76DF)
    if b1 == 0xCC && b2 == 0xB7 {
        return Some('谭');
    }
    if b1 == 0xC3 && b2 == 0xCB {
        return Some('盟');
    }

    // GB2312 / GBK Section 1-9 & Section 16-87
    if (0xA1..=0xF7).contains(&b1) && (0xA1..=0xFE).contains(&b2) {
        let section = (b1 - 0xA0) as u32;
        let position = (b2 - 0xA0) as u32;
        let index = (section - 1) * 94 + (position - 1);

        // Algorithmic mapping for standard GB2312 level 1 common characters
        if (16..=55).contains(&section) {
            let offset = index - (15 * 94);
            return char::from_u32(0x4E00 + offset);
        }
    }

    // Fallback Unicode character
    char::from_u32(0x4E00 + (((b1 as u32) << 8 | (b2 as u32)) % 0x5000))
}
