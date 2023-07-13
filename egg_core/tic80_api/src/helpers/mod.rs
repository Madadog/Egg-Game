// Copyright (c) 2023 Adam Godwin <evilspamalt/at/gmail.com>
//
// This file is part of Egg Game - https://github.com/Madadog/Egg-Game/
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program. If not, see <https://www.gnu.org/licenses/>.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};


pub mod input_manager;

pub const SWEETIE_16: [[u8; 3]; 16] = [
    [26, 28, 44],    // #1a1c2c
    [93, 39, 93],    // #5d275d
    [177, 62, 83],   // #b13e53
    [239, 125, 87],  // #ef7d57
    [255, 205, 117], // #ffcd75
    [167, 240, 112], // #a7f070
    [56, 183, 100],  // #38b764
    [37, 113, 121],  // #257179
    [41, 54, 111],   // #29366f
    [59, 93, 201],   // #3b5dc9
    [65, 166, 246],  // #41a6f6
    [115, 239, 247], // #73eff7
    [244, 244, 244], // #f4f4f4
    [148, 176, 194], // #94b0c2
    [86, 108, 134],  // #566c86
    [51, 60, 87],    // #333c57
];
pub const NIGHT_16: [[u8; 3]; 16] = [
    [10, 10, 10],    // #0a0a0a
    [26, 28, 44],    // #1a1c2c
    [41, 54, 111],   // #29366f
    [59, 93, 201],   // #3b5dc9
    [65, 166, 246],  // #41a6f6
    [115, 239, 247], // #73eff7
    [167, 240, 112], // #a7f070
    [56, 183, 100],  // #38b764
    [37, 113, 121],  // #257179
    [41, 54, 111],   // #29366f
    [59, 93, 201],   // #3b5dc9
    [65, 166, 246],  // #41a6f6
    [244, 244, 244], // #f4f4f4
    [115, 239, 247], // #73eff7
    [148, 176, 194], // #94b0c2
    [86, 108, 134],  // #566c86
];
pub const B_W: [[u8; 3]; 16] = [
    [28, 24, 24],    // #1c1818
    [72, 64, 64],    // #484040
    [149, 141, 141], // #958d79
    [200, 200, 186], // #f6f6da
    [246, 246, 218], // #41a6f6
    [115, 239, 247], // #73eff7
    [167, 240, 112], // #a7f070
    [56, 183, 100],  // #38b764
    [37, 113, 121],  // #257179
    [41, 54, 111],   // #29366f
    [59, 93, 201],   // #3b5dc9
    [65, 166, 246],  // #41a6f6
    [244, 244, 244], // #f4f4f4
    [115, 239, 247], // #73eff7
    [148, 176, 194], // #94b0c2
    [86, 108, 134],  // #566c86
];
