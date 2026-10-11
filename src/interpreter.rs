/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use lopdf::content::Content;
use lopdf::Object;
use std::collections::HashMap;

use crate::document::PdfPage;
use crate::encoding::decode_pdf_text;
use crate::error::PdfError;
use crate::html_emitter::HtmlPageEmitter;
use crate::matrix::Matrix;

fn to_f64_opt(obj: &Object) -> Option<f64> {
    match *obj {
        Object::Real(f) => Some(f as f64),
        Object::Integer(i) => Some(i as f64),
        _ => None,
    }
}

pub fn interpret_page(
    page: &PdfPage,
    content: &Content,
    cmaps: &HashMap<String, HashMap<u16, String>>,
    differences: &HashMap<String, HashMap<u8, char>>,
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
                    // Skip full-page background clipping rectangles
                    if !(w >= page.width_pts - 2.0 && h >= page.height_pts - 2.0) {
                        let (abs_x, abs_y) = ctm.transform_point(x, y);
                        emitter.emit_rect(abs_x, abs_y, w, h, true, false);
                    }
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

            "Do" => {
                if let Some(name_bytes) = op.operands.first().and_then(|o| o.as_name().ok()) {
                    let (left, bottom) = ctm.transform_point(0.0, 0.0);
                    let width = ctm.a.abs();
                    let height = ctm.d.abs();
                    if width > 1.0 && height > 1.0 {
                        if let Some(data_uri) = page.extract_xobject_image(name_bytes) {
                            emitter.emit_image(left, bottom, width, height, &data_uri);
                        }
                    }
                }
            }

            "Tj" => {
                if let Some(str_obj) = op.operands.first() {
                    if let Ok(bytes) = str_obj.as_str() {
                        let text = decode_pdf_text(bytes, &active_font_name, cmaps, differences);
                        let combined = text_matrix.multiply(&ctm);
                        emitter.emit_text(combined.e, combined.f, &text);
                    }
                }
            }

            "TJ" => {
                if let Some(array) = op.operands.first().and_then(|o| o.as_array().ok()) {
                    let mut full_text = String::new();
                    for item in array {
                        match item {
                            Object::String(bytes, _) => {
                                full_text.push_str(&decode_pdf_text(
                                    bytes,
                                    &active_font_name,
                                    cmaps,
                                    differences,
                                ));
                            }
                            Object::Integer(n) => {
                                // Large displacement in TJ indicates a space or column gap
                                if *n < -180 {
                                    full_text.push(' ');
                                }
                            }
                            Object::Real(f) => {
                                if *f < -180.0 {
                                    full_text.push(' ');
                                }
                            }
                            _ => {}
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
