/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#[derive(Debug)]
pub enum PdfError {
    Parse(lopdf::Error),
    PageNotFound(u32),
    MissingAttribute(&'static str),
    InvalidFormat(&'static str),
    InvalidDimensions,
}

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfError::Parse(e) => write!(f, "PDF parse error: {e:?}"),
            PdfError::PageNotFound(n) => write!(f, "Page {n} not found"),
            PdfError::MissingAttribute(attr) => write!(f, "Missing attribute: {attr}"),
            PdfError::InvalidFormat(msg) => write!(f, "Invalid PDF format: {msg}"),
            PdfError::InvalidDimensions => write!(f, "Invalid page dimensions"),
        }
    }
}

impl std::error::Error for PdfError {}

impl From<lopdf::Error> for PdfError {
    fn from(err: lopdf::Error) -> Self {
        PdfError::Parse(err)
    }
}
