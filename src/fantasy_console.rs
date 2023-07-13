use egg_core::{system::{ConsoleApi, EggMemory}, gamestate::EggInput, tic80_api::core::MouseInput};

pub struct FantasyConsole {
    memory: EggMemory,
    input: EggInput,
}

impl FantasyConsole {
    pub fn new() -> Self {
        Self {
            memory: EggMemory::new(),
            input: EggInput::new(),
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

    fn get_sprite_flags(&mut self) -> &mut [u8; 512] {
        todo!()
    }

    fn get_system_font(&mut self) -> &mut [u8; 2048] {
        todo!()
    }

    fn get_palette(&mut self) -> &mut [[u8; 3]; 16] {
        todo!()
    }

    fn get_palette_map(&mut self) -> &mut [u8; 16] {
        todo!()
    }

    fn get_border_colour(&mut self) -> &mut u8 {
        todo!()
    }

    fn get_screen_offset(&mut self) -> &mut [i8; 2] {
        todo!()
    }

    fn get_mouse_cursor(&mut self) -> &mut u8 {
        todo!()
    }

    fn get_blit_segment(&mut self) -> &mut u8 {
        todo!()
    }

    fn btn(&self, index: i32) -> i32 {
        todo!()
    }

    fn btnp(&self, index: i32, hold: i32, period: i32) -> bool {
        todo!()
    }

    fn clip(&mut self, x: i32, y: i32, width: i32, height: i32) {
        todo!()
    }

    fn cls(&mut self, color: u8) {
        todo!()
    }

    fn circ(&mut self, x: i32, y: i32, radius: i32, color: u8) {
        todo!()
    }

    fn circb(&mut self, x: i32, y: i32, radius: i32, color: u8) {
        todo!()
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

    fn font_alloc(text: impl AsRef<str>, x: i32, y: i32, opts: egg_core::tic80_api::core::FontOptions) -> i32 {
        todo!()
    }

    fn key(&self, index: i32) -> bool {
        todo!()
    }

    fn keyp(&self, index: i32, hold: i32, period: i32) -> bool {
        todo!()
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, color: u8) {
        todo!()
    }

    fn map(&mut self, opts: egg_core::tic80_api::core::MapOptions) {
        todo!()
    }

    fn mget(&self, x: i32, y: i32) -> i32 {
        todo!()
    }

    fn mset(&mut self, x: i32, y: i32, value: i32) {
        todo!()
    }

    fn mouse(&self) -> MouseInput {
        self.input.mouse.clone()
    }

    fn music(&mut self, track: i32, opts: egg_core::tic80_api::core::MusicOptions) {
        todo!()
    }

    fn pix(&mut self, x: i32, y: i32, color: i8) -> u8 {
        todo!()
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

    fn print_alloc(&mut self, text: impl AsRef<str>, x: i32, y: i32, opts: egg_core::tic80_api::core::PrintOptions) -> i32 {
        todo!()
    }

    fn print_raw(&mut self, text: &str, x: i32, y: i32, opts: egg_core::tic80_api::core::PrintOptions) -> i32 {
        todo!()
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        todo!()
    }

    fn rectb(&mut self, x: i32, y: i32, w: i32, h: i32, color: u8) {
        todo!()
    }

    fn sfx(&mut self, sfx_id: i32, opts: egg_core::tic80_api::core::SfxOptions) {
        todo!()
    }

    fn spr(&mut self, id: i32, x: i32, y: i32, opts: egg_core::tic80_api::core::SpriteOptions) {
        todo!()
    }

    fn sync(&mut self, mask: i32, bank: u8, to_cart: bool) {
        todo!()
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
        todo!()
    }

    fn sync_helper(&mut self) -> &mut egg_core::tic80_api::helpers::SyncHelper {
        todo!()
    }

    fn rng(&mut self) -> &mut egg_core::rand::Lcg64Xsh32 {
        todo!()
    }

    fn previous_gamepad(&mut self) -> &mut [u8; 4] {
        &mut self.input.previous_gamepads
    }
    
    fn previous_mouse(&mut self) -> &mut MouseInput {
        &mut self.input.previous_mouse
    }
}