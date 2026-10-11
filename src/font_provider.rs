/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

pub trait FontProvider: Send + Sync {
    fn fallback_font(&self) -> Option<Vec<u8>>;

    fn match_family(&self, _family_name: &str) -> Option<Vec<u8>> {
        self.fallback_font()
    }
}

#[derive(Default, Clone)]
pub struct DefaultFontProvider;

impl FontProvider for DefaultFontProvider {
    fn fallback_font(&self) -> Option<Vec<u8>> {
        #[cfg(target_os = "linux")]
        {
            std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf")
                .or_else(|_| {
                    std::fs::read("/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf")
                })
                .or_else(|_| std::fs::read("/usr/share/fonts/TTF/DejaVuSans.ttf"))
                .ok()
        }
        #[cfg(target_os = "windows")]
        {
            std::fs::read("C:\\Windows\\Fonts\\arial.ttf")
                .or_else(|_| std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf"))
                .ok()
        }
        #[cfg(target_os = "macos")]
        {
            std::fs::read("/System/Library/Fonts/Helvetica.ttc")
                .or_else(|_| std::fs::read("/Library/Fonts/Arial.ttf"))
                .ok()
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            None
        }
    }
}
