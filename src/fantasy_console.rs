use std::collections::HashMap;

use bevy::prelude::{info, Image};
use egg_core::{
    data::sound::music::MusicTrack,
    gamestate::EggInput,
    rand::Lcg64Xsh32,
    system::{ConsoleApi, EggMemory, GameMap, MapLayer, SyncHelper},
    tic80_api::{
        core::{Flip, MouseInput, SfxOptions, StaticSpriteOptions},
        helpers::SWEETIE_16,
    },
};
use tiny_skia::{
    Color, FillRule, IntSize, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform,
};

use crate::tiled::TiledMap;

use self::drawing::{array_to_colour, IndexedImage};

mod drawing;

pub struct FantasyConsole {
    screen: Pixmap,
    overlay_screen: Pixmap,
    _output_screen: Pixmap,

    font: Pixmap,
    sprites: Pixmap,
    indexed_sprites: IndexedImage,
    maps: Vec<GameMap>,
    files: HashMap<String, Vec<u8>>,

    vbank: usize,
    palette_size: usize,
    palette: Vec<[u8; 3]>,
    palette_map: Vec<usize>,
    blit_segment: u8,
    screen_offset: [i8; 2],
    sprite_flags: Vec<u8>,
    music: Option<(MusicTrack, bool)>,
    memory: EggMemory,
    sounds: Vec<(String, SfxOptions)>,
    input: EggInput,
    rng: Lcg64Xsh32,
    sync_helper: SyncHelper,
}

