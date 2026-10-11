/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod error;
pub mod font_provider;
pub mod html_emitter;

#[cfg(target_arch = "wasm32")]
mod custom_rng;

#[cfg(target_arch = "wasm32")]
#[allow(warnings, clippy::all)]
mod bindings;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use error::PdfError;
use lopdf::content::Content;
use lopdf::{Dictionary, Object};
use std::collections::HashMap;
use std::sync::Arc;

use crate::font_provider::{DefaultFontProvider, FontProvider};
use crate::html_emitter::HtmlPageEmitter;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PdfSize {
    pub width: f64,
    pub height: f64,
}

impl PdfSize {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// 2D Affine Transform Matrix: [a, b, c, d, e, f]
#[derive(Copy, Clone, Debug)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }

    fn multiply(&self, o: &Matrix) -> Self {
        Self {
            a: self.a * o.a + self.b * o.c,
            b: self.a * o.b + self.b * o.d,
            c: self.c * o.a + self.d * o.c,
            d: self.c * o.b + self.d * o.d,
            e: self.e * o.a + self.f * o.c + o.e,
            f: self.e * o.b + self.f * o.d + o.f,
        }
    }

    fn transform_point(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}

#[derive(Clone)]
pub struct PdfDocument {
    doc: Arc<lopdf::Document>,
    font_provider: Arc<dyn FontProvider>,
}

fn to_f64_opt(obj: &Object) -> Option<f64> {
    match *obj {
        Object::Real(f) => Some(f as f64),
        Object::Integer(i) => Some(i as f64),
        _ => None,
    }
}

fn to_f64(obj: &Object) -> f64 {
    to_f64_opt(obj).unwrap_or(0.0)
}

fn resolve_dict<'a>(doc: &'a lopdf::Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Dictionary(ref d) => Some(d),
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        _ => None,
    }
}

