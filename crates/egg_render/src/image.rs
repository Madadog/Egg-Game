use std::ops::{Index, IndexMut};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const TRANSPARENT: Self = Self([0, 0, 0, 0]);

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    pub const fn a(self) -> u8 {
        self.0[3]
    }

    pub const fn from_rgb(array: [u8; 3]) -> Self {
        Rgba::new(array[0], array[1], array[2], 255)
    }
}

#[derive(Clone, Debug)]
pub struct RgbaImage {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
        }
    }
    pub fn from_vec(data: Vec<u8>, width: u32, height: u32) -> Self {
        assert_eq!(data.len(), (width * height * 4) as usize);
        Self {
            width,
            height,
            data,
        }
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
    pub fn clone_from(&mut self, other: &RgbaImage) {
        assert_eq!(self.width, other.width);
        assert_eq!(self.height, other.height);
        self.data.copy_from_slice(&other.data);
    }
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Rgba {
        let i = ((x + y * self.width) * 4) as usize;
        Rgba::new(
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        )
    }
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, colour: Rgba) {
        let i = ((x + y * self.width) * 4) as usize;
        self.data[i..i + 4].copy_from_slice(&colour.0);
    }
    #[inline]
    pub fn set_pixel_index(&mut self, index: usize, colour: Rgba) {
        let i = index * 4;
        self.data[i..i + 4].copy_from_slice(&colour.0);
    }
    #[inline]
    pub fn get_pixel_index(&self, index: usize) -> Rgba {
        let i = index * 4;
        Rgba::new(
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        )
    }
    #[inline]
    pub fn alpha_at_index(&self, index: usize) -> u8 {
        self.data[index * 4 + 3]
    }
    pub fn fill(&mut self, colour: Rgba) {
        for chunk in self.data.chunks_exact_mut(4) {
            chunk.copy_from_slice(&colour.0);
        }
    }

    /// Encode this surface as an 8-bit RGBA PNG, returning the file bytes.
    ///
    /// Hand-written with no image/compression dependency — the same call the
    /// repo makes for its base64 codec (see the host's `base64` module): a PNG's
    /// container is a handful of length-prefixed, CRC-guarded chunks, and DEFLATE
    /// permits *stored* (uncompressed) blocks, so a conformant file needs no
    /// entropy coder. The output is therefore larger than a compressed PNG but
    /// decodes identically in any reader (`png`/`image`, browsers, the headless
    /// harness's own boot decode), which is all the test-harness screenshots and
    /// the future web image-layer persistence route need.
    ///
    /// This is that persistence seam: the web save path stores authored
    /// image-layer pixels as PNG bytes (base64-wrapped through `localStorage`),
    /// and this is where those bytes come from — a `RgbaImage` is the engine's
    /// canonical pixel container, so encoding lives on it rather than in a host.
    ///
    /// Structure: the 8-byte signature; `IHDR` (bit depth 8, colour type 6 =
    /// truecolour+alpha, no interlace); one `IDAT` holding a minimal zlib stream
    /// (`0x78 0x01`, no preset dictionary) of stored DEFLATE blocks over the
    /// filter-0-prefixed rows, closed by the raw data's Adler-32; and `IEND`.
    pub fn encode_png(&self) -> Vec<u8> {
        // PNG scanlines are each prefixed with a filter-type byte; we use filter
        // 0 (None), so the "filtered" stream is just the rows with a 0 in front.
        // The Adler-32 and the stored DEFLATE blocks both run over this stream.
        let row_bytes = (self.width * 4) as usize;
        let mut raw = Vec::with_capacity(self.height as usize * (1 + row_bytes));
        for y in 0..self.height {
            raw.push(0);
            let start = y as usize * row_bytes;
            raw.extend_from_slice(&self.data[start..start + row_bytes]);
        }

        let mut out = Vec::new();
        out.extend_from_slice(&PNG_SIGNATURE);

        let mut ihdr = Vec::with_capacity(13);
        ihdr.extend_from_slice(&self.width.to_be_bytes());
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // depth, colour type, compression, filter, interlace
        write_chunk(&mut out, b"IHDR", &ihdr);
        write_chunk(&mut out, b"IDAT", &zlib_stored(&raw));
        write_chunk(&mut out, b"IEND", &[]);
        out
    }

    /// Convert to indexed form by matching each pixel's RGB against `palette`:
    /// the first entry whose R/G/B equal the pixel's becomes that pixel's index,
    /// and a pixel matching none becomes index 0. Alpha is ignored — only the
    /// colour channels are compared, so a transparent pixel still indexes by its
    /// stored colour.
    ///
    /// This is the single home for the sheet-indexing policy: the host converts a
    /// loaded RGBA sheet with it (via `sprites_from_image(...).to_indexed(...)`),
    /// and the headless harness applies it to a decoded sheet the same way, so
    /// both routes produce byte-identical indexed sprites.
    pub fn to_indexed(&self, palette: &[[u8; 3]]) -> IndexedImage {
        let width = self.width as usize;
        let height = self.height as usize;
        let mut data = Vec::with_capacity(width * height);
        'outer: for pixel in self.data.chunks_exact(4) {
            for (i, colour) in palette.iter().enumerate() {
                if pixel[0] == colour[0] && pixel[1] == colour[1] && pixel[2] == colour[2] {
                    data.push(i.try_into().unwrap());
                    continue 'outer;
                }
            }
            data.push(0);
        }
        IndexedImage::from_vec(data, width, height)
    }
}