impl FantasyConsole {
    pub fn new() -> Self {
        let palette_size = 256;
        let palette_map = (0..palette_size).collect();
        let palette: Vec<[u8; 3]> = SWEETIE_16
            .into_iter()
            .chain(
                ([255].into_iter().cycle())
                    .map(|x| [x; 3])
                    .take(palette_size - 16),
            )
            .collect();
        assert_eq!(palette.len(), palette_size);
        let mut x = Self {
            screen: Pixmap::new(240, 136).unwrap(),
            overlay_screen: Pixmap::new(240, 136).unwrap(),
            _output_screen: Pixmap::new(240, 136).unwrap(),

            font: Pixmap::new(128, 128).unwrap(),
            sprites: Pixmap::new(1, 1).unwrap(),
            indexed_sprites: IndexedImage::new(1, 1),
            maps: Vec::new(),
            files: HashMap::new(),

            vbank: 0,
            palette_size,
            palette,
            palette_map,
            blit_segment: 2,
            screen_offset: [0; 2],
            sprite_flags: vec![0; 2048],
            music: None,
            sounds: Vec::new(),
            memory: EggMemory::new(),
            input: EggInput::new(),
            rng: Lcg64Xsh32::default(),
            sync_helper: SyncHelper::new(),
        };
        let mut spr_flags = String::from("00100000000000000000000000000000000000801000000000000000002020000010101010500000001000000000000000101030101000000000001010000000101010002000000000301010400000001000100000400010500000000000000010101010108020100000000000101010203000301080302000000000001010101010100000100010001010100000000010001000001000100010100000000000000000000010101030303010000010100000000000000000000000002030203000000000000000000000101010400000000000000010000000203010102000100000000000000000000000000010101000000000000000100010a060b0101020");
        spr_flags.push_str("00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        spr_flags.push_str("000000001010101000000000000000000070601010700000000000000000000010000000001000000000000000000000601010606060000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000100000000000000000000000000000000010100000000000000000000000001010101000000000000000700000302030200000000000000000d0006000");
        spr_flags.push_str("00000000101010100000000000000000000000200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000010001000000000000000000000000000100010000000000000000000000010001010100000000000000000000000000000000000000000000000000000000000001010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000");
        x.load_sprite_flags(&spr_flags);
        x
    }
    pub fn load_sprite_flags(&mut self, string: &str) {
        for i in 0..string.len() / 2 {
            let (char1, char2) = (
                string.chars().nth(i * 2).unwrap(),
                string.chars().nth(i * 2 + 1).unwrap(),
            );
            let mut string = String::new();
            string.push(char2);
            string.push(char1);
            let flag = u8::from_str_radix(&string, 16).unwrap();
            let (x, y) = (i % 16, i / 16);
            let index = x + y * 32;
            self.sprite_flags[index] = flag;
        }
    }
    pub fn input(&mut self) -> &mut EggInput {
        &mut self.input
    }
    pub fn sounds(&mut self) -> &mut Vec<(String, SfxOptions)> {
        &mut self.sounds
    }
    pub fn music_track(&mut self) -> &mut Option<(MusicTrack, bool)> {
        &mut self.music
    }
    pub fn colour(&self, index: u8) -> Color {
        if self.vbank == 1 && index == 0 {
            return Color::from_rgba8(0, 0, 0, 0);
        }
        array_to_colour(self.palette[index as usize])
    }
    pub fn to_texture(&mut self, image: &mut Image) {
        let [x, y] = self.get_screen_offset().clone();
        self._output_screen.clone_from(&self.screen);
        self._output_screen.draw_pixmap(
            x.into(),
            y.into(),
            self.overlay_screen.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        image.data.copy_from_slice(self._output_screen.data());
    }
    pub fn set_font(&mut self, font: &Image) {
        assert!(font.size().x == 128);
        assert!(font.size().y >= 128);
        for (i, c) in self.font.data_mut().iter_mut().zip(font.data.iter()) {
            *i = *c;
        }
    }
    pub fn set_sprites(&mut self, sheet: &Image) {
        self.sprites = Pixmap::from_vec(
            sheet.data.clone(),
            IntSize::from_wh(sheet.size().x as u32, sheet.size().y as u32).unwrap(),
        )
        .unwrap();
    }
    // TODO: 255 index as transparent...
    pub fn set_indexed_sprites(&mut self, sheet: &Image) {
        self.indexed_sprites = IndexedImage::from_image(sheet, &self.palette);
    }
    pub fn get_screen(&mut self) -> &mut Pixmap {
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
    }
    fn draw_colour_letter(&mut self, char: char, x: i32, y: i32, colour: Color) -> i32 {
        let char_index = char as u8 as usize;
        let pixel_index = (char_index % 16) * 8 + (char_index / 16) * 8 * 128;
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        let mut letter_width = 0;
        let colour = colour.premultiply().to_color_u8();
        for j in 0..8 {
            for i in 0..8 {
                let screen_index = (x + i) + 240 * (y + j);
                let pixel = self.font.pixels()[pixel_index + (i + 128 * j) as usize];
                if pixel.alpha() == 0 {
                    continue;
                }
                letter_width = letter_width.max(i + 1);
                if x + i >= 240 || y + j >= 136 {
                    continue;
                }
                if let Some(x) = screen.pixels_mut().get_mut(screen_index as usize) {
                    *x = colour;
                }
            }
        }
        letter_width
    }
    // TODO: RGB and indexed sprite data
    pub fn blit_sprite(&mut self, index: i32, x: i32, y: i32, flip: bool) {
        let (tx, ty) = ((index % 32) * 8, (index / 32) * 8);
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        let x_offset = x.min(0).abs();
        let y_offset = y.min(0).abs();
        let x_start = x.max(0);
        let y_start = y.max(0);
        let y_end = (y + 8).min(136);
        let x_end = (x + 8).min(240);
        if !flip {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                for (x, i) in (x_start..x_end).zip(x_offset..8) {
                    let screen_index = x + 240 * y;
                    let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                    if self.sprites.pixels()[sprite_index].alpha() == 0 {
                        continue;
                    }
                    screen.pixels_mut()[screen_index as usize] =
                        self.sprites.pixels()[sprite_index];
                }
            }
        } else {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                for (x, i) in (x_start..x_end).rev().zip(x_offset..8) {
                    let screen_index = x + 240 * y;
                    let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                    if self.sprites.pixels()[sprite_index].alpha() == 0 {
                        continue;
                    }
                    screen.pixels_mut()[screen_index as usize] =
                        self.sprites.pixels()[sprite_index];
                }
            }
        }
    }
    pub fn blit_mask(&mut self, index: i32, x: i32, y: i32, colour: Color, flip: bool) {
        let (tx, ty) = ((index % 32) * 8, (index / 32) * 8);
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        let x_offset = x.min(0).abs();
        let y_offset = y.min(0).abs();
        let x_start = x.max(0);
        let y_start = y.max(0);
        let y_end = (y + 8).min(136);
        let x_end = (x + 8).min(240);
        let colour = colour.premultiply().to_color_u8();
        if !flip {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                for (x, i) in (x_start..x_end).zip(x_offset..8) {
                    let screen_index = x + 240 * y;
                    let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                    if self.sprites.pixels()[sprite_index].alpha() == 0 {
                        continue;
                    }
                    screen.pixels_mut()[screen_index as usize] = colour;
                }
            }
        } else {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                for (x, i) in (x_start..x_end).rev().zip(x_offset..8) {
                    let screen_index = x + 240 * y;
                    let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                    if self.sprites.pixels()[sprite_index].alpha() == 0 {
                        continue;
                    }
                    screen.pixels_mut()[screen_index as usize] = colour;
                }
            }
        }
    }
    pub fn set_maps(&mut self, maps: Vec<TiledMap>) {
        info!("lodding maps");
        let maps = maps
            .into_iter()
            .enumerate()
            .map(|(i, map)| {
                info!("map {i}");
                let layers = map
                    .layers
                    .into_iter()
                    .map(|layer| {
                        info!("layer: {}", layer.name);
                        MapLayer::new(layer.name, layer.width, layer.height, layer.data)
                    })
                    .collect();
                GameMap::new(map.width, map.height, layers)
            })
            .collect();
        self.maps = maps;
    }
    pub fn horizontal_line(&mut self, x: i32, y: i32, width: i32, colour: Color) {
        if x >= 240 || y >= 136 || x < 0 || y < 0 {
            return;
        }
        let colour = colour.premultiply().to_color_u8();
        let over = (x + width - 240).max(0);
        let width = width - over;
        for i in 0..width {
            let screen_index = x + 240 * y + i;
            self.screen.pixels_mut()[screen_index as usize] = colour;
        }
    }
    pub fn vertical_line(&mut self, x: i32, y: i32, height: i32, colour: Color) {
        if x >= 240 || y >= 136 || x < 0 || y < 0 {
            return;
        }
        let colour = colour.premultiply().to_color_u8();
        let over = (y + height - 136).max(0);
        let height = height - over;
        for i in 0..height {
            let screen_index = x + 240 * (y + i);
            self.screen.pixels_mut()[screen_index as usize] = colour;
        }
    }
    pub fn draw_rect(
        &mut self,
        mut x: i32,
        mut y: i32,
        mut width: i32,
        mut height: i32,
        colour: Color,
    ) {
        if x >= 240 || y >= 136 {
            return;
        }
        if x < 0 {
            width += x;
            x = 0;
        }
        if y < 0 {
            height += y;
            y = 0;
        }
        if x + width > 240 {
            width = 240 - x;
        }
        for i in 0..height {
            self.horizontal_line(x, y + i, width, colour);
        }
    }
    pub fn draw_rect_border(&mut self, x: i32, y: i32, width: i32, height: i32, colour: Color) {
        if x >= 240 || y >= 136 {
            return;
        }
        self.horizontal_line(x, y, width, colour);
        self.horizontal_line(x, y + height - 1, width, colour);
        self.vertical_line(x, y, height, colour);
        self.vertical_line(x + width - 1, y, height, colour);
    }
    #[inline]
    pub fn draw_indexed_pixel(&mut self, index: usize, x: i32, y: i32) {
        let colour_index = self.indexed_sprites.data[index];
        let colour_index = self.palette_map[colour_index as usize];
        let colour = array_to_colour(self.palette[colour_index])
            .premultiply()
            .to_color_u8();
        let screen = &mut self.screen;
        let screen_index = x + 240 * y;
        screen.pixels_mut()[screen_index as usize] = colour;
    }
    pub fn draw_indexed_sprite(
        &mut self,
        index: i32,
        x: i32,
        y: i32,
        flip: bool,
        transparent_colour: u8,
    ) {
        let (tx, ty) = ((index % 32) * 8, (index / 32) * 8);
        let x_offset = x.min(0).abs();
        let y_offset = y.min(0).abs();
        let x_start = x.max(0);
        let y_start = y.max(0);
        let y_end = (y + 8).min(136);
        let x_end = (x + 8).min(240);
        if !flip {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                for (x, i) in (x_start..x_end).zip(x_offset..8) {
                    let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                    if self.indexed_sprites.data[sprite_index] == transparent_colour {
                        continue;
                    }
                    self.draw_indexed_pixel(sprite_index, x, y);
                }
            }
        } else {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                for (x, i) in (x_start..x_end).rev().zip(x_offset..8) {
                    let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                    if self.indexed_sprites.data[sprite_index] == transparent_colour {
                        continue;
                    }
                    self.draw_indexed_pixel(sprite_index, x, y);
                }
            }
        }
    }
    pub fn draw_scaled_sprite(
        &mut self,
        index: i32,
        x: i32,
        y: i32,
        flip: bool,
        transparent_colour: u8,
        scale: i32,
    ) {
        let (tx, ty) = ((index % 32) * 8, (index / 32) * 8);
        let x_offset = x.min(0).abs();
        let y_offset = y.min(0).abs();
        let x_start = x.max(0);
        let y_start = y.max(0);
        for j in y_offset..8 {
            for i in x_offset..8 {
                let sprite_index = if flip {
                    (tx + 7 - i + (ty + j) * 8 * 32) as usize
                } else {
                    (tx + i + (ty + j) * 8 * 32) as usize
                };
                if self.indexed_sprites.data[sprite_index] == transparent_colour {
                    continue;
                }
                for sx in 0..scale {
                    for sy in 0..scale {
                        self.draw_indexed_pixel(
                            sprite_index,
                            x_start + i * scale + sx,
                            y_start + j * scale + sy,
                        );
                    }
                }
            }
        }
    }
}

