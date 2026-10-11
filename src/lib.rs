/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub mod document;
pub mod encoding;
pub mod error;
pub mod font;
pub mod html_emitter;
pub mod image;
pub mod interpreter;
pub mod matrix;
pub mod path;

#[cfg(target_arch = "wasm32")]
mod custom_rng;

#[cfg(target_arch = "wasm32")]
#[allow(warnings, clippy::all)]
mod bindings;

#[cfg(target_arch = "wasm32")]
mod wasm;

pub use document::{PdfDocument, PdfPage, PdfSize};
pub use error::PdfError;
