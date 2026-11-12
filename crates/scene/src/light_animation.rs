//! Lights that animate by themselves: a color by temperature, flicker, pulse, fade.
//!
//! ```ron
//! "light": (radius: 0.15, color: (1.0, 1.0, 1.0), intensity: 8.0,
//!     temperature: 2700.0, flicker: 0.3, flicker_rate: 12.0, pulse: 0.2,
//!     pulse_seconds: 2.0, fade: 0.5)
//! ```
//!
//! A light with one of `temperature`, `flicker`, `pulse`, `fade` gets [`LightAnimation`]:
//! the values of the file (`base`) and the time. Each fixed step [`animate_lights`] writes
//! the effective [`Light`] from it. The `Live` form of the light reads and writes `base`,
//! so a track or a `SetField` on `intensity` changes the base and the animation stays on
//! top. A light with none of them gets no new component: a new archetype changes the
//! order of the lights for the renderer.

use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::{Query, Res};
use bevy_ecs::world::World;
use glam::Vec3;
use maki_core::{App, FixedUpdate, Time};
use serde::{Deserialize, Serialize};

use crate::{Light, LightDef, ObjectId};

/// Registers the components and systems of this module.
pub(crate) fn add(app: &mut App) {
    // Before `animate`: a field track then writes the base on top of this step.
    app.add_systems(FixedUpdate, animate_lights.before(crate::animate));
}

/// The animation fields of a light, as in the file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LightAnimDef {
    /// Kelvin. 0: off. Else the color is `kelvin_rgb(temperature) * color`.
    pub temperature: f32,
    /// 0 to 1: how far the intensity drops at most in a step of the flicker.
    pub flicker: f32,
    /// Steps of the flicker in a second.
    pub flicker_rate: f32,
    /// 0 to 1: how far the intensity drops at the low point of a pulse.
    pub pulse: f32,
    /// Length of one pulse.
    pub pulse_seconds: f32,
    /// Seconds from off to full and back for a change of `on`. 0: at once.
    pub fade: f32,
}

impl Default for LightAnimDef {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            flicker: 0.0,
            flicker_rate: 10.0,
            pulse: 0.0,
            pulse_seconds: 1.0,
            fade: 0.0,
        }
    }
}

impl LightAnimDef {
    /// True if the light needs a [`LightAnimation`]. The rate and the length alone do not.
    pub fn is_animated(&self) -> bool {
        self.temperature != 0.0 || self.flicker != 0.0 || self.pulse != 0.0 || self.fade != 0.0
    }

    /// For `skip_serializing_if`.
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The values of the file of an animated light and its time. The `Light` of the entity is
/// the effective light, written from this.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct LightAnimation {
    /// The light of the file, with no animation.
    pub base: Light,
    pub def: LightAnimDef,
    /// Of the flicker pattern: the object id.
    seed: u32,
    /// Fixed steps since the load: a count, so two runs give the same times.
    steps: u64,
    /// 0 (off) to 1 (on): where the fade is.
    level: f32,
}

impl LightAnimation {
    pub fn new(base: Light, def: LightAnimDef, seed: u32) -> Self {
        Self {
            base,
            def,
            seed,
            steps: 0,
            level: if base.on { 1.0 } else { 0.0 },
        }
    }

    /// The level of the fade now: with no fade, at once where `on` says.
    fn level(&self) -> f32 {
        match (self.def.fade > 0.0, self.base.on) {
            (true, _) => self.level,
            (false, true) => 1.0,
            (false, false) => 0.0,
        }
    }

    /// One fixed step of `dt` seconds: the time, and the fade toward `on`.
    pub fn step(&mut self, dt: f64) {
        self.steps += 1;
        let target: f32 = if self.base.on { 1.0 } else { 0.0 };
        self.level = if self.def.fade > 0.0 {
            // The small add: the last step lands on the target, not just above it.
            let delta = dt as f32 / self.def.fade + 1e-6;
            target.clamp(self.level - delta, self.level + delta)
        } else {
            target
        };
    }

    /// The light to show now. `dt`: the length of a fixed step.
    pub fn effective(&self, dt: f64) -> Light {
        let def = &self.def;
        let mut light = self.base;
        if def.temperature > 0.0 {
            light.color *= kelvin_rgb(def.temperature);
        }
        let level = self.level();
        light.on = level > 0.0;
        if !light.on {
            return light;
        }
        let seconds = self.steps as f64 * dt;
        let mut scale = level;
        if def.flicker > 0.0 {
            // The small add: a count of steps x dt can land just below a whole number.
            let step = (seconds * def.flicker_rate.max(0.0) as f64 + 1e-9).floor() as u64;
            scale *= 1.0 - def.flicker * unit_hash(self.seed, step);
        }
        if def.pulse > 0.0 && def.pulse_seconds > 0.0 {
            // 1 at the start of a pulse, 1 - pulse in its middle.
            let phase = (seconds / def.pulse_seconds as f64).fract() as f32;
            let dip = 0.5 * (1.0 - (std::f32::consts::TAU * phase).cos());
            scale *= 1.0 - def.pulse * dip;
        }
        light.intensity *= scale;
        light
    }
}