impl ConsoleApi for FantasyConsole {
    fn get_framebuffer(&mut self) -> &mut [u8; 16320] {
        todo!()
    }

    fn get_tiles(&mut self) -> &mut [u8; 8192] {
        todo!()
    }

    fn get_sprites(&mut self) -> &mut [u8; 8192] {
        todo!()
    }

    fn get_map(&mut self) -> &mut [u8; 32640] {
        todo!()
    }

    fn get_gamepads(&mut self) -> &mut [u8; 4] {
        &mut self.input.gamepads
    }

    fn get_mouse(&mut self) -> &mut MouseInput {
        &mut self.input.mouse
    }

    fn get_keyboard(&mut self) -> &mut [u8; 4] {
        todo!()
    }

    fn get_sfx_state(&mut self) -> &mut [u8; 16] {
        todo!()
    }

    fn get_sound_registers(&mut self) -> &mut [u8; 72] {
        todo!()
    }

    fn get_waveforms(&mut self) -> &mut [u8; 256] {
        todo!()
    }

    fn get_sfx(&mut self) -> &mut [u8; 4224] {
        todo!()
    }

    fn get_music_patterns(&mut self) -> &mut [u8; 11520] {
        todo!()
    }

    fn get_music_tracks(&mut self) -> &mut [u8; 408] {
        todo!()
    }

