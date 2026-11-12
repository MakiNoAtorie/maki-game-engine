//! The walk grid of the room and the path search on it.
//!
//! The grid has square cells of `RoomDef::cell` meters on the floor, with a cell edge
//! at the corner (-width/2, -depth/2) of the room. After each room load a fixed step
//! makes it from the fixed colliders of the physics: a cell is blocked when a box a
//! little smaller than the cell, of the height of a walker, overlaps one. The
//! kinematic colliders of the room (doors, movers) move, so they are not in the made
//! grid: [`WalkGrid::is_free`] tests them with an overlap when it reads a cell.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy_ecs::entity::Entity;
use bevy_ecs::query::{Or, With, Without};
use bevy_ecs::resource::Resource;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::schedule::common_conditions::resource_changed;
use bevy_ecs::system::{Query, Res, ResMut};
use glam::{IVec2, Quat, Vec3};
use maki_core::{App, FixedUpdate, Module, Settings};
use maki_physics::{Body, ObjectCollider, Physics, PhysicsSet, QueryFilter, Shape};
use maki_scene::{Collider, CurrentRoom, Mover};
use serde::{Deserialize, Serialize};

use crate::player::Walker;

/// The `[grid]` table of the config file.
#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GridSettings {
    /// True: a path goes to the 8 neighbours of a cell (never across the corner of a
    /// blocked cell); false: to the 4 along the axes.
    pub diagonal: bool,
    /// The height of the box that tests a cell, in meters: a walker's height.
    pub height: f32,
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            diagonal: false,
            height: 1.7,
        }
    }
}

impl Settings for GridSettings {
    const TABLE: &'static str = "grid";

    fn check(&self) -> Result<(), String> {
        if !(self.height.is_finite() && self.height > LIFT) {
            return Err(format!("height must be more than {LIFT}"));
        }
        Ok(())
    }
}

/// Makes the [`WalkGrid`] after each room load.
pub struct GridModule;

impl Module for GridModule {
    fn build(&self, app: &mut App) {
        app.add_settings::<GridSettings>()
            .init_resource::<WalkGrid>()
            // After the step: the physics has the colliders of the new room then.
            .add_systems(
                FixedUpdate,
                make_grid
                    .after(PhysicsSet::Step)
                    .run_if(resource_changed::<CurrentRoom>),
            );
    }
}

/// The test box is this much smaller than the cell on each side, as a part of the
/// cell: a collider that only touches the edge of a cell (a wall of the room) does not
/// block it.
const MARGIN: f32 = 0.05;

/// The test box starts this high above the floor, in meters: a flat thing on the floor
/// (a rug) does not block a cell.
const LIFT: f32 = 0.05;

/// The cost of a step along an axis and of a diagonal step (10 times the length, as
/// whole numbers: the same sums in each run).
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;

/// The cells of the room that a walker can stand in.
#[derive(Resource, Debug, Clone, Default)]
pub struct WalkGrid {
    /// The corner of cell (0, 0), on the floor.
    corner: Vec3,
    cell: f32,
    size: IVec2,
    /// Blocked by a fixed collider, by `index`.
    blocked: Vec<bool>,
    /// The kinematic entities of the room (doors, movers), sorted.
    moving: Vec<Entity>,
    /// Half the test box.
    half: Vec3,
    diagonal: bool,
}

impl WalkGrid {
    /// The grid of a room of `width` x `depth` meters with cells of `cell` meters:
    /// a cell is blocked when its box overlaps a collider of `fixed`. `moving`: the
    /// colliders that [`Self::is_free`] tests at each read.
    pub fn new(
        width: f32,
        depth: f32,
        cell: f32,
        settings: &GridSettings,
        physics: &Physics,
        fixed: &[Entity],
        mut moving: Vec<Entity>,
    ) -> Self {
        moving.sort();
        let mut fixed = fixed.to_vec();
        fixed.sort();
        let cell = cell.max(0.01);
        let size = IVec2::new((width / cell).ceil() as i32, (depth / cell).ceil() as i32);
        let half_side = cell * (0.5 - MARGIN);
        let mut grid = Self {
            corner: Vec3::new(-width / 2.0, 0.0, -depth / 2.0),
            cell,
            size,
            blocked: Vec::new(),
            moving,
            half: Vec3::new(half_side, (settings.height - LIFT) / 2.0, half_side),
            diagonal: settings.diagonal,
        };
        let cells =
            (0..size.y).flat_map(|row| (0..size.x).map(move |column| IVec2::new(column, row)));
        grid.blocked = cells
            .map(|cell| {
                grid.overlap(physics, cell)
                    .iter()
                    .any(|entity| fixed.binary_search(entity).is_ok())
            })
            .collect();
        grid
    }

