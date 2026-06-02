use std::collections::HashMap;

use bevy::prelude::{Image, info};
use egg_core::{
    data::{save::SaveData, sound::music::MusicTrack},
    gamestate::EggInput,
    rand::Lcg64Xsh32,
    system::{
        ConsoleApi, Controller, Font, GameMap, HEIGHT, MapLayer, MouseInput, ScanCode,
        SfxOptions,
        WIDTH,
        image::{IndexedImage, Rgba, RgbaImage},
    },
};

use crate::tiled;

// TODO:
// Load interactables from tiled maps
// Separate BG & FG palettes, upgrade BGs.
// Move tiled map parsing/loading into core
// Serialize save/load state, use structs, remove bits.
// Dialogue hashmap
// Make UI actually work: Hierarchical layout, compositional widgets.
// Unified walkaround collision space
// Yolkomatic

// TODO:
// Turn `Creature` into normal entities
// Remove Static{name} from everything, load data from files like a sane program.
// Level editor that serialises to json
// Dialogue dsl and previewer
// Cutscene editor
// Resizable screen

pub struct FantasyConsole {
    pub output_screen: RgbaImage,
    pub font: Font,
    // sprites + indexed_sprites + maps + sprite_flags also live on
    // EggState::draw_state. These copies are kept around for the asset
    // loaders and Collider::from_sprite reads via get_bitmap_indexed.
    pub sprites: RgbaImage,
    pub indexed_sprites: IndexedImage,
    pub maps: Vec<GameMap>,
    pub sprite_flags: Vec<u8>,
    files: HashMap<String, Vec<u8>>,
    music: Option<(MusicTrack, bool)>,
    memory: SaveData,
    sounds: HashMap<String, SfxOptions>,
    input: EggInput,
    rng: Lcg64Xsh32,
    bank: u8,
}

impl FantasyConsole {
    pub fn new() -> Self {
        let mut x = Self {
            output_screen: RgbaImage::new(WIDTH as u32, HEIGHT as u32),
            font: Font::blank(),
            sprites: RgbaImage::new(1, 1),
            indexed_sprites: IndexedImage::new(1, 1),
            maps: Vec::new(),
            files: HashMap::new(),
            sprite_flags: vec![0; 2048],
            music: None,
            sounds: HashMap::new(),
            memory: SaveData::default(),
            input: EggInput::new(),
            rng: Lcg64Xsh32::default(),
            bank: 0,
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
    /// A snapshot of the current persistent save data, for the autosave system
    /// to diff against the last value written to disk.
    pub fn save_data(&self) -> SaveData {
        self.memory
    }
    pub fn blit_to_image(&self, image: &mut [u8]) {
        // Gamestate draw fns composite directly into output_screen each frame.
        image.copy_from_slice(self.output_screen.data());
    }
    pub fn set_font(&mut self, font: &Image) {
        assert!(font.size().x == 128);
        assert!(font.size().y >= 128);
        for (i, c) in self
            .font
            .image_mut()
            .data_mut()
            .iter_mut()
            .zip(font.data.iter().flatten())
        {
            *i = *c;
        }
        self.font.refresh();
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
    /// Convert an RGBA sprite sheet to indexed form by matching each pixel
    /// against `palette`. Pixels that don't match a palette entry become
    /// index 0.
    pub fn set_indexed_sprites(&mut self, sheet: &Image, palette: &[[u8; 3]]) {
        let width = sheet.size().x as usize;
        let height = sheet.size().y as usize;
        let mut data = Vec::with_capacity(width * height);
        'outer: for pixel in sheet
            .data
            .as_ref()
            .expect("Tried to read uninitialised image.")
            .chunks_exact(4)
        {
            for (i, colour) in palette.iter().enumerate() {
                if pixel[0] == colour[0] && pixel[1] == colour[1] && pixel[2] == colour[2] {
                    data.push(i.try_into().unwrap());
                    continue 'outer;
                }
            }
            data.push(0);
        }
        self.indexed_sprites = IndexedImage::from_vec(data, width, height);
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
}

impl ConsoleApi for FantasyConsole {
    fn controllers(&self) -> &[Controller; 4] {
        &self.input.controllers
    }

    fn memory(&mut self) -> &mut SaveData {
        &mut self.memory
    }

    fn get_sprite_flags(&mut self) -> &mut [u8] {
        self.sprite_flags.as_mut_slice()
    }

    fn exit(&mut self) {
        panic!("Perfectly normal shutdown.")
    }

    fn key(&self, scancode: ScanCode) -> bool {
        self.input.key(scancode)
    }

    fn keyp(&self, scancode: ScanCode) -> bool {
        self.input.keyp(scancode)
    }

    fn key_chars(&self) -> &[char] {
        self.input.key_chars()
    }

    fn mouse(&self) -> MouseInput {
        self.input.mouse
    }

    fn music(&mut self, track: Option<&MusicTrack>) {
        info!("Playing track \"{:?}\"", track);
        if let Some(track) = track {
            self.music = Some((track.clone(), false));
        } else {
            self.music = None;
        }
    }

    fn sfx(&mut self, sfx_id: &str, opts: egg_core::system::SfxOptions) {
        self.sounds.insert(sfx_id.to_string(), opts);
    }

    fn trace_alloc(text: impl AsRef<str>, _color: u8) {
        println!("{}", text.as_ref());
    }

    fn bank(&mut self) -> &mut u8 {
        &mut self.bank
    }

    fn rng(&mut self) -> &mut egg_core::rand::Lcg64Xsh32 {
        &mut self.rng
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

    fn get_bitmap_indexed(&self, id: usize) -> &[u8] {
        match id {
            2 => &self.indexed_sprites.data,
            _ => panic!("bitmap {id} does not exist"),
        }
    }

    fn output_image(&mut self) -> &mut RgbaImage {
        &mut self.output_screen
    }

    fn font(&self) -> &Font {
        &self.font
    }
}