/// Pure Rust Base64 Encoder for Data URI Images (no external crate required)
fn to_base64(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARSET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARSET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Parses /ToUnicode CMap for Chinese, Japanese, and Western font subsets
fn parse_to_unicode_cmap(bytes: &[u8]) -> HashMap<u16, String> {
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

impl PdfDocument {
    pub fn new(data: &[u8]) -> Result<Self, PdfError> {
        Self::with_font_provider(data, Arc::new(DefaultFontProvider))
    }

    pub fn with_font_provider(
        data: &[u8],
        font_provider: Arc<dyn FontProvider>,
    ) -> Result<Self, PdfError> {
        let doc = lopdf::Document::load_mem(data).map_err(PdfError::Parse)?;
        Ok(Self {
            doc: Arc::new(doc),
            font_provider,
        })
    }

    pub fn empty() -> Self {
        Self {
            doc: Arc::new(lopdf::Document::new()),
            font_provider: Arc::new(DefaultFontProvider),
        }
    }

    pub fn page_count(&self) -> usize {
        self.doc.get_pages().len()
    }

    pub fn page(&self, page_number: u32) -> Result<PdfPage, PdfError> {
        let pages = self.doc.get_pages();
        let page_id = pages
            .get(&page_number)
            .copied()
            .ok_or(PdfError::PageNotFound(page_number))?;

        let media_box = self.get_page_media_box(page_id)?;

        Ok(PdfPage {
            document: self.clone(),
            page_id,
            page_number,
            width_pts: media_box.width,
            height_pts: media_box.height,
        })
    }

    fn get_inherited_prop<'a>(
        &'a self,
        mut dict: &'a Dictionary,
        key: &[u8],
    ) -> Option<&'a Object> {
        loop {
            if let Ok(obj) = dict.get(key) {
                return Some(obj);
            }
            if let Ok(parent_ref) = dict.get(b"Parent").and_then(|o| o.as_reference()) {
                if let Ok(parent_dict) = self.doc.get_dictionary(parent_ref) {
                    dict = parent_dict;
                    continue;
                }
            }
            break;
        }
        None
    }

    fn get_page_media_box(&self, page_id: lopdf::ObjectId) -> Result<PdfSize, PdfError> {
        let page_dict = self.doc.get_dictionary(page_id).map_err(PdfError::Parse)?;
        let media_box_obj = self
            .get_inherited_prop(page_dict, b"MediaBox")
            .ok_or(PdfError::MissingAttribute("MediaBox"))?;

        let array = match media_box_obj {
            Object::Array(ref a) => a,
            Object::Reference(id) => self
                .doc
                .get_object(*id)
                .and_then(|o| o.as_array())
                .map_err(PdfError::Parse)?,
            _ => return Err(PdfError::InvalidFormat("MediaBox must be an array")),
        };

        if array.len() < 4 {
            return Err(PdfError::InvalidFormat(
                "MediaBox array must have at least 4 numbers",
            ));
        }

        let x1 = to_f64(&array[0]);
        let y1 = to_f64(&array[1]);
        let x2 = to_f64(&array[2]);
        let y2 = to_f64(&array[3]);

        Ok(PdfSize::new((x2 - x1).abs(), (y2 - y1).abs()))
    }

    /// Load font CMaps from page or inherited catalog resources, checking both /Font and /DescendantFonts
    fn load_page_cmaps(&self, page_id: lopdf::ObjectId) -> HashMap<String, HashMap<u16, String>> {
        let mut cmaps = HashMap::new();
        if let Ok(page_dict) = self.doc.get_dictionary(page_id) {
            if let Some(res_obj) = self.get_inherited_prop(page_dict, b"Resources") {
                if let Some(res_dict) = resolve_dict(&self.doc, res_obj) {
                    if let Ok(font_obj) = res_dict.get(b"Font") {
                        if let Some(font_dict) = resolve_dict(&self.doc, font_obj) {
                            for (name, obj) in font_dict.iter() {
                                let font_name = String::from_utf8_lossy(name).to_string();
                                if let Some(single_font) = resolve_dict(&self.doc, obj) {
                                    // 1. Direct /ToUnicode
                                    if let Some(map) = self.extract_to_unicode_stream(single_font) {
                                        cmaps.insert(font_name.clone(), map);
                                        continue;
                                    }
                                    // 2. Look in /DescendantFonts (Type 0 / CIDFont)
                                    if let Ok(df_array) = single_font
                                        .get(b"DescendantFonts")
                                        .and_then(|o| o.as_array())
                                    {
                                        for df_ref in df_array {
                                            if let Some(df_dict) = resolve_dict(&self.doc, df_ref) {
                                                if let Some(map) =
                                                    self.extract_to_unicode_stream(df_dict)
                                                {
                                                    cmaps.insert(font_name.clone(), map);
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        cmaps
    }

    fn extract_to_unicode_stream(&self, dict: &Dictionary) -> Option<HashMap<u16, String>> {
        if let Ok(to_unicode) = dict.get(b"ToUnicode") {
            let stream_opt = match to_unicode {
                Object::Reference(id) => self.doc.get_object(*id).and_then(|o| o.as_stream()).ok(),
                Object::Stream(ref s) => Some(s),
                _ => None,
            };
            if let Some(stream) = stream_opt {
                if let Ok(decompressed) = stream.decompressed_content() {
                    return Some(parse_to_unicode_cmap(&decompressed));
                }
            }
        }
        None
    }
}

pub struct PdfPage {
    document: PdfDocument,
    page_id: lopdf::ObjectId,
    page_number: u32,
    pub width_pts: f64,
    pub height_pts: f64,
}

impl PdfPage {
    pub fn dimensions(&self) -> PdfSize {
        PdfSize::new(self.width_pts, self.height_pts)
    }

    pub fn render_to_html(&self) -> Result<(String, String), PdfError> {
        let mut emitter = HtmlPageEmitter::new(self.width_pts, self.height_pts);
        let content_bytes = self.document.doc.get_page_content(self.page_id);
        let content = Content::decode(&content_bytes).map_err(PdfError::Parse)?;
        let font_cmaps = self.document.load_page_cmaps(self.page_id);

        self.interpret_to_html(&content, &font_cmaps, &mut emitter)?;
        Ok(emitter.finish())
    }

    fn interpret_to_html(
        &self,
        content: &Content,
        cmaps: &HashMap<String, HashMap<u16, String>>,
        emitter: &mut HtmlPageEmitter,
    ) -> Result<(), PdfError> {
        let mut matrix_stack: Vec<Matrix> = Vec::new();
        let mut ctm = Matrix::identity();

        let mut text_matrix = Matrix::identity();
        let mut text_line_matrix = Matrix::identity();
        let mut active_font_name = String::new();

        let mut line_start: Option<(f64, f64)> = None;

        for op in &content.operations {
            match op.operator.as_str() {
                "q" => matrix_stack.push(ctm),
                "Q" => {
                    if let Some(prev) = matrix_stack.pop() {
                        ctm = prev;
                    }
                }
                "cm" => {
                    if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                        op.operands.get(2).and_then(to_f64_opt),
                        op.operands.get(3).and_then(to_f64_opt),
                        op.operands.get(4).and_then(to_f64_opt),
                        op.operands.get(5).and_then(to_f64_opt),
                    ) {
                        let m = Matrix { a, b, c, d, e, f };
                        ctm = m.multiply(&ctm);
                    }
                }

                "BT" => {
                    text_matrix = Matrix::identity();
                    text_line_matrix = Matrix::identity();
                }
                "ET" => {}

                "Tf" => {
                    if let (Some(font_name), Some(size)) = (
                        op.operands.get(0).and_then(|o| o.as_name().ok()),
                        op.operands.get(1).and_then(to_f64_opt),
                    ) {
                        active_font_name = String::from_utf8_lossy(font_name).to_string();
                        emitter.set_font(&active_font_name, size);
                    }
                }

                "Td" => {
                    if let (Some(tx), Some(ty)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                    ) {
                        let offset_m = Matrix {
                            a: 1.0,
                            b: 0.0,
                            c: 0.0,
                            d: 1.0,
                            e: tx,
                            f: ty,
                        };
                        text_line_matrix = offset_m.multiply(&text_line_matrix);
                        text_matrix = text_line_matrix;
                    }
                }

                "Tm" => {
                    if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                        op.operands.get(2).and_then(to_f64_opt),
                        op.operands.get(3).and_then(to_f64_opt),
                        op.operands.get(4).and_then(to_f64_opt),
                        op.operands.get(5).and_then(to_f64_opt),
                    ) {
                        text_matrix = Matrix { a, b, c, d, e, f };
                        text_line_matrix = text_matrix;
                    }
                }

                "rg" => {
                    if let (Some(r), Some(g), Some(b)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                        op.operands.get(2).and_then(to_f64_opt),
                    ) {
                        emitter.set_fill_rgb(r, g, b);
                    }
                }
                "RG" => {
                    if let (Some(r), Some(g), Some(b)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                        op.operands.get(2).and_then(to_f64_opt),
                    ) {
                        emitter.set_stroke_rgb(r, g, b);
                    }
                }
                "w" => {
                    if let Some(w) = op.operands.first().and_then(to_f64_opt) {
                        emitter.set_line_width(w);
                    }
                }

                "re" => {
                    if let (Some(x), Some(y), Some(w), Some(h)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                        op.operands.get(2).and_then(to_f64_opt),
                        op.operands.get(3).and_then(to_f64_opt),
                    ) {
                        let (abs_x, abs_y) = ctm.transform_point(x, y);
                        emitter.emit_rect(abs_x, abs_y, w, h, true, false);
                    }
                }

                "m" => {
                    if let (Some(x), Some(y)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                    ) {
                        line_start = Some(ctm.transform_point(x, y));
                    }
                }
                "l" => {
                    if let (Some(x), Some(y)) = (
                        op.operands.get(0).and_then(to_f64_opt),
                        op.operands.get(1).and_then(to_f64_opt),
                    ) {
                        let pt2 = ctm.transform_point(x, y);
                        if let Some(pt1) = line_start {
                            emitter.emit_line(pt1.0, pt1.1, pt2.0, pt2.1);
                            line_start = Some(pt2);
                        }
                    }
                }
                "S" | "s" => {
                    line_start = None;
                }

                // Paint XObject Image / Logo
                "Do" => {
                    if let Some(name_bytes) = op.operands.first().and_then(|o| o.as_name().ok()) {
                        let (left, bottom) = ctm.transform_point(0.0, 0.0);
                        let width = ctm.a.abs();
                        let height = ctm.d.abs();
                        if width > 1.0 && height > 1.0 {
                            if let Some(data_uri) = self.extract_xobject_image(name_bytes) {
                                emitter.emit_image(left, bottom, width, height, &data_uri);
                            } else {
                                emitter.emit_image_placeholder(left, bottom, width, height);
                            }
                        }
                    }
                }

                // Show Text
                "Tj" => {
                    if let Some(str_obj) = op.operands.first() {
                        if let Ok(bytes) = str_obj.as_str() {
                            let text = decode_text_with_cmap(bytes, &active_font_name, cmaps);
                            let combined = text_matrix.multiply(&ctm);
                            emitter.emit_text(combined.e, combined.f, &text);
                        }
                    }
                }

                // Show Text Array
                "TJ" => {
                    if let Some(array) = op.operands.first().and_then(|o| o.as_array().ok()) {
                        let mut full_text = String::new();
                        for item in array {
                            if let Ok(bytes) = item.as_str() {
                                full_text.push_str(&decode_text_with_cmap(
                                    bytes,
                                    &active_font_name,
                                    cmaps,
                                ));
                            }
                        }
                        if !full_text.is_empty() {
                            let combined = text_matrix.multiply(&ctm);
                            emitter.emit_text(combined.e, combined.f, &full_text);
                        }
                    }
                }

                _ => {}
            }
        }

        Ok(())
    }

    /// Extract an image XObject stream and convert to a data:image/... URI
    fn extract_xobject_image(&self, name_bytes: &[u8]) -> Option<String> {
        let page_dict = self.document.doc.get_dictionary(self.page_id).ok()?;
        let res_obj = self.document.get_inherited_prop(page_dict, b"Resources")?;
        let res_dict = resolve_dict(&self.document.doc, res_obj)?;
        let xobj_ref = res_dict.get(b"XObject").ok()?;
        let xobj_dict = resolve_dict(&self.document.doc, xobj_ref)?;

        let img_obj = xobj_dict.get(name_bytes).ok()?;
        let stream = match img_obj {
            Object::Reference(id) => self
                .document
                .doc
                .get_object(*id)
                .and_then(|o| o.as_stream())
                .ok()?,
            Object::Stream(ref s) => s,
            _ => return None,
        };

        // Check if JPEG (/DCTDecode)
        let is_jpeg = if let Ok(filter) = stream.dict.get(b"Filter") {
            match filter {
                Object::Name(ref n) => n == b"DCTDecode",
                Object::Array(ref a) => a
                    .iter()
                    .any(|o| o.as_name().map(|n| n == b"DCTDecode").unwrap_or(false)),
                _ => false,
            }
        } else {
            false
        };

        if is_jpeg {
            let b64 = to_base64(&stream.content);
            return Some(format!("data:image/jpeg;base64,{b64}"));
        }

        None
    }
}

/// Decodes bytes via ToUnicode CMap, with automatic 2-byte UTF-16BE (Chinese/CJK) detection
fn decode_text_with_cmap(
    bytes: &[u8],
    font_name: &str,
    cmaps: &HashMap<String, HashMap<u16, String>>,
) -> String {
    // 1. CMap Lookup
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

    // 3. Auto-detect 2-byte UTF-16BE without BOM (WPS / Word / Chinese government forms)
    // If even length and contains CJK ideograph ranges (0x4E00..=0x9FFF) or ASCII zero-high byte (0x00, 0x20..0x7E)
    if bytes.len() >= 2 && bytes.len() % 2 == 0 {
        let is_likely_utf16 = bytes.chunks_exact(2).any(|c| {
            let u = u16::from_be_bytes([c[0], c[1]]);
            (0x4E00..=0x9FFF).contains(&u) || (c[0] == 0x00 && c[1] >= 0x20 && c[1] <= 0x7E)
        });

        if is_likely_utf16 {
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

    // 5. Clean ASCII string
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