    /// The side of a cell, in meters.
    pub fn cell(&self) -> f32 {
        self.cell
    }

    /// Columns (along X) and rows (along Z). Zero before the first room.
    pub fn size(&self) -> IVec2 {
        self.size
    }

    /// The cell of a point; `None` outside the room.
    pub fn cell_of(&self, point: Vec3) -> Option<IVec2> {
        let local = (point - self.corner) / self.cell;
        let cell = IVec2::new(local.x.floor() as i32, local.z.floor() as i32);
        self.inside(cell).then_some(cell)
    }

    /// The center of a cell, on the floor.
    pub fn center(&self, cell: IVec2) -> Vec3 {
        self.corner + Vec3::new(cell.x as f32 + 0.5, 0.0, cell.y as f32 + 0.5) * self.cell
    }

    /// True if the cell is in the room, no fixed collider blocks it, and no door or
    /// mover is in it now.
    pub fn is_free(&self, physics: &Physics, cell: IVec2) -> bool {
        self.inside(cell)
            && !self.blocked[self.index(cell)]
            && (self.moving.is_empty()
                || !self
                    .overlap(physics, cell)
                    .iter()
                    .any(|entity| self.moving.binary_search(entity).is_ok()))
    }

    /// The path from `from` to `to`: the centers of the cells after the cell of
    /// `from`, the last point `to` itself. `None` if a point is outside the room, the
    /// cell of `to` is not free, or no path goes there. The cell of `from` may be
    /// blocked: a walker that stands at a wall still finds its way out.
    pub fn find_path(&self, physics: &Physics, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        let (start, goal) = (self.cell_of(from)?, self.cell_of(to)?);
        // Each cell read once: a test of the kinematic colliders is a query.
        let mut known: Vec<Option<bool>> = vec![None; self.blocked.len()];
        let cells = find_cells(self.size, start, goal, self.diagonal, |cell| {
            *known[self.index(cell)].get_or_insert_with(|| self.is_free(physics, cell))
        })?;
        let mut points: Vec<Vec3> = cells[1..].iter().map(|&cell| self.center(cell)).collect();
        points.pop();
        points.push(to);
        Some(points)
    }

    fn inside(&self, cell: IVec2) -> bool {
        cell.cmpge(IVec2::ZERO).all() && cell.cmplt(self.size).all()
    }

    fn index(&self, cell: IVec2) -> usize {
        (cell.y * self.size.x + cell.x) as usize
    }

    /// The entities whose colliders overlap the test box of a cell (no sensors).
    fn overlap(&self, physics: &Physics, cell: IVec2) -> Vec<Entity> {
        let center = self.center(cell) + Vec3::Y * (LIFT + self.half.y);
        let shape = Shape::Box { half: self.half };
        physics.overlap(shape, center, Quat::IDENTITY, QueryFilter::default())
    }
}

