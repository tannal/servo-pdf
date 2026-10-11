/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use lopdf::content::Content;
use lopdf::{Dictionary, Object};
use std::collections::HashMap;
use std::sync::Arc;

use crate::encoding::cmap::parse_to_unicode_cmap;
use crate::encoding::win_ansi::parse_differences;
use crate::error::PdfError;
use crate::font::{extract_embedded_fonts, resolve_dict};
use crate::html_emitter::HtmlPageEmitter;
use crate::image::to_base64;
use crate::interpreter::interpret_page;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PdfSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone)]
pub struct PdfDocument {
    pub(crate) doc: Arc<lopdf::Document>,
}

impl PdfDocument {
    pub fn new(data: &[u8]) -> Result<Self, PdfError> {
        let doc = lopdf::Document::load_mem(data).map_err(PdfError::Parse)?;
        Ok(Self { doc: Arc::new(doc) })
    }

    pub fn empty() -> Self {
        Self {
            doc: Arc::new(lopdf::Document::new()),
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

    pub(crate) fn get_inherited_prop<'a>(
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
            return Err(PdfError::InvalidFormat("MediaBox requires 4 coordinates"));
        }

        let x1 = to_f64(&array[0]);
        let y1 = to_f64(&array[1]);
        let x2 = to_f64(&array[2]);
        let y2 = to_f64(&array[3]);

        Ok(PdfSize {
            width: (x2 - x1).abs(),
            height: (y2 - y1).abs(),
        })
    }

    pub(crate) fn load_page_cmaps(
        &self,
        page_id: lopdf::ObjectId,
    ) -> HashMap<String, HashMap<u16, String>> {
        let mut cmaps = HashMap::new();
        if let Ok(page_dict) = self.doc.get_dictionary(page_id) {
            if let Some(res_obj) = self.get_inherited_prop(page_dict, b"Resources") {
                if let Some(res_dict) = resolve_dict(&self.doc, res_obj) {
                    if let Ok(font_obj) = res_dict.get(b"Font") {
                        if let Some(font_dict) = resolve_dict(&self.doc, font_obj) {
                            for (name, obj) in font_dict.iter() {
                                let font_name = String::from_utf8_lossy(name).to_string();
                                if let Some(single_font) = resolve_dict(&self.doc, obj) {
                                    if let Some(map) = self.extract_to_unicode_stream(single_font) {
                                        cmaps.insert(font_name.clone(), map);
                                        continue;
                                    }
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

    pub(crate) fn load_page_differences(
        &self,
        page_id: lopdf::ObjectId,
    ) -> HashMap<String, HashMap<u8, char>> {
        let mut diffs = HashMap::new();
        if let Ok(page_dict) = self.doc.get_dictionary(page_id) {
            if let Some(res_obj) = self.get_inherited_prop(page_dict, b"Resources") {
                if let Some(res_dict) = resolve_dict(&self.doc, res_obj) {
                    if let Ok(font_obj) = res_dict.get(b"Font") {
                        if let Some(font_dict) = resolve_dict(&self.doc, font_obj) {
                            for (name, obj) in font_dict.iter() {
                                let font_name = String::from_utf8_lossy(name).to_string();
                                if let Some(single_font) = resolve_dict(&self.doc, obj) {
                                    if let Ok(enc_obj) = single_font.get(b"Encoding") {
                                        if let Some(enc_dict) = resolve_dict(&self.doc, enc_obj) {
                                            if let Ok(arr) = enc_dict
                                                .get(b"Differences")
                                                .and_then(|o| o.as_array())
                                            {
                                                diffs.insert(font_name, parse_differences(arr));
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
        diffs
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
    pub(crate) document: PdfDocument,
    pub(crate) page_id: lopdf::ObjectId,
    pub page_number: u32,
    pub width_pts: f64,
    pub height_pts: f64,
}

impl PdfPage {
    pub fn dimensions(&self) -> PdfSize {
        PdfSize {
            width: self.width_pts,
            height: self.height_pts,
        }
    }

    pub fn render_to_html(&self) -> Result<(String, String), PdfError> {
        let mut emitter = HtmlPageEmitter::new(self.width_pts, self.height_pts);
        let content_bytes = self.document.doc.get_page_content(self.page_id);
        let content = Content::decode(&content_bytes).map_err(PdfError::Parse)?;

        let font_cmaps = self.document.load_page_cmaps(self.page_id);
        let differences = self.document.load_page_differences(self.page_id);

        interpret_page(self, &content, &font_cmaps, &differences, &mut emitter)?;

        Ok(emitter.finish())
    }

    pub(crate) fn extract_xobject_image(&self, name_bytes: &[u8]) -> Option<String> {
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

fn to_f64(obj: &Object) -> f64 {
    match *obj {
        Object::Real(f) => f as f64,
        Object::Integer(i) => i as f64,
        _ => 0.0,
    }
}
