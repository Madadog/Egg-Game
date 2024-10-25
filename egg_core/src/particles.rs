use crate::{position::Vec2, system::ConsoleApi};
use tic80_api::core::StaticSpriteOptions;

#[derive(Clone, Debug)]
pub enum ParticleDraw {
    Rect(i32, i32, u8),
    RectB(i32, i32, u8),
    Circ(i32, u8),
    Spr(i32),
}
impl ParticleDraw {
    pub fn draw_tic80(&self, system: &mut impl ConsoleApi, x: i32, y: i32) {
        match &self {
            ParticleDraw::Rect(w, h, colour) => system.rect(x, y, *w, *h, *colour),
            ParticleDraw::RectB(w, h, colour) => system.rectb(x, y, *w, *h, *colour),
            ParticleDraw::Circ(radius, colour) => system.circ(x, y, *radius, *colour),
            ParticleDraw::Spr(id) => system.spr(*id, x, y, StaticSpriteOptions::transparent_zero()),
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
    pub fn draw_tic80(&self, system: &mut impl ConsoleApi, x_offset: i32, y_offset: i32) {
        let (x, y): (i32, i32) = (self.position.x.into(), self.position.y.into());
        self.draw.draw_tic80(system, x + x_offset, y + y_offset);
    }
}

#[derive(Clone, Debug)]
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
    pub fn draw_tic80(&self, system: &mut impl ConsoleApi, x_offset: i32, y_offset: i32) {
        self.particles
            .iter()
            .for_each(|x| x.draw_tic80(system, x_offset, y_offset));
    }
    pub fn add(&mut self, particle: Particle) {
        self.particles.push(particle)
    }
    pub fn clear(&mut self) {
        self.particles.clear();
        self.shrink_to_fit();
    }
}