/// A number in [0, 1) from `seed` and `step` (SplitMix64): the same in each run.
fn unit_hash(seed: u32, step: u64) -> f32 {
    let mut z = ((seed as u64) << 32 ^ step).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// The color of a black body of `kelvin` (clamped to 1667..25000 K): linear RGB, the
/// largest channel 1. White-balanced to 6500 K: `kelvin_rgb(6500)` is (1, 1, 1).
///
/// The chromaticity is the cubic spline of the Planckian locus of Kim et al. (2002, the
/// form in Wikipedia "Planckian locus"); XYZ to linear sRGB by the matrix of the sRGB
/// standard (D65).
pub fn kelvin_rgb(kelvin: f32) -> Vec3 {
    let rgb = planck_rgb(kelvin) / planck_rgb(6500.0);
    rgb / rgb.max_element()
}

/// The chromaticity (x, y) of the Planckian locus at `kelvin`.
pub(crate) fn planck_xy(kelvin: f32) -> (f32, f32) {
    let t = kelvin.clamp(1667.0, 25000.0) as f64;
    let x = if t <= 4000.0 {
        -0.266_123_9e9 / t.powi(3) - 0.234_358_9e6 / t.powi(2) + 0.877_695_6e3 / t + 0.179_910
    } else {
        -3.025_846_9e9 / t.powi(3) + 2.107_037_9e6 / t.powi(2) + 0.222_634_7e3 / t + 0.240_390
    };
    let y = if t <= 2222.0 {
        -1.106_381_4 * x.powi(3) - 1.348_110_20 * x.powi(2) + 2.185_558_32 * x - 0.202_196_83
    } else if t <= 4000.0 {
        -0.954_947_6 * x.powi(3) - 1.374_185_93 * x.powi(2) + 2.091_370_15 * x - 0.167_488_67
    } else {
        3.081_758_0 * x.powi(3) - 5.873_386_70 * x.powi(2) + 3.751_129_97 * x - 0.370_014_83
    };
    (x as f32, y as f32)
}

/// Linear sRGB of the locus at `kelvin`, no channel below 0, not balanced.
fn planck_rgb(kelvin: f32) -> Vec3 {
    let (x, y) = planck_xy(kelvin);
    let (big_x, big_y, big_z) = (x / y, 1.0, (1.0 - x - y) / y);
    Vec3::new(
        3.240_454_2 * big_x - 1.537_138_5 * big_y - 0.498_531_4 * big_z,
        -0.969_266 * big_x + 1.876_010_8 * big_y + 0.041_556 * big_z,
        0.055_643_4 * big_x - 0.204_025_9 * big_y + 1.057_225_2 * big_z,
    )
    .max(Vec3::ZERO)
}

/// Runs in `FixedUpdate`: steps each animated light and writes its effective `Light`,
/// only a different value (a write is a soft reset of the denoiser).
pub fn animate_lights(time: Res<Time>, mut lights: Query<(&mut LightAnimation, &mut Light)>) {
    let dt = time.fixed_delta.as_secs_f64();
    for (mut animation, mut light) in &mut lights {
        animation.step(dt);
        light.set_if_neq(animation.effective(dt));
    }
}

/// Writes the light of the file `def` into `entity`: the base of its animation, or the
/// `Light` itself. A light that gets an animation field gets a [`LightAnimation`].
pub(crate) fn write_light(world: &mut World, entity: Entity, def: &LightDef) -> Result<(), String> {
    let dt = world
        .get_resource::<Time>()
        .map_or(0.0, |time| time.fixed_delta.as_secs_f64());
    let base = def.light();
    let effective = match world.get_mut::<LightAnimation>(entity) {
        Some(mut animation) => {
            animation.base = base;
            animation.def = def.animation;
            animation.effective(dt)
        }
        None if def.animation.is_animated() => {
            let seed = world.get::<ObjectId>(entity).map_or(0, |id| id.0);
            let animation = LightAnimation::new(base, def.animation, seed);
            let effective = animation.effective(dt);
            world.entity_mut(entity).insert(animation);
            effective
        }
        None => base,
    };
    world
        .get_mut::<Light>(entity)
        .ok_or("the entity has no light")?
        .set_if_neq(effective);
    Ok(())
}
