use tic80_api::core::{
        FontOptions, MapOptions, MouseInput, MusicOptions, PrintOptions, SfxOptions, SpriteOptions,
        TTriOptions,
    };

use crate::{rand::Lcg64Xsh32, data::{save, sound::SfxData}};

pub struct SyncHelper {
    already_synced: bool,
    last_bank: u8,
}

impl SyncHelper {
    pub const fn new() -> Self {
        SyncHelper {
            already_synced: false,
            last_bank: 0,
        }
    }
    pub fn step(&mut self) {
        self.already_synced = false;
    }
    /// Sync can only be called once per frame. Returns result to indicate failure or success.
    /// Mask lets you switch out sections of cart data:
    /// * all     = 0    -- 0
    /// * tiles   = 1<<0 -- 1
    /// * sprites = 1<<1 -- 2
    /// * map     = 1<<2 -- 4
    /// * sfx     = 1<<3 -- 8
    /// * music   = 1<<4 -- 16
    /// * palette = 1<<5 -- 32
    /// * flags   = 1<<6 -- 64
    /// * screen  = 1<<7 -- 128 (as of 0.90)
    pub fn sync(&mut self, system: &mut impl ConsoleApi, mask: i32, bank: u8) -> Result<(), ()> {
        if self.already_synced() {
            Err(())
        } else {
            self.already_synced = true;
            self.last_bank = bank;
            system.sync(mask, bank, false);
            Ok(())
        }
    }
    pub fn sync2(&mut self, mask: i32, bank: u8) -> Result<(), ()> {
        if self.already_synced() {
            Err(())
        } else {
            self.already_synced = true;
            self.last_bank = bank;
            Ok(())
        }
    }
    pub fn already_synced(&self) -> bool {
        self.already_synced
    }
    pub fn last_bank(&self) -> u8 {
        self.last_bank
    }
}


#[derive(Clone, Copy, Debug)]
pub struct EggMemory {
    pub memory: [u8; 1024],
}
impl EggMemory {
    pub fn new() -> Self {
        Self {
            memory: [0; 1024],
        }
    }
    pub fn from_array(array: [u8; 1024]) -> Self {
        Self { memory: array }
    }
    pub fn is(&self, bit: save::PmemBit) -> bool {
        bit.is_true_with(&self.memory)
    }
    pub fn set(&mut self, bit: save::PmemBit) {
        bit.set_true_with(&mut self.memory);
    }
    pub fn clear(&mut self, bit: save::PmemBit) {
        bit.set_false_with(&mut self.memory);
    }
    pub fn toggle(&mut self, bit: save::PmemBit) {
        bit.toggle_with(&mut self.memory);
    }
    pub fn get_byte(&self, byte: save::PmemU8) -> u8 {
        self.memory[byte.index()]
    }
    pub fn set_byte(&mut self, byte: save::PmemU8, value: u8) {
        self.memory[byte.index()] = value;
    }
}

#[derive(Clone, Debug)]
pub struct DrawParams<'a> {
    // (i32, i32, i32, SpriteOptions, Option<u8>, u8)
    pub index: i32,
    pub x: i32,
    pub y: i32,
    pub options: SpriteOptions<'a>,
    pub outline: Option<u8>,
    pub palette_rotate: u8,
}

impl<'a> DrawParams<'a> {
    pub fn new(
        index: i32,
        x: i32,
        y: i32,
        options: SpriteOptions<'a>,
        outline: Option<u8>,
        palette_rotate: u8,
    ) -> Self {
        Self {
            index,
            x,
            y,
            options,
            outline,
            palette_rotate,
        }
    }
    pub fn draw(self, system: &mut impl ConsoleApi) {
        system.palette_map_rotate(self.palette_rotate);
        if let Some(outline) = self.outline {
            system.spr_outline(self.index, self.x, self.y, self.options, outline);
        } else {
            system.spr(self.index, self.x, self.y, self.options);
        }
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.options.h * 8
    }
}