    fn get_sound_state(&mut self) -> &mut [u8; 4] {
        todo!()
    }

    fn get_stereo_volume(&mut self) -> &mut [u8; 4] {
        todo!()
    }

    fn memory(&mut self) -> &mut EggMemory {
        &mut self.memory
    }

    fn get_sprite_flags(&mut self) -> &mut [u8] {
        self.sprite_flags.as_mut_slice()
    }

    fn get_system_font(&mut self) -> &mut [u8; 2048] {
        todo!()
    }

    fn get_palette(&mut self) -> &mut [[u8; 3]] {
        &mut self.palette
    }

    fn get_palette_map(&mut self) -> &mut [usize] {
        self.palette_map.as_mut_slice()
    }

    fn get_border_colour(&mut self) -> &mut u8 {
        todo!()
    }

    fn get_screen_offset(&mut self) -> &mut [i8; 2] {
        &mut self.screen_offset
    }

    fn get_mouse_cursor(&mut self) -> &mut u8 {
        todo!()
    }

    fn get_blit_segment(&mut self) -> &mut u8 {
        &mut self.blit_segment
    }

    fn btn(&self, index: i32) -> bool {
        self.input.mem_btn(index as u8)
    }

    fn btnp(&self, index: i32, _hold: i32, _period: i32) -> bool {
        self.input.mem_btnp(index as u8)
    }

    fn clip(&mut self, _x: i32, _y: i32, _width: i32, _height: i32) {
        todo!()
    }

    fn cls(&mut self, color: u8) {
        let colour = self.colour(color);
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .fill(colour)
    }

    fn circ(&mut self, x: i32, y: i32, radius: i32, color: u8) {
        let mut paint = Paint {
            anti_alias: false,
            ..Default::default()
        };
        paint.set_color(self.colour(color));
        let path = {
            let mut pb = PathBuilder::new();
            pb.push_circle(x as f32, y as f32, radius as f32);
            pb.finish().unwrap()
        };
        let fill = FillRule::default();

        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .fill_path(&path, &paint, fill, Transform::identity(), None);
    }

