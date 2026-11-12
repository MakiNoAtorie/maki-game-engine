//! Stage 61, ticket A: the walk grid of a room and the path search.

use std::time::{Duration, Instant};

use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res};
use glam::{IVec2, Vec3};
use maki_core::{App, ConfigFile, Update};
use maki_physics::{Physics, PhysicsModule};
use maki_scene::{Mover, MoverState, RoomRequest, RoomTarget, SceneModule, room_text};

use crate::{GridModule, WalkGrid, find_cells};

/// A room of 8 x 8 m in cells of 1 m (cell (0, 0) at the corner (-4, -4)): a wall in
/// column 4 from row 0 to row 6, a gate (a mover) in row 7 that lifts out of the way,
/// and a crate in cell (1, 6).
const WALLED: [&str; 3] = [
    r#"(name: "wall", pose: (position: (0.5, 0, -0.5)), components: {"collider": (shape: Box(half: (0.5, 1.0, 3.5)))})"#,
    r#"(name: "gate", pose: (position: (0.5, 0.5, 3.5)), components: {"mover": (mesh: Cube(1.0), b: (position: (0.5, 2.5, 3.5)), seconds: 0.1)})"#,
    r#"(name: "crate", pose: (position: (-2.5, 0.5, 2.5)), components: {"prop": (mesh: Cube(1.0))})"#,
];

fn room_app(width: f32, depth: f32, objects: &[&str], settings: &str) -> App {
    let config = format!("[time]\nfixed_hz = 50.0\n[grid]\n{settings}");
    let mut app = App::new(ConfigFile::from_text(&config)).unwrap();
    app.add_module(SceneModule)
        .add_module(PhysicsModule)
        .add_module(GridModule)
        .init_resource::<OpenGates>()
        .add_systems(Update, open_gates);
    app.resource_mut::<RoomRequest>().0 = Some(RoomTarget {
        path: "l61a-grid.ron".into(),
        spawn: None,
        text: Some(room_text(width, depth, 3.0, objects)),
    });
    steps(&mut app, 3);
    app
}

fn steps(app: &mut App, count: usize) {
    for _ in 0..count {
        app.advance(Duration::from_millis(20));
    }
}

/// True: the movers of the room go to B.
#[derive(Resource, Default)]
struct OpenGates(bool);

fn open_gates(open: Res<OpenGates>, mut movers: Query<&mut Mover>) {
    for mut mover in &mut movers {
        if open.0 && mover.state == MoverState::AtA {
            mover.go_to_b();
        }
    }
}

fn open_gate(app: &mut App) {
    app.resource_mut::<OpenGates>().0 = true;
    steps(app, 10);
}

fn grid_of(app: &App) -> (&WalkGrid, &Physics) {
    let world = app.world();
    (world.resource::<WalkGrid>(), world.resource::<Physics>())
}

fn cell(x: i32, y: i32) -> IVec2 {
    IVec2::new(x, y)
}

#[test]
fn the_walk_grid_has_the_wall_the_prop_and_the_gate() {
    let app = room_app(8.0, 8.0, &WALLED, "");
    let (grid, physics) = grid_of(&app);
    assert_eq!((grid.size(), grid.cell()), (cell(8, 8), 1.0));
    assert_eq!(grid.cell_of(Vec3::new(-3.9, 0.0, -3.9)), Some(cell(0, 0)));
    assert_eq!(grid.cell_of(Vec3::new(4.1, 0.0, 0.0)), None);
    assert_eq!(grid.center(cell(4, 7)), Vec3::new(0.5, 0.0, 3.5));

    let free = |x, y| grid.is_free(physics, cell(x, y));
    assert!((0..7).all(|row| !free(4, row)), "the wall");
    assert!(!free(4, 7), "the closed gate");
    assert!(!free(1, 6), "the crate");
    // The cells beside them, also those at the walls of the room, are free.
    for (x, y) in [
        (3, 0),
        (5, 0),
        (3, 7),
        (5, 7),
        (0, 6),
        (2, 6),
        (1, 5),
        (1, 7),
        (0, 0),
        (7, 7),
    ] {
        assert!(free(x, y), "({x}, {y})");
    }
}

#[test]
fn a_gate_that_opens_frees_its_cell_for_a_path() {
    let mut app = room_app(8.0, 8.0, &WALLED, "");
    let (from, to) = (Vec3::new(-2.5, 0.0, -2.5), Vec3::new(2.7, 0.0, -2.2));
    let (grid, physics) = grid_of(&app);
    assert_eq!(grid.find_path(physics, from, to), None, "a closed gate");

    open_gate(&mut app);
    let (grid, physics) = grid_of(&app);
    assert!(grid.is_free(physics, cell(4, 7)));
    let path = grid.find_path(physics, from, to).expect("through the gate");
    assert!(path.contains(&grid.center(cell(4, 7))));
    assert_eq!(path.last(), Some(&to), "the last point is the place itself");
    assert_ne!(path[0], grid.center(cell(1, 1)), "not the start cell");
    // 4 neighbours: from (1, 1) to (4, 7) and on to (6, 1): 3 + 6 + 2 + 6 steps.
    assert_eq!(path.len(), 17);
}

