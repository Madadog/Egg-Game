/// For simplicity all layers under a map have the same width and height.
/// Ordering of layers is: first at the bottom, last at the top.
#[derive(Clone, Debug)]
pub struct GameMap {
    width: usize,
    height: usize,
    pub layers: Vec<MapLayer>,
}
impl GameMap {
    pub fn new(width: usize, height: usize, layers: Vec<MapLayer>) -> Self {
        Self {
            width,
            height,
            layers,
        }
    }
    pub fn new_empty(width: usize, height: usize, layers: usize) -> Self {
        Self::new(
            width,
            height,
            (0..layers)
                .map(|_| MapLayer::new_empty(width, height))
                .collect(),
        )
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
}

#[derive(Clone, Debug)]
pub struct MapLayer {
    pub name: String,
    width: usize,
    height: usize,
    pub data: Vec<usize>,
}
impl MapLayer {
    pub fn new(name: String, width: usize, height: usize, data: Vec<usize>) -> Self {
        assert!(width * height == data.len());
        Self {
            name,
            width,
            height,
            data,
        }
    }
    pub fn new_empty(width: usize, height: usize) -> Self {
        Self::new(String::new(), width, height, vec![0; width * height])
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn get(&self, x: usize, y: usize) -> Option<usize> {
        self.data.get(y * self.width + x).copied()
    }
    pub fn get_mut(&mut self, x: usize, y: usize) -> Option<&mut usize> {
        self.data.get_mut(y * self.width + x)
    }
}