/// Abstracts away all static memory accesses
pub trait ConsoleApi {
    // TIC-80 RAM
    fn get_framebuffer(&mut self) -> &mut [u8; 16320];
    fn get_tiles(&mut self) -> &mut [u8; 8192];
    fn get_sprites(&mut self) -> &mut [u8; 8192];
    fn get_map(&mut self) -> &mut [u8; 32640];
    fn get_gamepads(&mut self) -> &mut [u8; 4];
    fn get_mouse(&mut self) -> &mut MouseInput;
    fn get_keyboard(&mut self) -> &mut [u8; 4];
    fn get_sfx_state(&mut self) -> &mut [u8; 16];
    fn get_sound_registers(&mut self) -> &mut [u8; 72];
    fn get_waveforms(&mut self) -> &mut [u8; 256];
    fn get_sfx(&mut self) -> &mut [u8; 4224];
    fn get_music_patterns(&mut self) -> &mut [u8; 11520];
    fn get_music_tracks(&mut self) -> &mut [u8; 408];
    fn get_sound_state(&mut self) -> &mut [u8; 4];
    fn get_stereo_volume(&mut self) -> &mut [u8; 4];
    fn memory(&mut self) -> &mut EggMemory;
    fn get_sprite_flags(&mut self) -> &mut [u8];
    fn get_system_font(&mut self) -> &mut [u8; 2048];

    // TIC-80 VRAM
    fn get_palette(&mut self) -> &mut [[u8; 3]; 16];
    fn get_palette_map(&mut self) -> &mut [u8; 16];
    fn get_border_colour(&mut self) -> &mut u8;
    fn get_screen_offset(&mut self) -> &mut [i8; 2];
    fn get_mouse_cursor(&mut self) -> &mut u8;
    fn get_blit_segment(&mut self) -> &mut u8;

    // TIC-80 API
    fn btn(&self, index: i32) -> bool;
    fn btnp(&self, index: i32, hold: i32, period: i32) -> bool;
    fn clip(&mut self, x: i32, y: i32, width: i32, height: i32);
    fn cls(&mut self, color: u8);
    fn circ(&mut self, x: i32, y: i32, radius: i32, color: u8);
    fn circb(&mut self, x: i32, y: i32, radius: i32, color: u8);
    fn elli(&mut self, x: i32, y: i32, a: i32, b: i32, color: u8);
    fn ellib(&mut self, x: i32, y: i32, a: i32, b: i32, color: u8);
    fn exit(&mut self);
    fn fget(&self, sprite_index: i32, flag: i8) -> bool;
    fn fset(&mut self, sprite_index: i32, flag: i8, value: bool);
    fn font_raw(text: &str, x: i32, y: i32, opts: FontOptions) -> i32;
    fn font_alloc(text: impl AsRef<str>, x: i32, y: i32, opts: FontOptions) -> i32;
    fn key(&self, index: i32) -> bool;
    fn keyp(&self, index: i32, hold: i32, period: i32) -> bool;
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u8);
    // `remap` is not yet implemented by the TIC-80 WASM runtime, so for now its type is a raw i32.
    fn map(&mut self, opts: MapOptions);
    // These clash with rustc builtins, so they are reimplemented in the wrappers.
    // fn memcpy(dest: i32, src: i32, length: i32);
    // fn memset(address: i32, value: i32, length: i32);

    fn mget(&self, x: i32, y: i32) -> i32;
    fn mset(&mut self, x: i32, y: i32, value: i32);
    fn mouse(&self) -> MouseInput;
    fn music(&mut self, track: i32, opts: MusicOptions);
    fn pix(&mut self, x: i32, y: i32, color: u8) -> u8;
    fn peek(&self, address: i32, bits: u8) -> u8;
    fn peek4(&self, address: i32) -> u8;
    fn peek2(&self, address: i32) -> u8;
    fn peek1(&self, address: i32) -> u8;
    fn pmem(&mut self, address: i32, value: i64) -> i32;
    fn poke(&mut self, address: i32, value: u8, bits: u8);
    fn poke4(&mut self, address: i32, value: u8);
    fn poke2(&mut self, address: i32, value: u8);
    fn poke1(&mut self, address: i32, value: u8);
    fn print_alloc(&mut self, text: impl AsRef<str>, x: i32, y: i32, opts: PrintOptions) -> i32;
    fn print_raw(&mut self, text: &str, x: i32, y: i32, opts: PrintOptions) -> i32;
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8);
    fn rectb(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8);
    fn sfx(&mut self, sfx_id: i32, opts: SfxOptions);
    fn spr(&mut self, id: i32, x: i32, y: i32, opts: SpriteOptions);
    fn sync(&mut self, mask: i32, bank: u8, to_cart: bool);
    fn time(&self) -> f32;
    fn tstamp(&self) -> u32;
    fn trace_alloc(text: impl AsRef<str>, color: u8);
    fn tri(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, color: u8);
    fn trib(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x3: f32, y3: f32, color: u8);
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
        opts: TTriOptions,
    );
    fn vbank(&mut self, bank: u8) -> u8;

    // Other things
    fn sync_helper(&mut self) -> &mut SyncHelper;
    fn rng(&mut self) -> &mut Lcg64Xsh32;
    fn previous_gamepad(&mut self) -> &mut [u8; 4];
    fn previous_mouse(&mut self) -> &mut MouseInput;

    // helpers
    fn palette_map_swap(&mut self, from: u8, to: u8) {
        let from: i32 = (from % 16).into();
        assert!(from >= 0);
        self.get_palette_map()[from as usize] = to;
    }
    fn palette_map_set_all(&mut self, to: u8) {
        for i in 0..=15 {
            self.get_palette_map()[i] = to;
        }
    }
    fn set_palette_map(&mut self, map: [u8; 16]) {
        for (i, item) in map.into_iter().enumerate() {
            self.get_palette_map()[i] = item;
        }
    }
    fn palette_map_reset(&mut self) {
        for i in 0..=15 {
            self.get_palette_map()[i] = i as u8;
        }
    }
    fn palette_map_rotate(&mut self, amount: u8) {
        for i in 0..=15 {
            self.get_palette_map()[i] = i as u8 + amount;
        }
    }
    fn set_palette_colour(&mut self, index: u8, rgb: [u8; 3]) {
        let index: usize = (index % 16).into();
        self.get_palette()[index] = rgb;
    }
    fn set_palette(&mut self, colours: [[u8; 3]; 16]) {
        for (i, colour) in colours.iter().enumerate() {
            self.set_palette_colour(i as u8, *colour);
        }
    }
    fn draw_outline(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        sprite_options: SpriteOptions,
        outline_colour: u8,
    ) {
        let old_map = *self.get_palette_map();
        self.palette_map_set_all(outline_colour);
        self.spr(id, x + 1, y, sprite_options.clone());
        self.spr(id, x - 1, y, sprite_options.clone());
        self.spr(id, x, y + 1, sprite_options.clone());
        self.spr(id, x, y - 1, sprite_options);
        self.set_palette_map(old_map);
    }
}

