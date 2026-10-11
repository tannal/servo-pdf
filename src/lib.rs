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
use lopdf::Object;
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

/// Parses a PDF /ToUnicode CMap stream into a lookup map (Glyph ID -> UTF-8 String)
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
            // Format: <srcCode> <destCode>
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let src_hex = parts[0].trim_matches(|c| c == '<' || c == '>');
                let dst_hex = parts[1].trim_matches(|c| c == '<' || c == '>');
                if let (Ok(src), Ok(dst)) = (
                    u16::from_str_radix(src_hex, 16),
                    u16::from_str_radix(dst_hex, 16),
                ) {
                    if let Some(ch) = char::from_u32(dst as u32) {
                        map.insert(src, ch.to_string());
                    }
                }
            }
        } else if in_bfrange {
            // Format: <start> <end> <destStart>
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

    fn get_page_media_box(&self, page_id: lopdf::ObjectId) -> Result<PdfSize, PdfError> {
        let page_dict = self.doc.get_dictionary(page_id).map_err(PdfError::Parse)?;
        let media_box_obj = page_dict
            .get(b"MediaBox")
            .map_err(|_| PdfError::MissingAttribute("MediaBox"))?;

        let array = media_box_obj.as_array().map_err(PdfError::Parse)?;
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

    /// Load all font ToUnicode CMaps for a given page
    fn load_page_cmaps(&self, page_id: lopdf::ObjectId) -> HashMap<String, HashMap<u16, String>> {
        let mut cmaps = HashMap::new();
        if let Ok(page_dict) = self.doc.get_dictionary(page_id) {
            if let Ok(res_dict) = page_dict.get(b"Resources").and_then(|o| o.as_dict()) {
                if let Ok(font_dict) = res_dict.get(b"Font").and_then(|o| o.as_dict()) {
                    for (name, obj) in font_dict.iter() {
                        let name_str = String::from_utf8_lossy(name).to_string();
                        if let Ok(font_id) = obj.as_reference() {
                            if let Ok(font_obj_dict) = self.doc.get_dictionary(font_id) {
                                if let Ok(to_unicode) = font_obj_dict.get(b"ToUnicode") {
                                    if let Ok(cmap_stream_id) = to_unicode.as_reference() {
                                        if let Ok(stream) = self
                                            .doc
                                            .get_object(cmap_stream_id)
                                            .and_then(|o| o.as_stream())
                                        {
                                            if let Ok(decompressed) = stream.decompressed_content()
                                            {
                                                cmaps.insert(
                                                    name_str,
                                                    parse_to_unicode_cmap(&decompressed),
                                                );
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
        // Graphics state stack (q / Q)
        let mut matrix_stack: Vec<Matrix> = Vec::new();
        let mut ctm = Matrix::identity();

        // Text state
        let mut text_matrix = Matrix::identity();
        let mut text_line_matrix = Matrix::identity();
        let mut active_font_name = String::new();

        let mut line_start: Option<(f64, f64)> = None;

        for op in &content.operations {
            match op.operator.as_str() {
                // Save Graphics State: q
                "q" => {
                    matrix_stack.push(ctm);
                }
                // Restore Graphics State: Q
                "Q" => {
                    if let Some(prev) = matrix_stack.pop() {
                        ctm = prev;
                    }
                }
                // Concat Matrix: a b c d e f cm
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

                // Begin / End Text
                "BT" => {
                    text_matrix = Matrix::identity();
                    text_line_matrix = Matrix::identity();
                }
                "ET" => {}

                // Set Font: /Name size Tf
                "Tf" => {
                    if let (Some(font_name), Some(size)) = (
                        op.operands.get(0).and_then(|o| o.as_name().ok()),
                        op.operands.get(1).and_then(to_f64_opt),
                    ) {
                        active_font_name = String::from_utf8_lossy(font_name).to_string();
                        emitter.set_font(&active_font_name, size);
                    }
                }

                // Move Text: tx ty Td
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

                // Set Text Matrix: a b c d e f Tm
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

                // Colors & Line styles
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

                // Rectangle: x y w h re
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

                // Path lines: m, l
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

                // Show Text: (str) Tj
                "Tj" => {
                    if let Some(str_obj) = op.operands.first() {
                        if let Ok(bytes) = str_obj.as_str() {
                            let text = decode_text_with_cmap(bytes, &active_font_name, cmaps);
                            let combined = text_matrix.multiply(&ctm);
                            emitter.emit_text(combined.e, combined.f, &text);
                        }
                    }
                }

                // Show Text Array: [ (str) offset (str) ] TJ
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
}

/// Decodes bytes via active font's ToUnicode CMap; falls back to UTF-8 / Latin1
fn decode_text_with_cmap(
    bytes: &[u8],
    font_name: &str,
    cmaps: &HashMap<String, HashMap<u16, String>>,
) -> String {
    if let Some(cmap) = cmaps.get(font_name) {
        let mut out = String::new();
        // 2-byte font glyph codes
        if bytes.len() >= 2 && bytes.len() % 2 == 0 {
            for chunk in bytes.chunks_exact(2) {
                let code = u16::from_be_bytes([chunk[0], chunk[1]]);
                if let Some(s) = cmap.get(&code) {
                    out.push_str(s);
                } else if let Some(ch) = char::from_u32(code as u32) {
                    out.push(ch);
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        // 1-byte font glyph codes
        for &b in bytes {
            let code = b as u16;
            if let Some(s) = cmap.get(&code) {
                out.push_str(s);
            } else {
                out.push(b as char);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    // Fallback: standard UTF-8 or Latin-1
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| bytes.iter().map(|&b| b as char).collect())
}
