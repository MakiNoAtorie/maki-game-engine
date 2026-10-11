use bevy_ecs::component::Component;
use glam::{Mat4, Quat, Vec3};

/// Position, rotation, and scale of an entity in the world.
///
/// Coordinates are right-handed with +Y up, as in glTF. The unit is the meter.
// ponytail: no parent/child hierarchy. Add one when an object must follow another.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    /// The pose at the part `t` (0.0 to 1.0) of the way from `self` to `other`. The
    /// rotation takes the shortest arc.
    pub fn blend(&self, other: &Self, t: f32) -> Self {
        // The blend of two equal numbers can differ from them in the last bit, with
        // another result for each `t`. An entity at rest must not move by that bit:
        // each move tells the renderer that the picture changed.
        if self == other {
            return *self;
        }
        Self {
            translation: self.translation.lerp(other.translation, t),
            rotation: self.rotation.slerp(other.rotation, t),
            scale: self.scale.lerp(other.scale, t),
        }
    }

    /// The matrix from object space to world space. Order: scale, then rotation, then translation.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}
