//! The original TIC-80 sprite-flag blob and its decoder — now a **test oracle
//! only**, no longer the runtime source of truth.
//!
//! The runtime per-tile collision-flag table ([`crate::drawstate::DrawState::sprite_flags`],
//! consulted by [`crate::map::layer_collides_flags`]) is loaded from the Tiled
//! tileset `assets/maps/tiles.tsj` (its per-tile `flags` int property), parsed
//! by [`crate::data::tmj::TilesetFile`]. This module's hardcoded hex blob is the
//! frozen export it was generated from: the `tsj_oracle` test asserts the two
//! agree exactly, pinning the data file to the historical flags. Nothing in the
//! shipping game reads the blob — only the test does.
//!
//! Full deletion waits for the all-maps-modern sweep (legacy bank-window maps
//! still feed [`layer_collides_flags`] the runtime table, which now just arrives
//! from data instead of from here). Until then this stays as the oracle.
//!
//! Blob layout is a TIC-80 quirk preserved byte-for-byte: the source string is
//! read in 16-tile rows but written into a 32-tile-wide table, and each pair of
//! hex digits is byte-swapped before parsing (see [`parse_sprite_flags`]). That
//! quirk is exactly what was *baked into* `tiles.tsj` on export — the tile ids
//! there are honest sheet positions, so the data file carries no quirk and the
//! quirk now lives only in this oracle.

/// Build the default 2048-entry sprite-flag table by decoding the built-in
/// blob into a zeroed table.
pub fn default_sprite_flags() -> Vec<u8> {
    let mut flags = vec![0u8; 2048];
    parse_sprite_flags(&mut flags, SPRITE_FLAGS_BLOB);
    flags
}

/// Decode the hex `blob` into `flags` in place. Each two characters describe one
/// tile: they're swapped (`char2`,`char1`) then parsed as base-16, and stored at
/// the table index `x + y * 32` where `(x, y) = (i % 16, i / 16)` for the i-th
/// pair. This 16-wide read / 32-wide write split is load-bearing — it matches the
/// original cartridge's layout, so the math must stay exactly as-is.
pub fn parse_sprite_flags(flags: &mut [u8], blob: &str) {
    let chars: Vec<char> = blob.chars().collect();
    for i in 0..chars.len() / 2 {
        let (char1, char2) = (chars[i * 2], chars[i * 2 + 1]);
        let mut pair = String::new();
        pair.push(char2);
        pair.push(char1);
        let flag = u8::from_str_radix(&pair, 16).unwrap();
        let (x, y) = (i % 16, i / 16);
        let index = x + y * 32;
        flags[index] = flag;
    }
}

/// The built-in sprite sheet's flags, exported from the original TIC-80
/// cartridge as one hex string (four cartridge blocks concatenated).
const SPRITE_FLAGS_BLOB: &str = concat!(
    "00100000000000000000000000000000000000801000000000000000002020000010101010500000001000000000000000101030101000000000001010000000101010002000000000301010400000001000100000400010500000000000000010101010108020100000000000101010203000301080302000000000001010101010100000100010001010100000000010001000001000100010100000000000000000000010101030303010000010100000000000000000000000002030203000000000000000000000101010400000000000000010000000203010102000100000000000000000000000000010101000000000000000100010a060b0101020",
    "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "000000001010101000000000000000000070601010700000000000000000000010000000001000000000000000000000601010606060000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000000000000000000000000000100000000000000000000000000000000010100000000000000000000000001010101000000000000000700000302030200000000000000000d0006000",
    "00000000101010100000000000000000000000200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000010001000000000000000000000000000100010000000000000000000000010001010100000000000000000000000000000000000000000000000000000000000001010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
);

#[cfg(test)]
mod tests {
    use super::default_sprite_flags;
    use crate::data::tmj::tileset_from_json;

    /// Oracle: the flag table the shipping game loads from `assets/maps/tiles.tsj`
    /// must equal, tile for tile, the table the historical blob decodes to. This
    /// is the same cross-check pattern as `eggtext_matches_en_json` — the data
    /// file is the runtime source of truth, the blob is the frozen reference it
    /// was generated from, and this pins the two together so an accidental edit
    /// to either is caught. Reads the real asset via the `../assets/` path the
    /// other egg_core data tests already use.
    #[test]
    fn tsj_oracle() {
        let bytes = std::fs::read("../assets/maps/tiles.tsj").expect("read tiles.tsj");
        let tileset = tileset_from_json(&bytes).expect("parse tiles.tsj");
        let from_tsj = tileset.flag_table();
        let from_blob = default_sprite_flags();
        assert_eq!(
            from_tsj.len(),
            from_blob.len(),
            "tiles.tsj tilecount must size the table like the blob does"
        );
        assert_eq!(
            from_tsj, from_blob,
            "tiles.tsj-derived flags must match the historical blob exactly"
        );
    }
}
