//! Fixed-capacity structure-of-arrays pool for one class. Every array is
//! sized at construction; nothing here allocates afterwards.

use mscent_math::Vec3;

/// A particle at birth.
#[derive(Clone, Copy, Debug)]
pub struct Born {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Seconds, already clamped positive by the emitter.
    pub life: f32,
    pub key: u8,
    pub surface: u32,
}

pub struct Pool {
    len: usize,
    px: Vec<f32>,
    py: Vec<f32>,
    pz: Vec<f32>,
    vx: Vec<f32>,
    vy: Vec<f32>,
    vz: Vec<f32>,
    /// Age as a fraction of life, `0..1`.
    t: Vec<f32>,
    inv_life: Vec<f32>,
    key: Vec<u8>,
    surface: Vec<u32>,
    /// Seconds since this particle last shed a trail particle.
    trail_acc: Vec<f32>,
}

impl Pool {
    pub fn new(capacity: usize) -> Self {
        let f = || vec![0.0; capacity];
        Self {
            len: 0,
            px: f(),
            py: f(),
            pz: f(),
            vx: f(),
            vy: f(),
            vz: f(),
            t: f(),
            inv_life: f(),
            key: vec![0; capacity],
            surface: vec![0; capacity],
            trail_acc: f(),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn capacity(&self) -> usize {
        self.px.len()
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn pos(&self, i: usize) -> Vec3 {
        Vec3::new(self.px[i], self.py[i], self.pz[i])
    }

    pub fn vel(&self, i: usize) -> Vec3 {
        Vec3::new(self.vx[i], self.vy[i], self.vz[i])
    }

    pub fn set_pos_vel(&mut self, i: usize, p: Vec3, v: Vec3) {
        (self.px[i], self.py[i], self.pz[i]) = (p.x, p.y, p.z);
        (self.vx[i], self.vy[i], self.vz[i]) = (v.x, v.y, v.z);
    }

    pub fn t(&self, i: usize) -> f32 {
        self.t[i]
    }

    pub fn inv_life(&self, i: usize) -> f32 {
        self.inv_life[i]
    }

    pub fn set_t(&mut self, i: usize, t: f32) {
        self.t[i] = t;
    }

    pub fn key(&self, i: usize) -> u8 {
        self.key[i]
    }

    pub fn surface(&self, i: usize) -> u32 {
        self.surface[i]
    }

    pub fn trail_acc(&self, i: usize) -> f32 {
        self.trail_acc[i]
    }

    pub fn set_trail_acc(&mut self, i: usize, v: f32) {
        self.trail_acc[i] = v;
    }

    /// Adds a particle. A full pool drops it, unless `replace_oldest`, in
    /// which case it takes the slot of the particle nearest its death.
    /// Returns whether the particle was stored.
    pub fn push(&mut self, b: &Born, replace_oldest: bool) -> bool {
        let slot = if self.len < self.capacity() {
            self.len += 1;
            self.len - 1
        } else if replace_oldest && self.len > 0 {
            self.oldest()
        } else {
            return false;
        };
        self.set_pos_vel(slot, b.pos, b.vel);
        self.t[slot] = 0.0;
        self.inv_life[slot] = 1.0 / b.life;
        self.key[slot] = b.key;
        self.surface[slot] = b.surface;
        self.trail_acc[slot] = 0.0;
        true
    }

    /// Index of the largest age fraction. Fractions, not seconds, because a
    /// pool mixes lifetimes and the one closest to vanishing is cheapest to
    /// lose. Only scanned when an Essential pool is full.
    fn oldest(&self) -> usize {
        let mut best = 0;
        for i in 1..self.len {
            if self.t[i] > self.t[best] {
                best = i;
            }
        }
        best
    }

    /// Removes particle `i` by moving the last one into its place.
    pub fn swap_remove(&mut self, i: usize) {
        let last = self.len - 1;
        if i != last {
            self.px[i] = self.px[last];
            self.py[i] = self.py[last];
            self.pz[i] = self.pz[last];
            self.vx[i] = self.vx[last];
            self.vy[i] = self.vy[last];
            self.vz[i] = self.vz[last];
            self.t[i] = self.t[last];
            self.inv_life[i] = self.inv_life[last];
            self.key[i] = self.key[last];
            self.surface[i] = self.surface[last];
            self.trail_acc[i] = self.trail_acc[last];
        }
        self.len = last;
    }
}
