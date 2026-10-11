/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

#[derive(Clone, Debug)]
pub enum PathCommand {
    Rect { x: f64, y: f64, w: f64, h: f64 },
    MoveTo { x: f64, y: f64 },
    LineTo { x: f64, y: f64 },
    Close,
}

#[derive(Default)]
pub struct PathBuilder {
    pub commands: Vec<PathCommand>,
}

impl PathBuilder {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.commands.push(PathCommand::Rect { x, y, w, h });
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.commands.push(PathCommand::MoveTo { x, y });
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        self.commands.push(PathCommand::LineTo { x, y });
    }

    pub fn close(&mut self) {
        self.commands.push(PathCommand::Close);
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }
}
