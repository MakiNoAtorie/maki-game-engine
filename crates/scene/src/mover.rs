//! Movers: entities that go between two poses. A door is a mover with a collider.

use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::query::Has;
use bevy_ecs::system::{Commands, Query, Res};
use bevy_ecs::world::Ref;
use maki_core::{Interpolated, Time, Transform};

use crate::Collider;

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

/// A door: a [`Mover`] that is closed at A and open at B. While it is not fully open,
/// the entity has the `blocker` as its [`Collider`].
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Door {
    /// The space that the closed door fills.
    pub blocker: Collider,
}

/// Gives each door its collider, and takes it away while the door is open.
// ponytail: an open door blocks nothing: the player walks through the door leaf. Add
// a second box for the open leaf when that shows.
pub(crate) fn block_doors(
    mut commands: Commands,
    doors: Query<(Entity, &Door, Ref<Mover>, Has<Collider>)>,
) {
    for (entity, door, mover, blocks) in &doors {
        if !mover.is_changed() {
            continue;
        }
        let open = mover.state == MoverState::AtB;
        if open && blocks {
            commands.entity(entity).remove::<Collider>();
        } else if !open && !blocks {
            commands.entity(entity).insert(door.blocker);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use glam::Vec3;
    use maki_core::{App, ConfigFile, Startup};

    use super::*;
    use crate::SceneModule;

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

    #[test]
    fn a_door_blocks_until_it_is_fully_open() {
        use bevy_ecs::schedule::IntoScheduleConfigs;
        use maki_core::FixedUpdate;

        // 50 Hz: one fixed step per 20 ms. The door takes 2 steps.
        let mut app = App::new(ConfigFile::from_text("[time]\nfixed_hz = 50.0")).unwrap();
        let blocker = Collider::new(Vec3::ZERO, Vec3::ONE);
        app.add_module(SceneModule)
            .add_systems(Startup, move |mut commands: Commands| {
                commands.spawn((
                    Door { blocker },
                    Mover::new(Transform::IDENTITY, Transform::IDENTITY, 0.04),
                    Interpolated::at(Transform::IDENTITY),
                ));
            })
            // Opens the door in step 2 and closes it in step 4.
            .add_systems(
                FixedUpdate,
                (|time: Res<Time>, mut movers: Query<&mut Mover>| match time.fixed_tick {
                    2 => movers.single_mut().unwrap().go_to_b(),
                    4 => movers.single_mut().unwrap().go_to_a(),
                    _ => {}
                })
                .before(move_movers),
            );
        app.run(|mut app| {
            let mut colliders_after_step = || {
                app.advance(Duration::from_millis(20));
                let mut colliders = app.world().try_query::<&Collider>().unwrap();
                colliders.iter(app.world()).count()
            };
            // Closed: the door gets its collider in the first step.
            assert_eq!(colliders_after_step(), 1);
            // On its way.
            assert_eq!(colliders_after_step(), 1);
            // Open.
            assert_eq!(colliders_after_step(), 0);
            // It blocks again from the first step of the way back.
            assert_eq!(colliders_after_step(), 1);
        });
    }
}
