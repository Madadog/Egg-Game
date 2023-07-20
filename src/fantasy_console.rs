

use bevy::prelude::Image;
use egg_core::{
    gamestate::EggInput,
    rand::Lcg64Xsh32,
    system::{ConsoleApi, EggMemory, SyncHelper},
    tic80_api::{core::{MouseInput, SfxOptions}, helpers::SWEETIE_16},
};
use tiny_skia::{
    Color, FillRule, IntSize, Paint, PathBuilder, Pattern, Pixmap, PixmapPaint, Rect, Stroke,
    Transform, Path,
};

use self::drawing::array_to_colour;

mod drawing;

pub struct FantasyConsole {
    screen: Pixmap,
    overlay_screen: Pixmap,
    _output_screen: Pixmap,

    font: Pixmap,
    sprites: Pixmap,

    vbank: usize,
    palette: [[u8; 3]; 16],
    palette_map: [u8; 16],
    blit_segment: u8,
    screen_offset: [i8; 2],
    sprite_flags: [u8; 512],
    music: Option<usize>,
    memory: EggMemory,
    sounds: Vec<(i32, SfxOptions)>,
    input: EggInput,
    rng: Lcg64Xsh32,
    sync_helper: SyncHelper,
}

impl FantasyConsole {
    pub fn new() -> Self {
        Self {
            screen: Pixmap::new(240, 136).unwrap(),
            overlay_screen: Pixmap::new(240, 136).unwrap(),
            _output_screen: Pixmap::new(240, 136).unwrap(),

            font: Pixmap::new(128, 128).unwrap(),
            sprites: Pixmap::new(1, 1).unwrap(),

            vbank: 0,
            palette: SWEETIE_16,
            palette_map: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            blit_segment: 2,
            screen_offset: [0; 2],
            sprite_flags: [0; 512],
            music: None,
            sounds: Vec::new(),
            memory: EggMemory::new(),
            input: EggInput::new(),
            rng: Lcg64Xsh32::default(),
            sync_helper: SyncHelper::new(),
        }
    }
    pub fn input(&mut self) -> &mut EggInput {
        &mut self.input
    }
    pub fn colour(&self, index: u8) -> Color {
        if self.vbank == 1 && index == 0 {
            return Color::from_rgba8(0, 0, 0, 0);
        }
        array_to_colour(self.palette[index as usize])
    }
    pub fn to_texture(&mut self, image: &mut Image) {
        let list = [self.screen.as_ref(), self.overlay_screen.as_ref()];
        for i in list {
            self._output_screen.draw_pixmap(
                0,
                0,
                i,
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        image.data.copy_from_slice(self._output_screen.data());
    }
    pub fn set_font(&mut self, font: &Image) {
        assert!(font.size().x >= 128.0);
        assert!(font.size().y >= 128.0);
        for (i, c) in self.font.data_mut().iter_mut().zip(font.data.iter()) {
            *i = *c;
        }
    }
    pub fn set_sheet(&mut self, sheet: &Image) {
        self.sprites = Pixmap::from_vec(
            sheet.data.clone(),
            IntSize::from_wh(sheet.size().x as u32, sheet.size().y as u32).unwrap(),
        )
        .unwrap();
    }
    pub fn get_screen(&mut self) -> &mut Pixmap {
        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
    }
    pub fn draw_letter(&mut self, char: char, x: i32, y: i32) {
        let char_index = char as u8;
        let (tx, ty) = ((char_index % 16) * 8, (char_index / 16) * 8);
        // This can't be made a function until Rust gets good.
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        screen.fill_rect(
            Rect::from_xywh(x as f32, y as f32, 8.0, 8.0).unwrap(),
            &Paint {
                shader: Pattern::new(
                    self.font.as_ref(),
                    tiny_skia::SpreadMode::Repeat,
                    tiny_skia::FilterQuality::Nearest,
                    1.0,
                    Transform::from_translate(-(tx as f32) + x as f32, -(ty as f32) + y as f32),
                ),
                anti_alias: false,
                ..Default::default()
            },
            Transform::identity(),
            None,
        )
    }
    pub fn draw_sprite(&mut self, index: i32, x: i32, y: i32) {
        let (tx, ty) = ((index % 32) * 8, (index / 32) * 8);
        // This can't be made a function until Rust gets good.
        let screen = match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        };
        screen.fill_rect(
            Rect::from_xywh(x as f32, y as f32, 8.0, 8.0).unwrap(),
            &Paint {
                shader: Pattern::new(
                    self.sprites.as_ref(),
                    tiny_skia::SpreadMode::Repeat,
                    tiny_skia::FilterQuality::Nearest,
                    1.0,
                    Transform::from_translate(-(tx as f32) + x as f32, -(ty as f32) + y as f32),
                ),
                anti_alias: false,
                ..Default::default()
            },
            Transform::identity(),
            None,
        )
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

    fn get_sprite_flags(&mut self) -> &mut [u8; 512] {
        &mut self.sprite_flags
    }

    fn get_system_font(&mut self) -> &mut [u8; 2048] {
        todo!()
    }

    fn get_palette(&mut self) -> &mut [[u8; 3]; 16] {
        &mut self.palette
    }

    fn get_palette_map(&mut self) -> &mut [u8; 16] {
        &mut self.palette_map
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

    fn btnp(&self, index: i32, hold: i32, period: i32) -> bool {
        self.input.mem_btnp(index as u8)
    }

    fn clip(&mut self, x: i32, y: i32, width: i32, height: i32) {
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
        let mut stroke = Stroke::default();
        stroke.width = 1.0;

        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    fn elli(&mut self, x: i32, y: i32, a: i32, b: i32, color: u8) {
        todo!()
    }

    fn ellib(&mut self, x: i32, y: i32, a: i32, b: i32, color: u8) {
        todo!()
    }

    fn exit(&mut self) {
        todo!()
    }

    fn fget(&self, sprite_index: i32, flag: i8) -> bool {
        todo!()
    }

    fn fset(&mut self, sprite_index: i32, flag: i8, value: bool) {
        todo!()
    }

    fn font_raw(text: &str, x: i32, y: i32, opts: egg_core::tic80_api::core::FontOptions) -> i32 {
        todo!()
    }

    fn font_alloc(
        text: impl AsRef<str>,
        x: i32,
        y: i32,
        opts: egg_core::tic80_api::core::FontOptions,
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
        let fill = FillRule::default();

        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .fill_path(&path, &paint, fill, Transform::identity(), None);
    }

    fn map(&mut self, opts: egg_core::tic80_api::core::MapOptions) {
        ()
    }

    fn mget(&self, x: i32, y: i32) -> i32 {
        0
    }

    fn mset(&mut self, x: i32, y: i32, value: i32) {
        todo!()
    }

    fn mouse(&self) -> MouseInput {
        self.input.mouse.clone()
    }

    fn music(&mut self, track: i32, _opts: egg_core::tic80_api::core::MusicOptions) {
        if track == -1 {
            self.music = None;
        } else {
            self.music = Some(track as usize);
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

    fn peek(&self, address: i32, bits: u8) -> u8 {
        todo!()
    }

    fn peek4(&self, address: i32) -> u8 {
        todo!()
    }

    fn peek2(&self, address: i32) -> u8 {
        todo!()
    }

    fn peek1(&self, address: i32) -> u8 {
        todo!()
    }

    fn pmem(&mut self, address: i32, value: i64) -> i32 {
        todo!()
    }

    fn poke(&mut self, address: i32, value: u8, bits: u8) {
        todo!()
    }

    fn poke4(&mut self, address: i32, value: u8) {
        todo!()
    }

    fn poke2(&mut self, address: i32, value: u8) {
        todo!()
    }

    fn poke1(&mut self, address: i32, value: u8) {
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
                10 => {
                    dx = x;
                    dy += 8;
                }
                _ => {
                    self.draw_letter(char, dx, dy);
                    dx += 8;
                }
            };
            max_width = max_width.max(dx - x);
        }
        max_width
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        let mut paint = Paint {
            anti_alias: false,
            ..Default::default()
        };
        paint.set_color(self.colour(color));
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            match self.vbank {
                0 => &mut self.screen,
                1 => &mut self.overlay_screen,
                _ => unreachable!(),
            }
            .fill_rect(rect, &paint, Transform::identity(), None);
        }
    }

    fn rectb(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        let mut paint = Paint {
            anti_alias: false,
            ..Default::default()
        };
        paint.set_color(self.colour(color));
        let rect = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32).unwrap();

        match self.vbank {
            0 => &mut self.screen,
            1 => &mut self.overlay_screen,
            _ => unreachable!(),
        }
        .fill_rect(rect, &paint, Transform::identity(), None);
    }

    fn sfx(&mut self, sfx_id: i32, opts: egg_core::tic80_api::core::SfxOptions) {
        self.sounds.push((sfx_id, opts));
    }

    fn spr(&mut self, id: i32, x: i32, y: i32, opts: egg_core::tic80_api::core::SpriteOptions) {
        match (opts.w, opts.h) {
            (1, 1) => self.draw_sprite(id, x, y),
            (w, h) => {
                for j in 0..h {
                    for i in 0..w {
                        self.draw_sprite(id + i + j * 32, x + i * 8, y + j * 8);
                    }
                }
            }
        }
    }

    fn sync(&mut self, mask: i32, bank: u8, to_cart: bool) {
        ()
    }

    fn time(&self) -> f32 {
        todo!()
    }

    fn tstamp(&self) -> u32 {
        todo!()
    }

    fn trace_alloc(text: impl AsRef<str>, color: u8) {
        println!("{}", text.as_ref());
    }

    fn tri(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, color: u8) {
        todo!()
    }

    fn trib(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, color: u8) {
        todo!()
    }

    fn ttri(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
        u1: f32,
        v1: f32,
        u2: f32,
        v2: f32,
        u3: f32,
        v3: f32,
        opts: egg_core::tic80_api::core::TTriOptions,
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
}
