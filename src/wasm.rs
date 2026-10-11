use crate::bindings;
use crate::bindings::exports::servo::pdf::viewer::{Document, Guest, GuestDocument};
use crate::PdfDocument;

pub struct DocumentWrapper {
    doc: PdfDocument,
}

impl GuestDocument for DocumentWrapper {
    fn new(bytes: Vec<u8>) -> Self {
        let doc = PdfDocument::new(&bytes).unwrap_or_else(|_| PdfDocument::empty());
        Self { doc }
    }

    fn page_count(&self) -> u32 {
        self.doc.page_count() as u32
    }

    fn page_dimensions(&self, page_index: u32) -> Result<(f32, f32), String> {
        let page = self.doc.page(page_index + 1).map_err(|e| e.to_string())?;
        let dims = page.dimensions();
        Ok((dims.width as f32, dims.height as f32))
    }

    fn render_page_html(&self, page_index: u32) -> Result<String, String> {
        let page = self.doc.page(page_index + 1).map_err(|e| e.to_string())?;
        let (html, _text) = page.render_to_html().map_err(|e| e.to_string())?;
        Ok(html)
    }

    fn extract_page_text(&self, page_index: u32) -> Result<String, String> {
        let page = self.doc.page(page_index + 1).map_err(|e| e.to_string())?;
        let (_html, text) = page.render_to_html().map_err(|e| e.to_string())?;
        Ok(text)
    }
}

struct Component;

impl Guest for Component {
    // Connect your struct to the WIT resource declaration
    type Document = DocumentWrapper;

    fn open(bytes: Vec<u8>) -> Result<Document, String> {
        let doc = PdfDocument::new(&bytes).map_err(|e| e.to_string())?;
        Ok(Document::new(DocumentWrapper { doc }))
    }
}

bindings::export!(Component with_types_in bindings);
