/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod cmap;
pub mod error;
pub mod font_provider;
pub mod html_emitter;
pub mod image;
pub mod interpreter;
pub mod matrix;

#[cfg(target_arch = "wasm32")]
mod custom_rng;

#[cfg(target_arch = "wasm32")]
#[allow(warnings, clippy::all)]
mod bindings;

#[cfg(target_arch = "wasm32")]
mod wasm;

use std::collections::HashMap;
use std::sync::Arc;

pub use error::PdfError;
use lopdf::content::Content;
use lopdf::{Dictionary, Object};

use crate::cmap::parse_to_unicode_cmap;
use crate::font_provider::{DefaultFontProvider, FontProvider};
use crate::html_emitter::HtmlPageEmitter;
use crate::interpreter::interpret_page_content;

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

fn to_f64(obj: &Object) -> f64 {
    match *obj {
        Object::Real(f) => f as f64,
        Object::Integer(i) => i as f64,
        _ => 0.0,
    }
}

fn resolve_dict<'a>(doc: &'a lopdf::Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Dictionary(ref d) => Some(d),
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        _ => None,
    }
}

#[derive(Clone)]
pub struct PdfDocument {
    doc: Arc<lopdf::Document>,
    font_provider: Arc<dyn FontProvider>,
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

    pub fn load_page_cmaps(
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
                                    // 1. Direct /ToUnicode
                                    if let Some(map) = self.extract_to_unicode_stream(single_font) {
                                        cmaps.insert(font_name.clone(), map);
                                        continue;
                                    }
                                    // 2. Type 0 /DescendantFonts
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
        let page_dict = self
            .document
            .doc
            .get_dictionary(self.page_id)
            .map_err(PdfError::Parse)?;
        let font_cmaps = self.document.load_page_cmaps(self.page_id);

        interpret_page_content(
            &self.document.doc,
            page_dict,
            &content,
            &font_cmaps,
            &mut emitter,
        );

        Ok(emitter.finish())
    }
}
