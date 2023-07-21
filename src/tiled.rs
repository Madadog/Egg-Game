use serde::{Serialize, Deserialize};
use serde_json::{self, Value};

#[derive(Debug, Deserialize, Serialize)]
pub struct TiledLayer {
    pub width: usize,
    pub height: usize,
    pub data: Vec<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TiledMap {
    pub width: usize,
    pub height: usize,
    pub layers: Vec<TiledLayer>,
}
impl TiledMap {
    pub fn get(&self, layer: usize, x: usize, y: usize) -> Option<usize> {
        self.layers.get(layer).and_then(|layer| {
            layer.data.get(y * layer.width + x).cloned()
        })
    }
}

// Tests for map serialization/deserialization:
mod tests {
    use super::*;

    #[test]
    fn test_map_serialization() {
        let map = TiledMap {
            width: 10,
            height: 10,
            layers: Vec::new(),
        };
        let json = serde_json::to_string(&map).unwrap();
        println!("{}", json);
        let map2: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, map2.width);
        assert_eq!(map.height, map2.height);
    }
    #[test]
    fn test_map_deserialization() {
        let json = std::fs::read_to_string("assets/map/bank1.json").unwrap();
        let map: TiledMap = serde_json::from_str(&json).unwrap();
        assert_eq!(map.width, 240);
        assert_eq!(map.height, 136);
    }
}