/// A* on a grid of `size` cells, from `start` to `goal`. `free` tells if a cell can be
/// walked; it is not asked for `start`. With `diagonal`, a step goes to the 8
/// neighbours, a diagonal one only when the two cells beside it are free (no corner
/// cut). The path holds `start` and `goal`; `None` if no path. Ties go the same way in
/// each run: by cost, then by the distance left, then by the index of the cell.
pub fn find_cells(
    size: IVec2,
    start: IVec2,
    goal: IVec2,
    diagonal: bool,
    mut free: impl FnMut(IVec2) -> bool,
) -> Option<Vec<IVec2>> {
    let inside = |cell: IVec2| cell.cmpge(IVec2::ZERO).all() && cell.cmplt(size).all();
    if !inside(start) || !inside(goal) || (start != goal && !free(goal)) {
        return None;
    }
    let index = |cell: IVec2| (cell.y * size.x + cell.x) as usize;
    let cell_at = |index: usize| IVec2::new(index as i32 % size.x, index as i32 / size.x);
    let left = |cell: IVec2| {
        let d = (goal - cell).abs();
        if diagonal {
            STRAIGHT * d.max_element() as u32 + (DIAGONAL - STRAIGHT) * d.min_element() as u32
        } else {
            STRAIGHT * (d.x + d.y) as u32
        }
    };

    let count = (size.x * size.y) as usize;
    let mut cost = vec![u32::MAX; count];
    let mut came = vec![usize::MAX; count];
    let mut done = vec![false; count];
    let mut open = BinaryHeap::new();
    cost[index(start)] = 0;
    open.push(Reverse((left(start), left(start), index(start))));

    const STEPS: [(IVec2, u32); 8] = [
        (IVec2::new(1, 0), STRAIGHT),
        (IVec2::new(-1, 0), STRAIGHT),
        (IVec2::new(0, 1), STRAIGHT),
        (IVec2::new(0, -1), STRAIGHT),
        (IVec2::new(1, 1), DIAGONAL),
        (IVec2::new(1, -1), DIAGONAL),
        (IVec2::new(-1, 1), DIAGONAL),
        (IVec2::new(-1, -1), DIAGONAL),
    ];
    let steps = if diagonal { &STEPS[..] } else { &STEPS[..4] };

    while let Some(Reverse((_, _, at))) = open.pop() {
        if done[at] {
            continue;
        }
        done[at] = true;
        let cell = cell_at(at);
        if cell == goal {
            let mut path = vec![goal];
            let mut back = at;
            while back != index(start) {
                back = came[back];
                path.push(cell_at(back));
            }
            path.reverse();
            return Some(path);
        }
        for &(step, step_cost) in steps {
            let next = cell + step;
            if !inside(next) || done[index(next)] || !free(next) {
                continue;
            }
            let corner_cut = step.x != 0
                && step.y != 0
                && !(free(cell + IVec2::new(step.x, 0)) && free(cell + IVec2::new(0, step.y)));
            let new_cost = cost[at] + step_cost;
            if corner_cut || new_cost >= cost[index(next)] {
                continue;
            }
            cost[index(next)] = new_cost;
            came[index(next)] = at;
            open.push(Reverse((new_cost + left(next), left(next), index(next))));
        }
    }
    None
}

/// The colliders of the room that do not move: the boxes of the room and the fixed
/// object colliders. Not sensors, not the walkers.
type FixedQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static ObjectCollider>,
        Option<&'static Body>,
    ),
    (
        Or<(With<Collider>, With<ObjectCollider>)>,
        Without<Walker>,
        Without<Mover>,
    ),
>;

/// The doors, movers, and kinematic object bodies; not the walkers.
type MovingQuery<'w, 's> = Query<
    'w,
    's,
    (Entity, Option<&'static Body>),
    (Or<(With<Mover>, With<Body>)>, Without<Walker>),
>;

/// Runs in `FixedUpdate` after a room load: makes the [`WalkGrid`] of the room.
fn make_grid(
    room: Res<CurrentRoom>,
    settings: Res<GridSettings>,
    physics: Res<Physics>,
    colliders: FixedQuery,
    movers: MovingQuery,
    mut grid: ResMut<WalkGrid>,
) {
    let fixed: Vec<Entity> = colliders
        .iter()
        .filter(|(_, collider, body)| {
            let solid = collider.is_none_or(|collider| !collider.sensor);
            solid && matches!(body, None | Some(Body::Fixed))
        })
        .map(|(entity, ..)| entity)
        .collect();
    let moving = movers
        .iter()
        .filter(|(_, body)| matches!(body, None | Some(Body::Kinematic)))
        .map(|(entity, _)| entity)
        .collect();
    let def = &room.def;
    *grid = WalkGrid::new(
        def.width, def.depth, def.cell, &settings, &physics, &fixed, moving,
    );
}
