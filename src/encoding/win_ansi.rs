/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::collections::HashMap;

/// Maps standard Adobe glyph names to Unicode characters.
pub fn glyph_name_to_unicode(name: &str) -> Option<char> {
    match name {
        "space" => Some(' '),
        "exclam" => Some('!'),
        "quotedbl" => Some('"'),
        "numbersign" => Some('#'),
        "dollar" => Some('$'),
        "percent" => Some('%'),
        "ampersand" => Some('&'),
        "quoteright" => Some('\''),
        "parenleft" => Some('('),
        "parenright" => Some(')'),
        "asterisk" => Some('*'),
        "plus" => Some('+'),
        "comma" => Some(','),
        "hyphen" | "minus" => Some('-'),
        "period" => Some('.'),
        "slash" => Some('/'),
        "colon" => Some(':'),
        "semicolon" => Some(';'),
        "equal" => Some('='),
        "question" => Some('?'),
        "at" => Some('@'),
        "bracketleft" => Some('['),
        "backslash" => Some('\\'),
        "bracketright" => Some(']'),
        "underscore" => Some('_'),
        _ if name.len() == 1 => name.chars().next(),
        _ => None,
    }
}

/// Parses a PDF /Differences array into a mapping table.
pub fn parse_differences(array: &[lopdf::Object]) -> HashMap<u8, char> {
    let mut map = HashMap::new();
    let mut current_code = 0u8;

    for item in array {
        match item {
            lopdf::Object::Integer(code) => {
                current_code = *code as u8;
            }
            lopdf::Object::Name(name) => {
                let name_str = String::from_utf8_lossy(name);
                if let Some(ch) = glyph_name_to_unicode(&name_str) {
                    map.insert(current_code, ch);
                }
                current_code = current_code.saturating_add(1);
            }
            _ => {}
        }
    }

    map
}