impl<T: ConsoleApi> ConsoleHelper for T {}

pub trait ConsoleHelper: ConsoleApi {
    // Helper functions
    fn play_sound(&mut self, sfx_data: SfxData) {
        self.sfx(sfx_data.id, sfx_data.options);
    }
    fn update_previous_gamepad(&mut self) {
        let buttons = self.get_gamepads();
        *self.previous_gamepad() = *buttons;
    }
    fn update_previous_mouse(&mut self) {
        let mouse = self.get_mouse();
        *self.previous_mouse() = mouse.clone();
    }
    fn mem_btn(&mut self, id: u8) -> bool {
        let controller: usize = (id / 8).min(3).into();
        let id = id % 8;
        let buttons = self.get_gamepads()[controller];
        (1 << id) & buttons != 0
    }
    fn mem_btnp(&mut self, id: u8) -> bool {
        let controller: usize = (id / 8).min(3).into();
        let id = id % 8;
        let buttons = self.get_gamepads()[controller];
        let previous = self.previous_gamepad()[controller];
        (1 << id) & buttons != (1 << id) & previous && (1 << id) & buttons != 0
    }
    /// Returns true if any button was pressed. Ignores button releases.
    fn any_btnp(&mut self) -> bool {
        let buttons = *self.get_gamepads();
        let previous = *self.previous_gamepad();
        let mut flag = false;
        for (b0, b1) in previous.iter().zip(buttons.iter()) {
            flag |= b0.count_ones() < b1.count_ones();
        }
        flag
    }
    /// Returns true if any button was pressed or released
    fn any_btnpr(&mut self) -> bool {
        let buttons = *self.get_gamepads();
        let previous = *self.previous_gamepad();
        buttons != previous
    }
    fn mouse_delta(&mut self) -> MouseInput {
        let old = self.previous_mouse().clone();
        let new = self.get_mouse();
        MouseInput {
            x: new.x - old.x,
            y: new.y - old.y,
            left: new.left && !old.left,
            middle: new.middle && !old.middle,
            right: new.right && !old.right,
            ..*new
        }
    }
    fn zero_pmem(&mut self) {
        self.memory().memory.fill(0);
    }
    fn fade_palette(&mut self, from: [[u8; 3]; 16], to: [[u8; 3]; 16], amount: u16) {
        let amount = amount.min(256);
        for (index, (colour1, colour2)) in from.iter().zip(to.iter()).enumerate() {
            let mut rgb = [0; 3];
            for (j, (component1, component2)) in colour1.iter().zip(colour2.iter()).enumerate() {
                rgb[j] = ((*component1 as u16 * (256 - amount) + *component2 as u16 * amount) >> 8)
                    as u8;
            }
            self.set_palette_colour(index as u8, rgb);
        }
    }
    fn fade_palette_colour(&mut self, index: u8, from: [u8; 3], to: [u8; 3], amount: u16) {
        let amount = amount.min(256);
        let index: usize = (index % 16).into();
        let mut rgb = [0; 3];
        for (j, (component1, component2)) in from.iter().zip(to.iter()).enumerate() {
            rgb[j] =
                ((*component1 as u16 * (256 - amount) + *component2 as u16 * amount) >> 8) as u8;
        }
        self.set_palette_colour(index as u8, rgb);
    }
    fn set_border_colour(&mut self, colour: u8) {
        *self.get_border_colour() = colour;
    }
    fn screen_offset(&mut self, horizontal: i8, vertical: i8) {
        self.get_screen_offset()[0] = horizontal;
        self.get_screen_offset()[1] = vertical;
    }
    fn draw_ovr2<T: FnMut(&mut Self)>(&mut self, mut draw: T) {
        self.vbank(1);
        draw(self);
        self.vbank(0);
    }
    fn draw_ovr<T: FnMut()>(&mut self, mut draw: T) {
        self.vbank(1);
        draw();
        self.vbank(0);
    }
    fn get_pmem(&mut self, address: usize) -> u8 {
        let address = address.min(1023);
        self.memory().memory[address]
    }
    fn set_pmem(&mut self, address: usize, value: u8) {
        let address = address.min(1023);
        self.memory().memory[address] = value;
    }
    /// Valid values:
    ///
    /// 0000 SYS GFX
    /// 0001 FONT
    ///
    /// 0010 4bpp BG Page 0
    /// 0011 4bpp FG Page 0
    ///
    /// 0100 2bpp BG Page 0
    /// 0101 2bpp BG Page 1
    /// 0110 2bpp FG Page 0
    /// 0111 2bpp FG Page 1
    ///
    /// 1000 1bpp BG Page 0
    /// 1001 1bpp BG Page 1
    /// 1010 1bpp BG Page 2
    /// 1011 1bpp BG Page 3
    /// 1100 1bpp FG Page 0
    /// 1101 1bpp FG Page 1
    /// 1110 1bpp FG Page 2
    /// 1111 1bpp FG Page 3
    fn blit_segment(&mut self, value: u8) {
        *self.get_blit_segment() = value;
    }
    fn spr_blit_segment(&mut self, id: i32, x: i32, y: i32, opts: SpriteOptions, blit_seg: u8) {
        let old = *self.get_blit_segment();
        self.blit_segment(blit_seg);
        self.spr(id, x, y, opts);
        self.blit_segment(old);
    }
    fn spr_outline(
        &mut self,
        id: i32,
        x: i32,
        y: i32,
        sprite_options: SpriteOptions,
        outline_colour: u8,
    ) {
        self.draw_outline(id, x, y, sprite_options.clone(), outline_colour);
        self.spr(id, x, y, sprite_options);
    }
    fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, fill: u8, outline: u8) {
        self.rect(x, y, w, h, fill);
        self.rectb(x, y, w, h, outline);
    }
    fn print_raw_centered(&mut self, string: &str, x: i32, y: i32, options: PrintOptions) {
        let string_width = self.print_raw(string, 999, 999, options.clone());
        self.print_raw(string, x - string_width / 2, y, options);
    }
    fn print_alloc_centered(&mut self, string: &str, x: i32, y: i32, options: PrintOptions) {
        let string_width = self.print_alloc(string, 999, 999, options.clone());
        self.print_alloc(string, x - string_width / 2, y, options);
    }
    fn print_raw_shadow(
        &mut self,
        string: &str,
        x: i32,
        y: i32,
        options: PrintOptions,
        shadow_colour: i32,
    ) {
        let shadow_options = PrintOptions {
            color: shadow_colour,
            ..options
        };
        self.print_raw(string, x + 1, y + 1, shadow_options);
        self.print_raw(string, x, y, options);
    }
}
