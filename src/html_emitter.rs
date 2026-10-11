/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub struct HtmlPageEmitter {
    page_width: f64,
    page_height: f64,
    html: String,
    current_fill_color: String,
    current_stroke_color: String,
    current_line_width: f64,
    current_font_size: f64,
    current_font_name: String,
    plain_text: String,
}

impl HtmlPageEmitter {
    pub fn new(page_width: f64, page_height: f64) -> Self {
        Self {
            page_width,
            page_height,
            html: String::with_capacity(4096),
            current_fill_color: "#0f172a".into(),
            current_stroke_color: "#1e293b".into(),
            current_line_width: 1.0,
            current_font_size: 10.0,
            current_font_name: "sans-serif".into(),
            plain_text: String::new(),
        }
    }

    pub fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) {
        self.current_fill_color = format!(
            "#{:02x}{:02x}{:02x}",
            (r.clamp(0.0, 1.0) * 255.0) as u8,
            (g.clamp(0.0, 1.0) * 255.0) as u8,
            (b.clamp(0.0, 1.0) * 255.0) as u8
        );
    }

    pub fn set_stroke_rgb(&mut self, r: f64, g: f64, b: f64) {
        self.current_stroke_color = format!(
            "#{:02x}{:02x}{:02x}",
            (r.clamp(0.0, 1.0) * 255.0) as u8,
            (g.clamp(0.0, 1.0) * 255.0) as u8,
            (b.clamp(0.0, 1.0) * 255.0) as u8
        );
    }

    pub fn set_line_width(&mut self, width: f64) {
        self.current_line_width = width.max(0.5);
    }

    pub fn set_font(&mut self, name: &str, size: f64) {
        self.current_font_name = name.to_string();
        self.current_font_size = size;
    }

    pub fn emit_rect(&mut self, x: f64, y: f64, w: f64, h: f64, stroke: bool, fill: bool) {
        let left = x;
        let top = (self.page_height - y - h).max(0.0);

        let mut style = format!(
            "position:absolute;left:{left:.1}pt;top:{top:.1}pt;width:{w:.1}pt;height:{h:.1}pt;box-sizing:border-box;"
        );

        if fill {
            style.push_str(&format!("background-color:{};", self.current_fill_color));
        }
        if stroke {
            style.push_str(&format!(
                "border:{:.1}pt solid {};border-radius:4pt;",
                self.current_line_width, self.current_stroke_color
            ));
        }

        self.html
            .push_str(&format!(r#"<div class="pdf-node" style="{style}"></div>"#));
    }

    pub fn emit_line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64) {
        let left = x1.min(x2);
        let right = x1.max(x2);
        let top_pdf = y1.max(y2);
        let bottom_pdf = y1.min(y2);

        let top = (self.page_height - top_pdf).max(0.0);
        let width = (right - left).max(0.5);
        let height = (top_pdf - bottom_pdf).max(0.5);

        if height <= 2.0 {
            let style = format!(
                "position:absolute;left:{left:.1}pt;top:{top:.1}pt;width:{width:.1}pt;height:0;border-bottom:{:.1}pt solid {};",
                self.current_line_width, self.current_stroke_color
            );
            self.html
                .push_str(&format!(r#"<div class="pdf-node" style="{style}"></div>"#));
        } else {
            let style = format!(
                "position:absolute;left:{left:.1}pt;top:{top:.1}pt;width:{width:.1}pt;height:{height:.1}pt;background-color:{};",
                self.current_stroke_color
            );
            self.html
                .push_str(&format!(r#"<div class="pdf-node" style="{style}"></div>"#));
        }
    }

    /// On `Do` (Image / Logo Paint)
    pub fn emit_image_placeholder(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let left = x;
        let top = (self.page_height - y - h).max(0.0);
        let style = format!(
            "position:absolute;left:{left:.1}pt;top:{top:.1}pt;width:{w:.1}pt;height:{h:.1}pt;border:1px dashed #cbd5e1;background:#f8fafc;border-radius:4pt;"
        );
        self.html
            .push_str(&format!(r#"<div class="pdf-node" style="{style}"></div>"#));
    }

    pub fn emit_text(&mut self, x: f64, y: f64, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        self.plain_text.push_str(trimmed);
        self.plain_text.push(' ');

        let left = x;
        // In PDF (x, y) is the font baseline. Subtracting font_size * 0.85 aligns with CSS top
        let top = (self.page_height - y - (self.current_font_size * 0.85)).max(0.0);

        let is_bold = self.current_font_name.to_lowercase().contains("bold");
        let weight_css = if is_bold { "font-weight:700;" } else { "" };

        let escaped = html_escape(text);
        self.html.push_str(&format!(
            r#"<span class="pdf-node" style="left:{left:.1}pt;top:{top:.1}pt;font-size:{:.1}pt;color:{};{};">{escaped}</span>"#,
            self.current_font_size, self.current_fill_color, weight_css
        ));
    }

    pub fn finish(self) -> (String, String) {
        (self.html, self.plain_text)
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
