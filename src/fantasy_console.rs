use std::collections::HashMap;

use bevy::prelude::{Image, info};
use egg_core::{
    data::sound::music::MusicTrack,
    gamestate::EggInput,
    rand::Lcg64Xsh32,
    system::{ConsoleApi, EggMemory, GameMap, MapLayer, SyncHelper},
    tic80_api::{
        core::{Flip, HEIGHT, MouseInput, SfxOptions, StaticSpriteOptions, WIDTH},
        helpers::SWEETIE_16,
    },
};

use crate::tiled;

use self::drawing::{Canvas, EdgePolicy, Transform};
use self::image::{IndexedImage, Rgba, RgbaImage};

mod drawing;
mod image;

// TODO:
// Load interactables from tiled maps
// Separate BG & FG palettes, upgrade BGs.
// Move tiled map parsing/loading into core
// Serialize save/load state, use structs, remove bits.
// Dialogue hashmap
// Make UI actually work: Hierarchical layout, compositional widgets.
// Unified walkaround collision space
// Yolkomatic

pub struct FantasyConsole {
    screen: RgbaImage,
    overlay_screen: RgbaImage,
    output_screen: RgbaImage,

    font: RgbaImage,
    sprites: RgbaImage,
    indexed_sprites: IndexedImage,
    maps: Vec<GameMap>,
    files: HashMap<String, Vec<u8>>,

    vbank: usize,

    palette: Vec<[u8; 3]>,
    palette_map: Vec<usize>,
    blit_segment: u8,
    screen_offset: [i8; 2],
    border_colour: [u8; 3],

    sprite_flags: Vec<u8>,
    music: Option<(MusicTrack, bool)>,
    memory: EggMemory,
    sounds: HashMap<String, SfxOptions>,
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
            screen: RgbaImage::new(WIDTH as u32, HEIGHT as u32),
            overlay_screen: RgbaImage::new(WIDTH as u32, HEIGHT as u32),
            output_screen: RgbaImage::new(WIDTH as u32, HEIGHT as u32),

            font: RgbaImage::new(128, 128),
            sprites: RgbaImage::new(1, 1),
            indexed_sprites: IndexedImage::new(1, 1),
            maps: Vec::new(),
            files: HashMap::new(),

            border_colour: palette[0],
            vbank: 0,
            palette,
            palette_map,
            blit_segment: 2,
            screen_offset: [0; 2],