/// The fixed 8-byte PNG file signature every stream starts with.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// Append one PNG chunk: big-endian length, 4-byte type, data, then the CRC-32
/// (over type+data) the reader validates it against.
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = !crc32_update(crc32_update(0xFFFF_FFFF, kind), data);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Wrap `raw` in a minimal zlib stream: the `0x78 0x01` header (32K window, no
/// preset dictionary — the only two bytes whose `%31` check passes for this
/// mode), the data as stored DEFLATE blocks, and the trailing Adler-32.
fn zlib_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    // Stored blocks cap at 65535 bytes (the 16-bit LEN field). An empty image
    // still needs one final block so the stream is well-formed.
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    } else {
        let mut blocks = raw.chunks(0xffff).peekable();
        while let Some(block) = blocks.next() {
            // Header byte: BTYPE=00 (stored) in bits 1-2, so the byte is just
            // BFINAL — set only on the last block.
            out.push(u8::from(blocks.peek().is_none()));
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes()); // NLEN = one's complement
            out.extend_from_slice(block);
        }
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// Fold `data` into a running CRC-32 (reflected, polynomial `0xEDB88320`). Seed
/// with `0xFFFF_FFFF` and invert the result for the final value — see
/// [`crc32`] (the one-shot form the tests pin against the standard vector).
fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            // Branchless conditional xor: `-(crc & 1)` is all-ones when the low
            // bit is set, masking the polynomial in only then.
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc
}

/// One-shot CRC-32 of `data` (init/xorout `0xFFFF_FFFF`), as PNG chunks use.
#[cfg(test)]
fn crc32(data: &[u8]) -> u32 {
    !crc32_update(0xFFFF_FFFF, data)
}

/// Adler-32 checksum of `data` — the zlib stream trailer. Two rolling sums mod
/// 65521, packed high-`b`/low-`a`.
fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + u32::from(byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

#[derive(Clone, Debug)]
pub struct IndexedImage {
    width: usize,
    height: usize,
    pub data: Vec<u8>,
}
impl IndexedImage {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0; width * height],
        }
    }
    pub fn from_vec(data: Vec<u8>, width: usize, height: usize) -> Self {
        assert_eq!(data.len(), width * height);
        Self {
            width,
            height,
            data,
        }
    }
    pub fn width(&self) -> u32 {
        self.width as u32
    }
    pub fn height(&self) -> u32 {
        self.height as u32
    }
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> u8 {
        self.data[x as usize + y as usize * self.width]
    }
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, colour: u8) {
        self.data[x as usize + y as usize * self.width] = colour;
    }
    pub fn fill(&mut self, colour: u8) {
        self.data.fill(colour);
    }
}
impl Index<(usize, usize)> for IndexedImage {
    type Output = u8;

    fn index(&self, index: (usize, usize)) -> &u8 {
        self.data.get(index.0 + index.1 * self.width).unwrap()
    }
}

impl IndexMut<(usize, usize)> for IndexedImage {
    fn index_mut(&mut self, index: (usize, usize)) -> &mut Self::Output {
        self.data.get_mut(index.0 + index.1 * self.width).unwrap()
    }
}

#[cfg(test)]
mod png_tests {
    use super::*;

    /// The canonical CRC-32 check value: the checksum of the ASCII string
    /// `"123456789"` is `0xCBF43926` for the reflected `0xEDB88320` polynomial
    /// every PNG chunk uses.
    #[test]
    fn crc32_matches_standard_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    /// Adler-32 of `"123456789"`: rolling sums `a=478 (0x1DE)`, `b=2334 (0x91E)`,
    /// packed `b<<16 | a`.
    #[test]
    fn adler32_matches_known_vector() {
        assert_eq!(adler32(b"123456789"), 0x091E_01DE);
        // The empty stream (an empty image's zlib body) is the identity: a=1, b=0.
        assert_eq!(adler32(b""), 0x0000_0001);
    }

    /// The encoded stream opens with the 8-byte PNG signature immediately
    /// followed by an `IHDR` chunk whose big-endian width/height are the image's.
    #[test]
    fn encode_png_signature_and_ihdr_dimensions() {
        let img = RgbaImage::new(7, 3);
        let png = img.encode_png();

        assert!(png.starts_with(&PNG_SIGNATURE), "starts with PNG signature");
        // After the signature: 4-byte length (IHDR is always 13), then "IHDR".
        assert_eq!(&png[8..12], &13u32.to_be_bytes());
        assert_eq!(&png[12..16], b"IHDR");
        // IHDR body opens with width then height, each big-endian u32.
        assert_eq!(&png[16..20], &7u32.to_be_bytes(), "width in IHDR");
        assert_eq!(&png[20..24], &3u32.to_be_bytes(), "height in IHDR");
        // Bit depth 8, colour type 6 (RGBA).
        assert_eq!(png[24], 8, "bit depth");
        assert_eq!(png[25], 6, "colour type RGBA");
    }

    /// The sheet-indexing policy: an exact RGB match takes that palette index, a
    /// non-match falls to 0, and alpha is ignored (a pixel whose RGB matches but
    /// whose alpha differs still indexes by colour).
    #[test]
    fn to_indexed_matches_rgb_ignores_alpha() {
        let palette = [[10, 20, 30], [40, 50, 60]];
        let mut img = RgbaImage::new(3, 1);
        img.set_pixel(0, 0, Rgba::new(10, 20, 30, 255)); // exact match -> index 0
        img.set_pixel(1, 0, Rgba::new(40, 50, 60, 0)); // RGB matches entry 1, alpha differs
        img.set_pixel(2, 0, Rgba::new(99, 99, 99, 255)); // no match -> 0
        let indexed = img.to_indexed(&palette);
        assert_eq!(indexed.get_pixel(0, 0), 0, "exact RGB -> its index");
        assert_eq!(indexed.get_pixel(1, 0), 1, "RGB match despite alpha mismatch");
        assert_eq!(indexed.get_pixel(2, 0), 0, "no palette match -> 0");
    }
}
