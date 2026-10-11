/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use lopdf::{Dictionary, Document, Object};
use std::collections::HashMap;

use crate::image::to_base64;

/// Discovers embedded fonts and generates `@font-face` rules.
pub fn extract_embedded_fonts(
    doc: &Document,
    resources: &Dictionary,
) -> (HashMap<String, String>, String) {
    let mut font_family_map = HashMap::new();
    let mut font_css = String::new();

    let font_obj = match resources.get(b"Font") {
        Ok(obj) => obj,
        Err(_) => return (font_family_map, font_css),
    };

    let font_dict = match resolve_dict(doc, font_obj) {
        Some(dict) => dict,
        None => return (font_family_map, font_css),
    };

    for (name, obj) in font_dict.iter() {
        let font_tag = String::from_utf8_lossy(name).to_string();
        if let Some(single_font) = resolve_dict(doc, obj) {
            if let Some(font_data) = extract_font_file(doc, single_font) {
                let css_font_name = format!("PDF_Font_{font_tag}");
                let b64 = to_base64(&font_data);
                font_css.push_str(&format!(
                    "@font-face {{ font-family: '{css_font_name}'; src: url('data:font/opentype;base64,{b64}'); }}\n"
                ));
                font_family_map.insert(font_tag, css_font_name);
            }
        }
    }

    (font_family_map, font_css)
}

fn extract_font_file(doc: &Document, font_dict: &Dictionary) -> Option<Vec<u8>> {
    let descriptor_ref = font_dict.get(b"FontDescriptor").ok()?;
    let descriptor = resolve_dict(doc, descriptor_ref)?;

    let stream_obj = descriptor
        .get(b"FontFile2")
        .or_else(|_| descriptor.get(b"FontFile3"))
        .or_else(|_| descriptor.get(b"FontFile"))
        .ok()?;

    let stream = match stream_obj {
        Object::Reference(id) => doc.get_object(*id).and_then(|o| o.as_stream()).ok()?,
        Object::Stream(ref s) => s,
        _ => return None,
    };

    stream.decompressed_content().ok()
}

pub fn resolve_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Dictionary(ref d) => Some(d),
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        _ => None,
    }
}
