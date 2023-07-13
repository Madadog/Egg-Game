use tiny_skia::Color;

pub fn array_to_colour(array: [u8; 3]) -> Color {
    Color::from_rgba8(array[0], array[1], array[2], 255)
}