#[test]
fn a_path_in_the_same_cell_is_the_place() {
    let app = room_app(8.0, 8.0, &[], "");
    let (grid, physics) = grid_of(&app);
    let to = Vec3::new(-3.2, 0.0, -3.7);
    assert_eq!(
        grid.find_path(physics, Vec3::new(-3.5, 0.0, -3.5), to),
        Some(vec![to])
    );
}

#[test]
fn a_star_takes_the_shortest_way_around_a_wall() {
    // A wall in column 2 from row 0 to row 3 of 5 x 5 cells.
    let free = |c: IVec2| !(c.x == 2 && c.y <= 3);
    let path = find_cells(cell(5, 5), cell(0, 0), cell(4, 0), false, free).unwrap();
    assert_eq!((path[0], path[path.len() - 1]), (cell(0, 0), cell(4, 0)));
    assert_eq!(path.len(), 13, "around the end of the wall: 6 + 6 steps");
    assert!(path.contains(&cell(2, 4)));
    assert!(
        path.windows(2)
            .all(|pair| (pair[1] - pair[0]).abs().element_sum() == 1)
    );
    assert!(path.iter().all(|&c| free(c)));
}

#[test]
fn a_star_finds_no_path_into_a_closed_room() {
    // Cell (3, 3) of 6 x 6 with blocked cells on all 8 sides.
    let free = |c: IVec2| (c - cell(3, 3)).abs().max_element() != 1;
    for diagonal in [false, true] {
        assert_eq!(
            find_cells(cell(6, 6), cell(0, 0), cell(3, 3), diagonal, free),
            None
        );
    }
    // A blocked goal, and a goal outside the grid.
    assert_eq!(
        find_cells(cell(6, 6), cell(0, 0), cell(2, 2), false, free),
        None
    );
    assert_eq!(
        find_cells(cell(6, 6), cell(0, 0), cell(6, 0), false, |_| true),
        None
    );
}

#[test]
fn a_star_with_8_neighbours_cuts_no_corner() {
    let open = find_cells(cell(3, 3), cell(0, 0), cell(1, 1), true, |_| true);
    assert_eq!(open, Some(vec![cell(0, 0), cell(1, 1)]));
    // Cell (1, 0) blocked: the diagonal step would cut its corner.
    let free = |c: IVec2| c != cell(1, 0);
    let path = find_cells(cell(3, 3), cell(0, 0), cell(1, 1), true, free);
    assert_eq!(path, Some(vec![cell(0, 0), cell(0, 1), cell(1, 1)]));
    // On the room grid too, from the setting.
    let app = room_app(8.0, 8.0, &WALLED, "diagonal = true");
    let (grid, physics) = grid_of(&app);
    // Each diagonal step from (0, 5) to (2, 7) passes a corner of the crate in (1, 6).
    let path = grid.find_path(physics, grid.center(cell(0, 5)), grid.center(cell(2, 7)));
    let mut cells = vec![cell(0, 5)];
    cells.extend(path.unwrap().iter().map(|&p| grid.cell_of(p).unwrap()));
    assert_eq!(cells.len(), 5, "{cells:?}");
    assert!(
        cells
            .windows(2)
            .all(|pair| (pair[1] - pair[0]).abs().element_sum() == 1)
    );
}

#[test]
fn a_star_gives_the_same_path_in_each_run() {
    // An open grid has many shortest paths: the tie order picks one, each time.
    let first = find_cells(cell(10, 10), cell(0, 0), cell(9, 9), false, |_| true).unwrap();
    for _ in 0..5 {
        let again = find_cells(cell(10, 10), cell(0, 0), cell(9, 9), false, |_| true);
        assert_eq!(again.as_ref(), Some(&first));
    }
    assert_eq!(first.len(), 19);
    let a = room_app(8.0, 8.0, &WALLED, "diagonal = true");
    let b = room_app(8.0, 8.0, &WALLED, "diagonal = true");
    let (from, to) = (Vec3::new(-3.5, 0.0, -3.5), Vec3::new(3.5, 0.0, 3.5));
    let path = |app: &App| grid_of(app).0.find_path(grid_of(app).1, from, to);
    assert_eq!(path(&a), path(&b));
}

/// Watch point 2: one search on 100 x 100 cells, with a gate so that each cell read is
/// also a query of the kinematic colliders. The time is printed, not checked.
#[test]
fn a_search_on_100_by_100_cells_is_timed() {
    // A wall across the room with an opening at the far end of +Z.
    let objects = [
        r#"(name: "wall", pose: (position: (0.5, 0, -1)), components: {"collider": (shape: Box(half: (0.5, 1.0, 49.0)))})"#,
        r#"(name: "gate", pose: (position: (-45.5, 0.5, -45.5)), components: {"mover": (mesh: Cube(1.0), b: (position: (-45.5, 2.5, -45.5)), seconds: 0.1)})"#,
    ];
    let app = room_app(100.0, 100.0, &objects, "");
    let (grid, physics) = grid_of(&app);
    assert_eq!(grid.size(), cell(100, 100));
    let (from, to) = (Vec3::new(-49.5, 0.0, -49.5), Vec3::new(49.5, 0.0, -49.5));
    let start = Instant::now();
    let path = grid.find_path(physics, from, to).expect("around the wall");
    let time = start.elapsed();
    println!("l61a,search_100x100_us,{}", time.as_micros());
    // 99 columns, and 98 rows up to the opening and back: one point per step.
    assert_eq!(path.len(), 99 + 2 * 98);
}