            sprite_flags: vec![0; 2048],
            music: None,
            sounds: HashMap::new(),
            memory: EggMemory::default(),
            input: EggInput::new(),
            rng: Lcg64Xsh32::default(),
            sync_helper: SyncHelper::default(),
        };
        let mut spr_flags = String::from(
            "00100000000000000000000000000000000000801000000000000000002020000010101010500000001000000000000000101030101000000000001010000000101010002000000000301010400000001000100000400010500000000000000010101010108020100000000000101010203000301080302000000000001010101010100000100010001010100000000010001000001000100010100000000000000000000010101030303010000010100000000000000000000000002030203000000000000000000000101010400000000000000010000000203010102000100000000000000000000000000010101000000000000000100010a060b0101020",
        );
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
    pub fn sounds(&mut self) -> &mut HashMap<String, SfxOptions> {
        &mut self.sounds
    }
    pub fn music_track(&mut self) -> &mut Option<(MusicTrack, bool)> {
        &mut self.music
    }
    pub fn colour(&self, index: u8) -> Rgba {
        if self.vbank == 1 && index == 0 {
            return Rgba::TRANSPARENT;
        }
        Rgba::from_rgb(self.palette[index as usize])
    }
    pub fn blit_to_image(&mut self, image: &mut [u8]) {
        let [x, y] = *self.get_screen_offset();
        self.output_screen.clone_from(&self.screen);
        self.output_screen.blit(
            x.into(),
            y.into(),
            &self.overlay_screen,
            EdgePolicy::Clamp,
            Transform::IDENTITY,
            |p| p.a() == 0,
        );
        image.copy_from_slice(self.output_screen.data());
    }
    pub fn set_font(&mut self, font: &Image) {
        assert!(font.size().x == 128);
        assert!(font.size().y >= 128);
        for (i, c) in self
            .font
            .data_mut()
            .iter_mut()
            .zip(font.data.iter().flatten())
        {
            *i = *c;
        }
    }
    pub fn set_sprites(&mut self, sheet: &Image) {
        self.sprites = RgbaImage::from_vec(
            sheet
                .data
                .as_ref()
                .expect("Tried to load uninitialised spritesheet.")
                .clone(),
            sheet.size().x,
            sheet.size().y,
        );
    }
    pub fn set_indexed_sprites(&mut self, sheet: &Image) {
        self.indexed_sprites = IndexedImage::from_image(sheet, &self.palette);
    }
    pub fn get_screen(&mut self) -> &mut RgbaImage {
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
    }
    fn draw_colour_letter(&mut self, char: char, x: i32, y: i32, colour: Rgba) -> i32 {
        let char_index = char as u8 as usize;
        let glyph_x = (char_index % 16) * 8;
        let glyph_y = (char_index / 16) * 8;
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        let mut letter_width = 0;
        for j in 0..8 {
            for i in 0..8 {
                let font_index = (glyph_x + i as usize) + (glyph_y + j as usize) * 128;
                if self.font.alpha_at_index(font_index) == 0 {
                    continue;
                }
                letter_width = letter_width.max(i + 1);
                if x + i >= WIDTH || y + j >= HEIGHT {
                    continue;
                }
                let screen_index = (x + i) + WIDTH * (y + j);
                if screen_index >= 0 && (screen_index as usize) < (WIDTH * HEIGHT) as usize {
                    screen.set_pixel_index(screen_index as usize, colour);
                }
            }
        }
        letter_width
    }
    // Draws an RGB sprite to the RGB screen
    pub fn blit_sprite(&mut self, index: i32, x: i32, y: i32, flip: bool) {
        let (tx, ty) = ((index % 32) * 8, (index / 32) * 8);
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        let (x_offset, y_offset) = (x.min(0).abs(), y.min(0).abs());
        let (x_start, y_start) = (x.max(0), y.max(0));
        let (x_end, y_end) = ((x + 8).min(WIDTH), (y + 8).min(HEIGHT));
        let sprites = &self.sprites;
        let mut draw_pix = |x: i32, i: i32| {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                let screen_index = (x + WIDTH * y) as usize;
                let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                if sprites.alpha_at_index(sprite_index) != 0 {
                    screen.set_pixel_index(screen_index, sprites.get_pixel_index(sprite_index));
                }
            }
        };
        if flip {
            for (x, i) in (x_start..x_end).zip(x_offset..8) {
                draw_pix(x, 7 - i);
            }
        } else {
            for (x, i) in (x_start..x_end).zip(x_offset..8) {
                draw_pix(x, i);
            }
        }
    }
    // Draws a sprite to the RGB screen using only `colour` and the sprite's transparency
    pub fn blit_mask(&mut self, index: i32, x: i32, y: i32, colour: Rgba, flip: bool) {
        let (tx, ty) = ((index % 32) * 8, (index / 32) * 8);
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        let (x_offset, y_offset) = (x.min(0).abs(), y.min(0).abs());
        let (x_start, y_start) = (x.max(0), y.max(0));
        let (x_end, y_end) = ((x + 8).min(WIDTH), (y + 8).min(HEIGHT));
        let sprites = &self.sprites;
        let mut draw_pix = |x: i32, i: i32| {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                let screen_index = (x + WIDTH * y) as usize;
                let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                if sprites.alpha_at_index(sprite_index) != 0 {
                    screen.set_pixel_index(screen_index, colour);
                }
            }
        };
        if flip {
            for (x, i) in (x_start..x_end).zip(x_offset..8) {
                draw_pix(x, 7 - i);
            }
        } else {
            for (x, i) in (x_start..x_end).zip(x_offset..8) {
                draw_pix(x, i);
            }
        }
    }
    pub fn set_maps(&mut self, maps: Vec<tiled::TiledMap>) {
        info!("lodding maps");
        let maps = maps
            .into_iter()
            .enumerate()
            .map(|(i, map)| {
                info!("map {i}");
                let layers = map
                    .layers
                    .into_iter()
                    .map(|layer| match layer {
                        tiled::TiledMapLayer::TileLayer(layer) => {
                            info!("layer: {}", layer.name);
                            MapLayer::new(layer.name, layer.width, layer.height, layer.data)
                        }
                        tiled::TiledMapLayer::ObjectLayer(layer) => {
                            info!("Oh hey, it's an object layer! ({})", layer.name);
                            for object in layer.objects {
                                info!("object: {:?}", object.properties);
                            }
                            MapLayer::new_empty(1, 1)
                        }
                    })
                    .collect();
                GameMap::new(map.width, map.height, layers)
            })
            .collect();
        self.maps = maps;
    }
    #[inline]
    pub fn draw_indexed_pixel(&mut self, index: usize, x: i32, y: i32) {
        let colour_index = self.indexed_sprites.data[index];
        let colour_index = self.palette_map[colour_index as usize];
        let colour = Rgba::from_rgb(self.palette[colour_index]);
        let screen_index = (x + WIDTH * y) as usize;
        self.screen.set_pixel_index(screen_index, colour);
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
        let (x_offset, y_offset) = (x.min(0).abs(), y.min(0).abs());
        let (x_start, y_start) = (x.max(0), y.max(0));
        let (x_end, y_end) = ((x + 8).min(WIDTH), (y + 8).min(HEIGHT));
        let mut draw_pix = |x: i32, i: i32| {
            for (y, j) in (y_start..y_end).zip(y_offset..8) {
                let sprite_index = (tx + i + (ty + j) * 8 * 32) as usize;
                match self.indexed_sprites.data.get(sprite_index) {
                    Some(&colour) if colour == transparent_colour => continue,
                    None => continue,
                    _ => self.draw_indexed_pixel(sprite_index, x, y),
                }
            }
        };
        if flip {
            for (x, i) in (x_start..x_end).zip(x_offset..8) {
                draw_pix(x, 7 - i);
            }
        } else {
            for (x, i) in (x_start..x_end).zip(x_offset..8) {
                draw_pix(x, i);
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
        let (x_offset, y_offset) = (x.min(0).abs(), y.min(0).abs());
        let (x_start, y_start) = (x.max(0), y.max(0));
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
    fn _draw_pixel_with_map(&mut self, _colour: u8, x: i32, y: i32, _map: &[[u8; 256]; 256]) {
        let screen_index = (x + WIDTH * y) as usize;
        let _colour = self.screen.get_pixel_index(screen_index);
    }
}

impl ConsoleApi for FantasyConsole {
    fn get_gamepads(&mut self) -> &mut [u8; 4] {
        &mut self.input.gamepads
    }

    fn get_mouse(&mut self) -> &mut MouseInput {
        &mut self.input.mouse
    }

    fn memory(&mut self) -> &mut EggMemory {
        &mut self.memory
    }

    fn get_sprite_flags(&mut self) -> &mut [u8] {
        self.sprite_flags.as_mut_slice()
    }

    fn get_palette(&mut self) -> &mut [[u8; 3]] {
        &mut self.palette
    }

    fn get_palette_map(&mut self) -> &mut [usize] {
        self.palette_map.as_mut_slice()
    }

    fn get_border_colour(&mut self) -> &mut [u8; 3] {
        &mut self.border_colour
    }

    fn get_screen_offset(&mut self) -> &mut [i8; 2] {
        &mut self.screen_offset
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
        let colour = self.colour(color);
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .fill_circle(x, y, radius, colour);
    }

    fn circb(&mut self, x: i32, y: i32, radius: i32, color: u8) {
        let colour = self.colour(color);
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .stroke_circle(x, y, radius, colour);
    }

    fn elli(&mut self, _x: i32, _y: i32, _a: i32, _b: i32, _color: u8) {
        todo!()
    }

    fn ellib(&mut self, _x: i32, _y: i32, _a: i32, _b: i32, _color: u8) {
        todo!()
    }

    fn exit(&mut self) {
        panic!("Perfectly normal shutdown.")
    }

    fn key(&self, index: i32) -> bool {
        self.input.key(index as usize)
    }

    fn keyp(&self, index: i32, hold: i32, period: i32) -> bool {
        self.input.keyp(index as usize, hold, period)
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u8) {
        let colour = self.colour(color);
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .line(x0 as i32, y0 as i32, x1 as i32, y1 as i32, colour);
    }

    fn map(&mut self, opts: egg_core::tic80_api::core::MapOptions) {
        let bank = self.sync_helper.last_bank() as usize;
        self.map_draw(bank, 0, opts);
    }

    fn mouse(&self) -> MouseInput {
        self.input.mouse.clone()
    }

    fn pmem(&mut self, _address: i32, _value: i64) -> i32 {
        todo!()
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
        let w = self.screen.width() as i32;
        let h = self.screen.height() as i32;
        let i = (y * w + x % w) as usize;
        if i >= (w * h) as usize {
            return 0;
        }
        let colour = self.colour(color);
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .set_pixel_index(i, colour);
        0
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
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .fill_rect(x, y, w, h, colour);
    }

    fn rectb(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        let colour = self.colour(color);
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .stroke_rect(x, y, w, h, colour);
    }

    fn sfx(&mut self, sfx_id: &str, opts: egg_core::tic80_api::core::SfxOptions) {
        self.sounds.insert(sfx_id.to_string(), opts);
    }

    fn spr(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        opts: egg_core::tic80_api::core::StaticSpriteOptions,
    ) {
        let flip = matches!(opts.flip, Flip::Horizontal);
        let transparent = opts.transparent.first().cloned().unwrap_or(255);
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
        self.sync_helper.sync(mask, bank).unwrap();
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
        if let Some(tile) = self.maps[bank]
            .layers
            .get_mut(layer)
            .and_then(|layer| layer.get_mut(x as usize, y as usize))
        {
            *tile = value
        }
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
            || opts.sx >= WIDTH
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
                    && let Some(index) = self.maps[bank]
                        .layers
                        .get(layer)
                        .and_then(|layer| layer.get(x_index, y_index))
                {
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

    fn sprite(
        &mut self,
        _id: i32,
        _x: i32,
        _y: i32,
        _opts: StaticSpriteOptions,
        _palette_map: &[usize],
    ) {
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
            let transparent = opts.transparent.first().cloned().unwrap_or(255);
            self.palette_map_set_all(outline_colour.into());
            let scale = opts.scale;
            self.draw_scaled_sprite(id, x + 1, y, flip, transparent, scale);
            self.draw_scaled_sprite(id, x - 1, y, flip, transparent, scale);
            self.draw_scaled_sprite(id, x, y + 1, flip, transparent, scale);
            self.draw_scaled_sprite(id, x, y - 1, flip, transparent, scale);
            self.palette_map_reset();
            return;
        }
        let (w, h) = (opts.w, opts.h);
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
