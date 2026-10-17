//! Movers: entities that go between two poses. A door is a mover whose leaf turns.

use bevy_ecs::component::Component;
use bevy_ecs::system::{Query, Res};
use maki_core::{Interpolated, Time, Transform};

/// Where a mover is. The number of a moving state is the part of the way from A to B
/// that is done: 0.0 at A, 1.0 at B.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MoverState {
    AtA,
    ToB(f32),
    AtB,
    ToA(f32),
}

/// Moves the entity between the poses `a` and `b`. The entity needs an `Interpolated`
/// component: the mover writes the pose there. Other code only sets the state
/// ([`Mover::go_to_a`], [`Mover::go_to_b`], [`Mover::toggle`]).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Mover {
    pub a: Transform,
    pub b: Transform,
    /// Seconds for the full way.
    pub duration: f32,
    pub state: MoverState,
}

impl Mover {
    /// At rest at `a`.
    pub fn new(a: Transform, b: Transform, duration: f32) -> Self {
        Self {
            a,
            b,
            duration,
            state: MoverState::AtA,
        }
    }

    /// Starts the way to A from where the mover is. Does nothing at A.
    pub fn go_to_a(&mut self) {
        self.state = match self.state {
            MoverState::AtB => MoverState::ToA(1.0),
            MoverState::ToB(part) => MoverState::ToA(part),
            state => state,
        };
    }

    /// Starts the way to B from where the mover is. Does nothing at B.
    pub fn go_to_b(&mut self) {
        self.state = match self.state {
            MoverState::AtA => MoverState::ToB(0.0),
            MoverState::ToA(part) => MoverState::ToB(part),
            state => state,
        };
    }

    /// Turns around: to B if the mover is at A or on its way there, else to A.
    pub fn toggle(&mut self) {
        match self.state {
            MoverState::AtA | MoverState::ToA(_) => self.go_to_b(),
            MoverState::AtB | MoverState::ToB(_) => self.go_to_a(),
        }
    }

    pub fn is_moving(&self) -> bool {
        matches!(self.state, MoverState::ToA(_) | MoverState::ToB(_))
    }

    /// One fixed step of `seconds`. Returns the new pose, or `None` at rest.
    fn step(&mut self, seconds: f32) -> Option<Transform> {
        // A duration of zero (or a bad number) gives a jump to the end.
        let part = if self.duration > 0.0 {
            seconds / self.duration
        } else {
            1.0
        };
        let (state, at) = match self.state {
            MoverState::ToB(done) if done + part >= 1.0 => (MoverState::AtB, 1.0),
            MoverState::ToB(done) => (MoverState::ToB(done + part), done + part),
            MoverState::ToA(done) if done - part <= 0.0 => (MoverState::AtA, 0.0),
            MoverState::ToA(done) => (MoverState::ToA(done - part), done - part),
            MoverState::AtA | MoverState::AtB => return None,
        };
        self.state = state;
        // Slow at the two ends ("smoothstep"), so a door does not start or stop at once.
        let eased = at * at * (3.0 - 2.0 * at);
        Some(self.a.blend(&self.b, eased))
    }
}

/// Runs in `FixedUpdate`. A system that sets mover states in `FixedUpdate` runs
/// `.before(move_movers)`.
pub fn move_movers(time: Res<Time>, mut movers: Query<(&mut Mover, &mut Interpolated)>) {
    let seconds = time.fixed_delta.as_secs_f32();
    for (mut mover, mut pose) in &mut movers {
        // No write at rest: a write would mark the mover as changed in each step.
        if mover.is_moving()
            && let Some(next) = mover.step(seconds)
        {
            pose.current = next;
        }
    }
}

/// A door: a [`Mover`] that is closed at A and open at B. The physics gives its leaf a
/// kinematic collider (the box of its mesh), which blocks at each pose.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Door;

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::*;

    #[test]
    fn a_mover_goes_to_b_and_back_and_turns_around_on_the_way() {
        let b = Transform::from_translation(Vec3::X * 4.0);
        let mut mover = Mover::new(Transform::IDENTITY, b, 2.0);
        assert_eq!(mover.step(0.5), None);
        mover.toggle();
        // A quarter of the way in time is less in distance: the start is slow.
        let first = mover.step(0.5).unwrap().translation.x;
        assert!(first > 0.0 && first < 1.0, "{first}");
        assert_eq!(mover.step(0.5).unwrap().translation.x, 2.0);
        // Turn around in the middle: back at A after the time that the way out took.
        mover.toggle();
        assert_eq!(mover.state, MoverState::ToA(0.5));
        assert_eq!(mover.step(0.5).unwrap().translation.x, first);
        assert_eq!(mover.step(0.5).unwrap().translation.x, 0.0);
        assert_eq!(mover.state, MoverState::AtA);
        // The full way, with a last step that is too long.
        mover.go_to_b();
        mover.go_to_b();
        mover.step(1.9);
        assert_eq!(mover.step(0.5), Some(b));
        assert_eq!(mover.state, MoverState::AtB);
        assert!(!mover.is_moving());
    }
}