    fn circb(&mut self, x: i32, y: i32, radius: i32, color: u8) {
        let mut paint = Paint {
            anti_alias: false,
            ..Default::default()
        };
        paint.set_color(self.colour(color));
        let path = {
            let mut pb = PathBuilder::new();
            // pb.move_to(x as f32, y as f32);
            pb.push_circle(x as f32, y as f32, radius as f32);
            pb.finish().unwrap()
        };
        let stroke = Stroke {
            width: 1.0,
            ..Default::default()
        };

        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    fn elli(&mut self, _x: i32, _y: i32, _a: i32, _b: i32, _color: u8) {
        todo!()
    }

    fn ellib(&mut self, _x: i32, _y: i32, _a: i32, _b: i32, _color: u8) {
        todo!()
    }

    fn exit(&mut self) {
        todo!()
    }

    fn fget(&self, _sprite_index: i32, _flag: i8) -> bool {
        todo!()
    }

    fn fset(&mut self, _sprite_index: i32, _flag: i8, _value: bool) {
        todo!()
    }

    fn font_raw(
        _text: &str,
        _x: i32,
        _y: i32,
        _opts: egg_core::tic80_api::core::FontOptions,
    ) -> i32 {
        todo!()
    }

    fn font_alloc(
        _text: impl AsRef<str>,
        _x: i32,
        _y: i32,
        _opts: egg_core::tic80_api::core::FontOptions,
    ) -> i32 {
        todo!()
    }

    fn key(&self, index: i32) -> bool {
        self.input.key(index as usize)
    }

    fn keyp(&self, index: i32, hold: i32, period: i32) -> bool {
        self.input.keyp(index as usize, hold, period)
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u8) {
        let mut paint = Paint {
            anti_alias: false,
            ..Default::default()
        };
        paint.set_color(self.colour(color));
        let path = {
            let mut pb = PathBuilder::new();
            pb.move_to(x0, y0);
            pb.line_to(x1, y1);
            pb.finish().unwrap()
        };
        let stroke = Stroke {
            width: 1.0,
            ..Default::default()
        };

        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    fn map(&mut self, opts: egg_core::tic80_api::core::MapOptions) {
        let bank = self.sync_helper.last_bank() as usize;
        self.map_draw(bank, 0, opts);
    }

    fn mget(&self, x: i32, y: i32) -> i32 {
        // let i = dbg!(self.maps[0].get(0, x as usize, y as usize).unwrap() as i32);
        self.map_get(self.sync_helper.last_bank() as usize, 0, x, y)
            .try_into()
            .unwrap()
    }

    fn mset(&mut self, _x: i32, _y: i32, _value: i32) {
        todo!()
    }

    fn mouse(&self) -> MouseInput {
        self.input.mouse.clone()
    }

    fn music(
        &mut self,
        track: Option<&MusicTrack>,
        _opts: egg_core::tic80_api::core::MusicOptions,
    ) {
        info!("Playing track \"{:?}\"", track);
        if let Some(track) = track {
            self.music = Some((track.clone(), false));
        } else {
            self.music = None;
        }
    }

    fn pix(&mut self, x: i32, y: i32, color: u8) -> u8 {
        let i = (y * self.screen.width() as i32 + x % self.screen.width() as i32) as usize;
        if i > self.screen.pixels().len() {
            return 0;
        }
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .pixels_mut()[i] = self.colour(color).premultiply().to_color_u8();
        0
    }

    fn peek(&self, _address: i32, _bits: u8) -> u8 {
        todo!()
    }

    fn peek4(&self, _address: i32) -> u8 {
        todo!()
    }

    fn peek2(&self, _address: i32) -> u8 {
        todo!()
    }

    fn peek1(&self, _address: i32) -> u8 {
        todo!()
    }

    fn pmem(&mut self, _address: i32, _value: i64) -> i32 {
        todo!()
    }

    fn poke(&mut self, _address: i32, _value: u8, _bits: u8) {
        todo!()
    }

    fn poke4(&mut self, _address: i32, _value: u8) {
        todo!()
    }

    fn poke2(&mut self, _address: i32, _value: u8) {
        todo!()
    }

    fn poke1(&mut self, _address: i32, _value: u8) {
        todo!()
    }

    fn print_alloc(
        &mut self,
        text: impl AsRef<str>,
        x: i32,
        y: i32,
        opts: egg_core::tic80_api::core::PrintOptions,
    ) -> i32 {
        self.print_raw(text.as_ref(), x, y, opts)
    }

    fn print_raw(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        opts: egg_core::tic80_api::core::PrintOptions,
    ) -> i32 {
        let mut max_width = 0;
        let mut dx = x;
        let mut dy = y;
        for char in text.chars() {
            // This is a bit of a hack to make lines wrap
            match char as u8 {
                // Newline
                10 => {
                    dx = x;
                    dy += 6;
                }
                32 => {
                    dx += if opts.small_text { 3 } else { 4 };
                }
                // Null
                0 => {}
                _ => {
                    let char = if opts.small_text {
                        (char as u8 + 128) as char
                    } else {
                        char
                    };
                    let width =
                        self.draw_colour_letter(char, dx, dy, self.colour(opts.color as u8));
                    dx += width + 1;
                }
            };
            max_width = max_width.max(dx - x);
        }
        max_width
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        let colour = self.colour(color);
        self.draw_rect(x, y, w, h, colour);
    }

    fn rectb(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        let colour = self.colour(color);
        self.draw_rect_border(x, y, w, h, colour)
    }

    fn sfx(&mut self, sfx_id: &str, opts: egg_core::tic80_api::core::SfxOptions) {
        self.sounds.push((sfx_id.to_string(), opts));
    }

    fn spr(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        opts: egg_core::tic80_api::core::StaticSpriteOptions,
    ) {
        let flip = match opts.flip {
            Flip::Horizontal => true,
            _ => false,
        };
        let transparent = opts.transparent.get(0).cloned().unwrap_or(255);
        if opts.scale > 1 {
            self.draw_scaled_sprite(id, x, y, flip, transparent, opts.scale);
            return;
        }
        match (opts.w, opts.h) {
            (1, 1) => self.draw_indexed_sprite(id, x, y, flip, transparent),
            (w, h) => {
                for j in 0..h {
                    for i in 0..w {
                        let x_pos = if !flip {
                            x + i * 8
                        } else {
                            x + (w - 1 - i) * 8
                        };
                        self.draw_indexed_sprite(
                            id + i + j * 32,
                            x_pos,
                            y + j * 8,
                            flip,
                            transparent,
                        );
                    }
                }
            }
        }
    }

    fn sync(&mut self, mask: i32, bank: u8, _to_cart: bool) {
        self.sync_helper.sync2(mask, bank).unwrap();
    }

    fn time(&self) -> f32 {
        todo!()
    }

    fn tstamp(&self) -> u32 {
        todo!()
    }

    fn trace_alloc(text: impl AsRef<str>, _color: u8) {
        println!("{}", text.as_ref());
    }

    fn tri(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x3: f32, _y3: f32, _color: u8) {
        todo!()
    }

    fn trib(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x3: f32, _y3: f32, _color: u8) {
        todo!()
    }

    fn ttri(
        &mut self,
        _x1: f32,
        _y1: f32,
        _x2: f32,
        _y2: f32,
        _x3: f32,
        _y3: f32,
        _u1: f32,
        _v1: f32,
        _u2: f32,
        _v2: f32,
        _u3: f32,
        _v3: f32,
        _opts: egg_core::tic80_api::core::TTriOptions,
    ) {
        todo!()
    }

    fn vbank(&mut self, bank: u8) -> u8 {
        if bank <= 1 {
            self.vbank = bank.into();
        }
        0
    }

    fn sync_helper(&mut self) -> &mut SyncHelper {
        &mut self.sync_helper
    }

    fn rng(&mut self) -> &mut egg_core::rand::Lcg64Xsh32 {
        &mut self.rng
    }

    fn previous_gamepad(&mut self) -> &mut [u8; 4] {
        &mut self.input.previous_gamepads
    }

    fn previous_mouse(&mut self) -> &mut MouseInput {
        &mut self.input.previous_mouse
    }

    fn maps(&mut self) -> &mut Vec<GameMap> {
        &mut self.maps
    }
    fn map_get(&self, bank: usize, layer: usize, x: i32, y: i32) -> usize {
        self.maps[bank]
            .layers
            .get(layer)
            .and_then(|layer| layer.get(x as usize, y as usize))
            .unwrap_or(0)
    }
    fn map_set(&mut self, bank: usize, layer: usize, x: i32, y: i32, value: usize) {
        self.maps[bank]
            .layers
            .get_mut(layer)
            .and_then(|layer| layer.get_mut(x as usize, y as usize))
            .map(|tile| *tile = value);
    }
    fn write_file(&mut self, filename: String, data: &[u8]) {
        self.files.insert(filename, data.into());
    }
    fn read_file(&mut self, filename: String) -> Option<&[u8]> {
        self.files.get(&filename).map(|vec| (*vec).as_slice())
    }
    fn map_draw(
        &mut self,
        bank: usize,
        layer: usize,
        mut opts: egg_core::tic80_api::core::MapOptions,
    ) {
        if self.maps.is_empty()
            || opts.sx + opts.w * 8 < 0
            || opts.sy + opts.h * 8 < 0
            || opts.sx >= 240
            || opts.sy >= 132
        {
            return;
        }
        // Crop map
        if opts.sx <= 0 {
            let x_tiles = -(opts.sx / 8);
            opts.sx += x_tiles * 8;
            opts.x += x_tiles;
            opts.w -= x_tiles;
        }
        if opts.sy <= 0 {
            let y_tiles = -(opts.sy / 8);
            opts.sy += y_tiles * 8;
            opts.y += y_tiles;
            opts.h -= y_tiles;
        }
        opts.w = opts.w.min(31);
        opts.h = opts.h.min(18);
        for j in 0..opts.h {
            for i in 0..opts.w {
                if let (Ok(x_index), Ok(y_index)) =
                    ((opts.x + i).try_into(), (opts.y + j).try_into())
                {
                    if let Some(index) = self.maps[bank]
                        .layers
                        .get(layer)
                        .and_then(|layer| layer.get(x_index, y_index))
                    {
                        // if index == 0 {
                        //     continue;
                        // } else {
                        //     index -= 1;
                        // }
                        let (x, y) = (opts.sx + i * 8, opts.sy + j * 8);
                        self.draw_indexed_sprite(
                            index as i32,
                            x,
                            y,
                            false,
                            opts.transparent.unwrap_or(255),
                        );
                    }
                }
            }
        }
    }

    fn sprite(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        opts: StaticSpriteOptions,
        palette_map: &[usize],
    ) {
        todo!()
    }

    fn send(&mut self, channel: egg_core::system::DataChannel, data: &[u8]) {
        todo!()
    }

    fn screen_size(&self) -> (u32, u32) {
        (self.screen.width(), self.screen.height())
    }

    fn get_bitmap_indexed(&self, id: usize) -> &[u8] {
        match id {
            0 => self.screen.data(),
            1 => self.overlay_screen.data(),
            2 => &self.indexed_sprites.data,
            _ => panic!("bitmap {id} does not exist"),
        }
    }

    fn draw_outline(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        opts: StaticSpriteOptions,
        outline_colour: u8,
    ) {
        let flip = match opts.flip {
            egg_core::tic80_api::core::Flip::None => false,
            egg_core::tic80_api::core::Flip::Horizontal => true,
            _ => false,
        };
        if opts.scale > 1 {
            let transparent = opts.transparent.get(0).cloned().unwrap_or(255);
            self.palette_map_set_all(outline_colour.into());
            let scale = opts.scale;
            self.draw_scaled_sprite(id, x + 1, y, flip, transparent, scale);
            self.draw_scaled_sprite(id, x - 1, y, flip, transparent, scale);
            self.draw_scaled_sprite(id, x, y + 1, flip, transparent, scale);
            self.draw_scaled_sprite(id, x, y - 1, flip, transparent, scale);
            self.palette_map_reset();
            return;
        }
        match (opts.w, opts.h) {
            (w, h) => {
                for j in 0..h {
                    for i in 0..w {
                        let id = id + i + j * 32;
                        let x = if !flip {
                            x + i * 8
                        } else {
                            x + (w - 1 - i) * 8
                        };
                        let y = y + j * 8;
                        let colour = self.colour(outline_colour);
                        self.blit_mask(id, x + 1, y, colour, flip);
                        self.blit_mask(id, x - 1, y, colour, flip);
                        self.blit_mask(id, x, y + 1, colour, flip);
                        self.blit_mask(id, x, y - 1, colour, flip);
                        self.blit_sprite(id, x, y, flip);
                    }
                }
            }
        }
    }
}
