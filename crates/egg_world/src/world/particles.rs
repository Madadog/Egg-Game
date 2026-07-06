use egg_render::geometry::Vec2;
use egg_render::SpriteOptions;

#[derive(Clone, Debug)]
pub enum ParticleDraw {
    Rect(i32, i32, u8),
    RectB(i32, i32, u8),
    Circ(i32, u8),
    Spr(i32),
}
impl ParticleDraw {
    pub fn draw_indexed(
        &self,
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
        x: i32,
        y: i32,
    ) {
        use crate::draw_state::PALETTE_MAP_IDENTITY;
        use egg_render::Canvas;
        let bg = layer as usize;
        match *self {
            ParticleDraw::Rect(w, h, colour) => {
                let c = draw_state.colour(colour);
                draw_state.rgba_canvas[bg].fill_rect(x, y, w, h, c);
            }
            ParticleDraw::RectB(w, h, colour) => {
                let c = draw_state.colour(colour);
                draw_state.rgba_canvas[bg].stroke_rect(x, y, w, h, c);
            }
            ParticleDraw::Circ(radius, colour) => {
                let c = draw_state.colour(colour);
                draw_state.rgba_canvas[bg].fill_circle(x, y, radius, c);
            }
            ParticleDraw::Spr(id) => {
                draw_state.spr(
                    layer,
                    &PALETTE_MAP_IDENTITY,
                    id,
                    x,
                    y,
                    SpriteOptions::transparent_zero(),
                );
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Particle {
    draw: ParticleDraw,
    lifetime: usize,
    max_life: usize,
    position: Vec2,
    velocity: Vec2,
}

impl Particle {
    pub fn new(draw: ParticleDraw, max_life: usize, position: Vec2) -> Self {
        Self {
            draw,
            lifetime: 0,
            max_life,
            position,
            velocity: Vec2::new(0, 0),
        }
    }
    pub fn with_velocity(self, velocity: Vec2) -> Self {
        Self { velocity, ..self }
    }
    pub fn step(&mut self) {
        self.position.x += self.velocity.x;
        self.position.y += self.velocity.y;
        self.lifetime += 1;
    }
    pub fn alive(&self) -> bool {
        self.lifetime <= self.max_life
    }
    pub fn draw_indexed(
        &self,
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
        x_offset: i32,
        y_offset: i32,
    ) {
        let (x, y): (i32, i32) = (self.position.x.into(), self.position.y.into());
        self.draw
            .draw_indexed(draw_state, layer, x + x_offset, y + y_offset);
    }
}

#[derive(Clone, Debug, Default)]
pub struct ParticleList {
    particles: Vec<Particle>,
}
impl ParticleList {
    pub const fn new() -> Self {
        Self {
            particles: Vec::new(),
        }
    }
    pub fn step(&mut self) {
        self.particles.iter_mut().for_each(|x| x.step());
        self.particles.retain(|x| x.alive());
    }
    pub fn shrink_to_fit(&mut self) {
        self.particles.shrink_to_fit();
    }
    pub fn draw_indexed(
        &self,
        draw_state: &mut crate::draw_state::DrawState,
        layer: crate::draw_state::LayerId,
        x_offset: i32,
        y_offset: i32,
    ) {
        for p in &self.particles {
            p.draw_indexed(draw_state, layer, x_offset, y_offset);
        }
    }
    pub fn add(&mut self, particle: Particle) {
        self.particles.push(particle)
    }
    pub fn clear(&mut self) {
        self.particles.clear();
        self.shrink_to_fit();
    }
}
