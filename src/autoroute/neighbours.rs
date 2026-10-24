//! Calculation of the doors and new incomplete rooms of a room
//! (port of `SortedRoomNeighbours`, `Sorted45DegreeRoomNeighbours` and
//! `SortedOrthogonalRoomNeighbours`).

use super::complete_shape::complete_shape;
use super::engine::{AutorouteEngine, IncompleteRoom, RoomKey, RoomKind};
use crate::board::{Board, TreeEntry, TreeKind, TreeObject};
use crate::geometry::basics::CRIT_INT;
use crate::geometry::{IntBox, IntDirection, IntOctagon, IntPoint, Line, Point, Side, Signum, Simplex, TileShape};
use std::cmp::Ordering;

/// `SortedRoomNeighbours.complete(room, engine)`
pub fn complete(eng: &mut AutorouteEngine, board: &Board, room: RoomKey) -> Option<RoomKey> {
    if crate::board::snapshot::evlog::maze_log_enabled() {
        let r = eng.room(room);
        let bb = r.shape().bounding_box();
        let k = match r.kind {
            RoomKind::Obstacle { item, index, .. } => format!("O{}.{}", item.0, index),
            RoomKind::Incomplete { .. } => "I".to_string(),
            RoomKind::Complete { .. } => "C".to_string(),
        };
        crate::board::snapshot::evlog::line(&format!("CALC {} L{} {},{},{},{}", k, r.layer, bb.ll.x, bb.ll.y, bb.ur.x, bb.ur.y));
    }
    match eng.tree_kind(board) {
        TreeKind::Degree90 => orthogonal::calculate(eng, board, room),
        TreeKind::Degree45 => deg45::calculate(eng, board, room),
        TreeKind::Default => any::calculate(eng, board, room),
    }
}

/// Inserts into a sorted vector with `TreeSet.add` semantics (equal elements are not added).
fn sorted_insert<T>(v: &mut Vec<T>, item: T, cmp: impl Fn(&T, &T) -> Ordering) -> bool {
    match v.binary_search_by(|probe| cmp(probe, &item)) {
        Ok(_) => false,
        Err(p) => {
            v.insert(p, item);
            true
        }
    }
}

/// `SearchTreeObject.getId()`
fn object_id(o: &TreeObject) -> i32 {
    match o {
        TreeObject::Item(id) => id.0,
        TreeObject::Room(_, id) => *id,
    }
}

/// The overlapping tree entries of a room, sorted by object id and shape index.
fn sorted_overlaps(eng: &AutorouteEngine, board: &Board, shape: &TileShape, layer: i32) -> Vec<TreeEntry> {
    let mut v = Vec::new();
    board
        .search_trees
        .get(eng.tree)
        .overlapping_tree_entries(shape, layer, &[], &board.lookup(), &mut v);
    v.sort_by(|a, b| {
        object_id(&a.object)
            .wrapping_sub(object_id(&b.object))
            .cmp(&0)
            .then((a.shape_index as i32 - b.shape_index as i32).cmp(&0))
    });
    v
}

fn tree_shape(eng: &AutorouteEngine, board: &Board, e: &TreeEntry) -> TileShape {
    board
        .search_trees
        .get(eng.tree)
        .object_shape(&e.object, e.shape_index)
        .cloned()
        .unwrap_or_else(TileShape::empty)
}

fn is_trace_obstacle(eng: &AutorouteEngine, board: &Board, o: &TreeObject) -> bool {
    board
        .search_trees
        .get(eng.tree)
        .object_is_trace_obstacle(o, eng.net_no, &board.lookup())
}

/// Creates the completed room for an incomplete room, or returns the obstacle room itself.
fn completed_room_for(eng: &mut AutorouteEngine, room: RoomKey, room_id_no: i32) -> Option<RoomKey> {
    let r = eng.room(room);
    match r.kind {
        RoomKind::Incomplete { .. } => {
            let shape = r.shape().clone();
            let layer = r.layer;
            Some(eng.new_complete_room(shape, layer, room_id_no))
        }
        RoomKind::Obstacle { .. } => Some(room),
        _ => {
            log::warn!("SortedRoomNeighbours.calculate: unexpected expansion room type");
            None
        }
    }
}

/// The neighbour room of a tree entry: a complete room or the obstacle room of a routable item.
fn neighbour_room(eng: &mut AutorouteEngine, board: &Board, e: &TreeEntry) -> Option<RoomKey> {
    match e.object {
        TreeObject::Room(r, _) => Some(RoomKey(r.0)),
        TreeObject::Item(id) => {
            if board.item(id).is_routable() {
                eng.get_obstacle_room(board, id, e.shape_index)
            } else {
                None
            }
        }
    }
}

/// `CompleteFreeSpaceExpansionRoom.calculateTargetDoors(entry, netNumber, tree)`
fn calculate_target_doors(eng: &mut AutorouteEngine, board: &Board, room: RoomKey, e: &TreeEntry) {
    if let RoomKind::Complete { net_dependent, .. } = &mut eng.room_mut(room).kind {
        *net_dependent = true;
    }
    add_target_door_if_connected(eng, board, room, e);
}

fn add_target_door_if_connected(eng: &mut AutorouteEngine, board: &Board, room: RoomKey, e: &TreeEntry) {
    let TreeObject::Item(id) = e.object else {
        return;
    };
    let item = board.item(id);
    if !item.is_connectable_type() || !item.contains_net(eng.net_no) {
        return;
    }
    let Some(conn) = board.trace_connection_shape(eng.tree, id, e.shape_index) else {
        return;
    };
    if eng.room(room).shape().intersects(&conn) {
        let t = eng.new_target_door(board, id, e.shape_index, Some(room));
        if let RoomKind::Complete { target_doors, .. } = &mut eng.room_mut(room).kind {
            target_doors.push(t);
        }
    }
}

/// `SortedRoomNeighbours.insertDoorOk(room1, room2, doorShape)`
pub fn insert_door_ok(eng: &AutorouteEngine, board: &Board, room1: RoomKey, room2: RoomKey, door_shape: &TileShape) -> bool {
    if eng.door_exists(room1, room2) {
        return false;
    }
    let r1 = eng.room(room1);
    let r2 = eng.room(room2);
    if let (Some(a), Some(b)) = (r1.obstacle_item(), r2.obstacle_item()) {
        return board.item(a).shares_net(board.item(b));
    }
    if !r1.is_obstacle() && !r2.is_obstacle() {
        return true;
    }
    let mut door_line: Option<Line> = None;
    let n = door_shape.border_line_count();
    if n > 0 {
        let mut prev = door_shape.corner(0);
        for i in 1..n {
            let c = door_shape.corner(i);
            if c != prev {
                door_line = Some(door_shape.border_line(i - 1));
                break;
            }
            prev = c;
        }
    }
    if r1.is_obstacle() && !insert_door_ok_obstacle(eng, board, room1, door_line.as_ref()) {
        return false;
    }
    if r2.is_obstacle() {
        return insert_door_ok_obstacle(eng, board, room2, door_line.as_ref());
    }
    true
}

fn insert_door_ok_obstacle(eng: &AutorouteEngine, board: &Board, room: RoomKey, door_line: Option<&Line>) -> bool {
    let Some(door_line) = door_line else {
        log::warn!("SortedRoomNeighbours.insert_door_ok: doorLine is null");
        return false;
    };
    let r = eng.room(room);
    let item = board.item(r.obstacle_item().unwrap());
    if let Some(t) = item.as_trace() {
        let idx = r.obstacle_index();
        if idx == 0 || idx == t.tile_shape_count() - 1 {
            let trace_line = t.polyline.line(idx + 1);
            return trace_line.is_parallel(door_line);
        }
    }
    true
}

fn connect_rooms(eng: &mut AutorouteEngine, completed: RoomKey, neighbour: RoomKey, dimension: Option<i32>) {
    let d = match dimension {
        Some(dim) => eng.new_door(completed, neighbour, dim),
        None => eng.new_door_auto(completed, neighbour),
    };
    eng.add_door(neighbour, d);
    eng.add_door(completed, d);
}

fn add_incomplete_with_door(eng: &mut AutorouteEngine, completed: RoomKey, shape: TileShape, layer: i32, contained: TileShape, dimension: i32) {
    let new_room = eng.add_incomplete_expansion_room(Some(shape), layer, Some(contained));
    let d = eng.new_door(completed, new_room, dimension);
    eng.add_door(completed, d);
    eng.add_door(new_room, d);
}

fn set_incomplete_shape(eng: &mut AutorouteEngine, room: RoomKey, new_room: IncompleteRoom) {
    let r = eng.room_mut(room);
    r.shape = new_room.shape;
    if let RoomKind::Incomplete { contained } = &mut r.kind {
        *contained = new_room.contained;
    }
}

// ==================================================================== orthogonal

mod orthogonal {
    use super::*;

    struct Neighbour {
        object: TreeObject,
        intersection: IntBox,
        first_touching_side: i32,
        last_touching_side: i32,
    }

    pub(super) fn calculate(eng: &mut AutorouteEngine, board: &Board, room: RoomKey) -> Option<RoomKey> {
        let id_no = eng.generate_room_id_no();
        let (completed, neighbours, edge_touch) = calculate_neighbours(eng, board, room, id_no)?;
        let edge_removed = try_remove_edge(eng, board, room, completed, &edge_touch);
        if edge_removed {
            eng.remove_all_doors(completed);
            return calculate(eng, board, room);
        }
        if neighbours.is_empty() {
            if eng.room(completed).is_obstacle() {
                calculate_incomplete_rooms_with_empty_neighbours(eng, board, completed);
            }
        } else {
            calculate_new_incomplete_rooms(eng, board, room, completed, &neighbours);
        }
        Some(completed)
    }

    fn calculate_incomplete_rooms_with_empty_neighbours(eng: &mut AutorouteEngine, board: &Board, room: RoomKey) {
        let TileShape::Box(room_box) = eng.room(room).shape().clone() else {
            log::warn!("SortedOrthoganelRoomNeighbours.calculate_incomplete_rooms_with_empty_neighbours: IntBox expected for roomShape");
            return;
        };
        let bb = board.bounding_box;
        let layer = eng.room(room).layer;
        for i in 0..4 {
            let new_box = match i {
                0 => IntBox::new(bb.ll.x, bb.ll.y, bb.ur.x, room_box.ll.y),
                1 => IntBox::new(room_box.ur.x, bb.ll.y, bb.ur.x, bb.ur.y),
                2 => IntBox::new(bb.ll.x, room_box.ur.y, bb.ur.x, bb.ur.y),
                _ => IntBox::new(bb.ll.x, bb.ll.y, room_box.ll.x, bb.ur.y),
            };
            let contained = room_box.intersection(&new_box);
            let new_room = eng.add_incomplete_expansion_room(Some(TileShape::Box(new_box)), layer, Some(TileShape::Box(contained)));
            let d = eng.new_door(room, new_room, 1);
            eng.add_door(room, d);
            eng.add_door(new_room, d);
        }
    }

    #[allow(clippy::type_complexity)]
    fn calculate_neighbours(
        eng: &mut AutorouteEngine,
        board: &Board,
        room: RoomKey,
        id_no: i32,
    ) -> Option<(RoomKey, Vec<Neighbour>, [bool; 4])> {
        let room_shape = eng.room(room).shape().clone();
        let TileShape::Box(room_box) = room_shape else {
            log::warn!("SortedOrthogonalRoomNeighbours.calculate: IntBox expected for roomShape");
            return None;
        };
        let layer = eng.room(room).layer;
        let completed = completed_room_for(eng, room, id_no)?;
        let mut edge = [false; 4];
        let mut neighbours: Vec<Neighbour> = Vec::new();
        let overlaps = sorted_overlaps(eng, board, &room_shape, layer);
        for e in &overlaps {
            if eng.room(completed).is_complete_free() && !is_trace_obstacle(eng, board, &e.object) {
                calculate_target_doors(eng, board, completed, e);
                continue;
            }
            let current_shape = tree_shape(eng, board, e);
            let TileShape::Box(current_box) = current_shape else {
                log::warn!("OrthogonalAutorouteEngine:calculate_sorted_neighbours: IntBox expected for currentShape");
                return None;
            };
            let intersection = room_box.intersection(&current_box);
            let dimension = intersection.dimension();
            if dimension > 1 && eng.room(completed).is_obstacle() {
                if let TreeObject::Item(id) = e.object {
                    if board.item(id).is_routable() {
                        if let Some(overlap_room) = eng.get_obstacle_room(board, id, e.shape_index) {
                            eng.create_overlap_door(board, completed, overlap_room);
                        }
                    }
                }
                continue;
            }
            if dimension < 0 {
                log::warn!("AutorouteEngine.calculate_doors: dimension >= 0 expected");
                continue;
            }
            let n = new_neighbour(e.object, intersection, &room_box, &mut edge);
            sorted_insert(&mut neighbours, n, compare);
            if dimension > 0 {
                if let Some(nr) = neighbour_room(eng, board, e) {
                    if insert_door_ok(eng, board, completed, nr, &TileShape::Box(intersection)) {
                        connect_rooms(eng, completed, nr, None);
                    }
                }
            }
        }
        Some((completed, neighbours, edge))
    }

    fn new_neighbour(object: TreeObject, is: IntBox, r: &IntBox, edge: &mut [bool; 4]) -> Neighbour {
        if is.ll.y == r.ll.y && is.ur.x > r.ll.x && is.ll.x < r.ur.x {
            edge[0] = true;
        }
        if is.ur.x == r.ur.x && is.ur.y > r.ll.y && is.ll.y < r.ur.y {
            edge[1] = true;
        }
        if is.ur.y == r.ur.y && is.ur.x > r.ll.x && is.ll.x < r.ur.x {
            edge[2] = true;
        }
        if is.ll.x == r.ll.x && is.ur.y > r.ll.y && is.ll.y < r.ur.y {
            edge[3] = true;
        }
        let first = if is.ll.y == r.ll.y && is.ll.x > r.ll.x {
            0
        } else if is.ur.x == r.ur.x && is.ll.y > r.ll.y {
            1
        } else if is.ur.y == r.ur.y {
            2
        } else if is.ll.x == r.ll.x {
            3
        } else {
            log::warn!("SortedRoomNeighbour: case not expected");
            -1
        };
        let last = if is.ll.x == r.ll.x && is.ll.y > r.ll.y {
            3
        } else if is.ur.y == r.ur.y && is.ll.x > r.ll.x {
            2
        } else if is.ur.x == r.ur.x {
            1
        } else if is.ll.y == r.ll.y {
            0
        } else {
            log::warn!("SortedRoomNeighbour: case not expected");
            -1
        };
        Neighbour {
            object,
            intersection: is,
            first_touching_side: first,
            last_touching_side: last,
        }
    }

    fn compare(a: &Neighbour, b: &Neighbour) -> Ordering {
        if a.first_touching_side > b.first_touching_side {
            return Ordering::Greater;
        }
        if a.first_touching_side < b.first_touching_side {
            return Ordering::Less;
        }
        let is1 = &a.intersection;
        let is2 = &b.intersection;
        let mut cmp = match a.first_touching_side {
            0 => is1.ll.x.wrapping_sub(is2.ll.x),
            1 => is1.ll.y.wrapping_sub(is2.ll.y),
            2 => is2.ur.x.wrapping_sub(is1.ur.x),
            3 => is2.ur.y.wrapping_sub(is1.ur.y),
            _ => return Ordering::Equal,
        };
        if cmp == 0 {
            let d1 = (a.last_touching_side - a.first_touching_side + 4) % 4;
            let d2 = (b.last_touching_side - b.first_touching_side + 4) % 4;
            if d1 > d2 {
                return Ordering::Greater;
            }
            if d1 < d2 {
                return Ordering::Less;
            }
            cmp = match a.last_touching_side {
                0 => is1.ur.x.wrapping_sub(is2.ur.x),
                1 => is1.ur.y.wrapping_sub(is2.ur.y),
                2 => is2.ll.x.wrapping_sub(is1.ll.x),
                3 => is2.ll.y.wrapping_sub(is1.ll.y),
                _ => return Ordering::Equal,
            };
        }
        if cmp == 0 {
            cmp = object_id(&a.object).wrapping_sub(object_id(&b.object));
        }
        cmp.cmp(&0)
    }

    fn remove_border_line(b: &IntBox, no: usize) -> Option<IntBox> {
        match no {
            0 => Some(IntBox::new(b.ll.x, -CRIT_INT, b.ur.x, b.ur.y)),
            1 => Some(IntBox::new(b.ll.x, b.ll.y, CRIT_INT, b.ur.y)),
            2 => Some(IntBox::new(b.ll.x, b.ll.y, b.ur.x, CRIT_INT)),
            3 => Some(IntBox::new(-CRIT_INT, b.ll.y, b.ur.x, b.ur.y)),
            _ => None,
        }
    }

    fn try_remove_edge(eng: &mut AutorouteEngine, board: &Board, room: RoomKey, completed: RoomKey, edge: &[bool; 4]) -> bool {
        if !eng.room(room).is_incomplete() {
            return false;
        }
        let TileShape::Box(room_box) = eng.room(room).shape().clone() else {
            log::warn!("SortedOrthogonalRoomNeighbours.try_remove_edge: IntBox expected for roomShape type");
            return false;
        };
        let room_area = room_box.area();
        let Some(remove_no) = (0..4).find(|i| !edge[*i]) else {
            return false;
        };
        let Some(enlarged) = remove_border_line(&room_box, remove_no) else {
            return false;
        };
        let (ignore_object, ignore_shape) = max_free_door(eng, completed);
        let inc = IncompleteRoom {
            shape: Some(TileShape::Box(enlarged)),
            layer: eng.room(room).layer,
            contained: eng.room(room).contained_shape().cloned(),
        };
        let new_rooms = complete_shape(board, eng.tree, &inc, eng.net_no, ignore_object.as_ref(), ignore_shape.as_ref());
        if new_rooms.len() == 1 {
            let nr = new_rooms.into_iter().next().unwrap();
            if nr.shape.as_ref().unwrap().area() > room_area {
                set_incomplete_shape(eng, room, nr);
                return true;
            }
        }
        false
    }

    fn insert_incomplete_room(
        eng: &mut AutorouteEngine,
        completed: RoomKey,
        room_shape: &IntBox,
        layer: i32,
        ll_x: i32,
        ll_y: i32,
        ur_x: i32,
        ur_y: i32,
    ) {
        let new_shape = IntBox::new(ll_x, ll_y, ur_x, ur_y);
        if new_shape.dimension() == 2 {
            let contained = room_shape.intersection(&new_shape);
            if !contained.is_empty() {
                let door_dim = new_shape.intersection(room_shape).dimension();
                if door_dim > 0 {
                    add_incomplete_with_door(
                        eng,
                        completed,
                        TileShape::Box(new_shape),
                        layer,
                        TileShape::Box(contained),
                        door_dim,
                    );
                }
            }
        }
    }

    fn calculate_new_incomplete_rooms(eng: &mut AutorouteEngine, board: &Board, from_room: RoomKey, completed: RoomKey, neighbours: &[Neighbour]) {
        let bb = board.bounding_box;
        let TileShape::Box(rs) = eng.room(completed).shape().clone() else {
            return;
        };
        let is_obstacle = eng.room(from_room).is_obstacle();
        let layer = eng.room(from_room).layer;
        let mut prev = neighbours.last().unwrap();
        for next in neighbours {
            if !next.intersection.intersects(&prev.intersection) {
                let pi = &prev.intersection;
                let ni = &next.intersection;
                let ins = |eng: &mut AutorouteEngine, a, b, c, d| insert_incomplete_room(eng, completed, &rs, layer, a, b, c, d);
                match next.first_touching_side {
                    0 => {
                        if prev.last_touching_side == 0 {
                            if pi.ur.x < ni.ll.x {
                                ins(eng, pi.ur.x, bb.ll.y, ni.ll.x, rs.ll.y);
                            }
                        } else if pi.ll.y > rs.ll.y || ni.ll.x > rs.ll.x {
                            if is_obstacle {
                                if prev.last_touching_side == 3 {
                                    ins(eng, bb.ll.x, rs.ll.y, rs.ll.x, pi.ll.y);
                                }
                                ins(eng, rs.ll.x, bb.ll.y, ni.ll.x, rs.ll.y);
                            } else {
                                ins(eng, bb.ll.x, bb.ll.y, ni.ll.x, pi.ll.y);
                            }
                        }
                    }
                    1 => {
                        if prev.last_touching_side == 1 {
                            if pi.ur.y < ni.ll.y {
                                ins(eng, rs.ur.x, pi.ur.y, bb.ur.x, ni.ll.y);
                            }
                        } else if pi.ur.x < rs.ur.x || ni.ll.y > rs.ll.y {
                            if is_obstacle {
                                if prev.last_touching_side == 0 {
                                    ins(eng, pi.ur.x, bb.ll.y, rs.ur.x, rs.ll.y);
                                }
                                ins(eng, rs.ur.x, rs.ll.y, rs.ur.x, ni.ll.y);
                            } else {
                                ins(eng, pi.ur.x, bb.ll.y, bb.ur.x, ni.ll.y);
                            }
                        }
                    }
                    2 => {
                        if prev.last_touching_side == 2 {
                            if pi.ll.x > ni.ur.x {
                                ins(eng, ni.ur.x, rs.ur.y, pi.ll.x, bb.ur.y);
                            }
                        } else if pi.ur.y < rs.ur.y || ni.ur.x < rs.ur.x {
                            if is_obstacle {
                                if prev.last_touching_side == 1 {
                                    ins(eng, rs.ur.x, pi.ur.y, bb.ur.x, rs.ur.y);
                                }
                                ins(eng, ni.ur.x, rs.ur.y, rs.ur.x, bb.ur.y);
                            } else {
                                ins(eng, ni.ur.x, pi.ur.y, bb.ur.x, bb.ur.y);
                            }
                        }
                    }
                    3 => {
                        if prev.last_touching_side == 3 {
                            if pi.ll.y > ni.ur.y {
                                ins(eng, bb.ll.x, ni.ur.y, rs.ll.x, pi.ll.y);
                            }
                        } else if ni.ur.y < rs.ur.y || pi.ll.x > rs.ll.x {
                            if is_obstacle {
                                if prev.last_touching_side == 2 {
                                    ins(eng, rs.ll.x, rs.ur.y, pi.ll.x, bb.ur.y);
                                }
                                ins(eng, bb.ll.x, ni.ur.y, rs.ll.x, rs.ur.y);
                            } else {
                                ins(eng, bb.ll.x, ni.ur.y, pi.ll.x, bb.ur.y);
                            }
                        }
                    }
                    _ => log::warn!("SortedOrthogonalRoomNeighbour.calculate_new_incomplete: illegal touching side"),
                }
            }
            prev = next;
        }
    }
}

/// The 2-dimensional door to a complete free space room with maximal area
/// (ignore object and shape for `completeShape` in `tryRemoveEdge`).
fn max_free_door(eng: &AutorouteEngine, completed: RoomKey) -> (Option<TreeObject>, Option<TileShape>) {
    let mut ignore_shape = None;
    let mut ignore_object = None;
    let mut max_area = 0.0;
    for d in eng.room(completed).doors.clone() {
        if eng.door(d).dimension != 2 {
            continue;
        }
        let Some(other) = eng.expandable_other_room(super::engine::Expandable::Door(d), Some(completed)) else {
            continue;
        };
        if eng.room(other).is_complete_free() {
            let shape = eng.door_shape(d);
            let area = shape.area();
            if area > max_area {
                max_area = area;
                ignore_shape = Some(shape);
                ignore_object = eng.room(other).tree_object(other);
            }
        }
    }
    (ignore_object, ignore_shape)
}

// ==================================================================== 45 degree

mod deg45 {
    use super::*;

    struct Neighbour {
        object: TreeObject,
        intersection: IntOctagon,
        first_touching_side: i32,
        last_touching_side: i32,
    }

    pub(super) fn calculate(eng: &mut AutorouteEngine, board: &Board, room: RoomKey) -> Option<RoomKey> {
        let id_no = eng.generate_room_id_no();
        let (completed, neighbours, edge, room_oct) = calculate_neighbours(eng, board, room, id_no)?;
        let edge_removed = try_remove_edge_line(eng, board, room, completed, &edge, &room_oct);
        if edge_removed {
            eng.remove_all_doors(completed);
            return calculate(eng, board, room);
        }
        if neighbours.is_empty() {
            if eng.room(completed).is_obstacle() {
                calculate_edge_incomplete_rooms_of_obstacle_room(eng, board, room, completed, &room_oct, 0, 7);
            }
        } else {
            calculate_new_incomplete_rooms(eng, board, room, completed, &room_oct, &neighbours);
        }
        Some(completed)
    }

    #[allow(clippy::type_complexity)]
    fn calculate_neighbours(
        eng: &mut AutorouteEngine,
        board: &Board,
        room: RoomKey,
        id_no: i32,
    ) -> Option<(RoomKey, Vec<Neighbour>, [bool; 8], IntOctagon)> {
        let room_shape = eng.room(room).shape().clone();
        let layer = eng.room(room).layer;
        let completed = completed_room_for(eng, room, id_no)?;
        let room_oct = eng.room(completed).shape().bounding_octagon();
        let mut edge = [false; 8];
        let mut neighbours: Vec<Neighbour> = Vec::new();
        let overlaps = sorted_overlaps(eng, board, &room_shape, layer);
        for e in &overlaps {
            if eng.room(completed).is_complete_free() && !is_trace_obstacle(eng, board, &e.object) {
                calculate_target_doors(eng, board, completed, e);
                continue;
            }
            let current_shape = tree_shape(eng, board, e);
            let current_oct = current_shape.bounding_octagon();
            let intersection = room_oct.intersection(&current_oct);
            let dimension = intersection.dimension();
            if dimension > 1 && eng.room(completed).is_obstacle() {
                if let TreeObject::Item(id) = e.object {
                    if board.item(id).is_routable() {
                        if let Some(overlap_room) = eng.get_obstacle_room(board, id, e.shape_index) {
                            eng.create_overlap_door(board, completed, overlap_room);
                        }
                    }
                }
                continue;
            }
            if dimension < 0 {
                continue;
            }
            if let Some(n) = new_neighbour(e.object, intersection, &room_oct, &mut edge) {
                sorted_insert(&mut neighbours, n, compare);
            }
            if dimension > 0 {
                if let Some(nr) = neighbour_room(eng, board, e) {
                    if insert_door_ok(eng, board, completed, nr, &TileShape::Octagon(intersection)) {
                        connect_rooms(eng, completed, nr, None);
                    }
                }
            }
        }
        Some((completed, neighbours, edge, room_oct))
    }

    fn new_neighbour(object: TreeObject, is: IntOctagon, r: &IntOctagon, edge: &mut [bool; 8]) -> Option<Neighbour> {
        let first = if is.ly == r.ly && is.llx > r.llx {
            0
        } else if is.lrx == r.lrx && is.ly > r.ly {
            1
        } else if is.rx == r.rx && is.lrx < r.lrx {
            2
        } else if is.urx == r.urx && is.rx < r.rx {
            3
        } else if is.uy == r.uy && is.urx < r.urx {
            4
        } else if is.ulx == r.ulx && is.uy < r.uy {
            5
        } else if is.lx == r.lx && is.ulx > r.ulx {
            6
        } else if is.llx == r.llx && is.lx > r.lx {
            7
        } else {
            return None;
        };
        let last = if is.llx == r.llx && is.ly > r.ly {
            7
        } else if is.lx == r.lx && is.llx > r.llx {
            6
        } else if is.ulx == r.ulx && is.lx > r.lx {
            5
        } else if is.uy == r.uy && is.ulx > r.ulx {
            4
        } else if is.urx == r.urx && is.uy < r.uy {
            3
        } else if is.rx == r.rx && is.urx < r.urx {
            2
        } else if is.lrx == r.lrx && is.rx < r.rx {
            1
        } else if is.ly == r.ly && is.lrx < r.lrx {
            0
        } else {
            return None;
        };
        let mut next_side = first;
        loop {
            let current = next_side;
            next_side = (next_side + 1) % 8;
            if !edge[current as usize] {
                let mut touch_only_at_corner = false;
                if current == first && is.corner(current as usize) == r.corner(next_side as usize) {
                    touch_only_at_corner = true;
                }
                if current == last && is.corner(next_side as usize) == r.corner(current as usize) {
                    touch_only_at_corner = true;
                }
                if !touch_only_at_corner {
                    edge[current as usize] = true;
                }
            }
            if current == last {
                break;
            }
        }
        Some(Neighbour {
            object,
            intersection: is,
            first_touching_side: first,
            last_touching_side: last,
        })
    }

    fn compare(a: &Neighbour, b: &Neighbour) -> Ordering {
        if a.first_touching_side > b.first_touching_side {
            return Ordering::Greater;
        }
        if a.first_touching_side < b.first_touching_side {
            return Ordering::Less;
        }
        let is1 = &a.intersection;
        let is2 = &b.intersection;
        let mut cmp = match a.first_touching_side {
            0 => is1.corner(0).x.wrapping_sub(is2.corner(0).x),
            1 => is1.corner(1).x.wrapping_sub(is2.corner(1).x),
            2 => is1.corner(2).y.wrapping_sub(is2.corner(2).y),
            3 => is1.corner(3).y.wrapping_sub(is2.corner(3).y),
            4 => is2.corner(4).x.wrapping_sub(is1.corner(4).x),
            5 => is2.corner(5).x.wrapping_sub(is1.corner(5).x),
            6 => is2.corner(6).y.wrapping_sub(is1.corner(6).y),
            7 => is2.corner(7).y.wrapping_sub(is1.corner(7).y),
            _ => return Ordering::Equal,
        };
        if cmp == 0 {
            let d1 = (a.last_touching_side - a.first_touching_side + 8) % 8;
            let d2 = (b.last_touching_side - b.first_touching_side + 8) % 8;
            if d1 > d2 {
                return Ordering::Greater;
            }
            if d1 < d2 {
                return Ordering::Less;
            }
            cmp = match a.last_touching_side {
                0 => is1.corner(1).x.wrapping_sub(is2.corner(1).x),
                1 => is1.corner(2).x.wrapping_sub(is2.corner(2).x),
                2 => is1.corner(3).y.wrapping_sub(is2.corner(3).y),
                3 => is1.corner(4).y.wrapping_sub(is2.corner(4).y),
                4 => is2.corner(5).x.wrapping_sub(is1.corner(5).x),
                5 => is2.corner(6).x.wrapping_sub(is1.corner(6).x),
                6 => is2.corner(7).y.wrapping_sub(is1.corner(7).y),
                7 => is2.corner(0).y.wrapping_sub(is1.corner(0).y),
                _ => 0,
            };
        }
        if cmp == 0 {
            cmp = object_id(&a.object).wrapping_sub(object_id(&b.object));
        }
        cmp.cmp(&0)
    }

    fn remove_not_touching_border_lines(r: &IntOctagon, edge: &[bool; 8]) -> IntOctagon {
        IntOctagon::new(
            if edge[6] { r.lx } else { -CRIT_INT },
            if edge[0] { r.ly } else { -CRIT_INT },
            if edge[2] { r.rx } else { CRIT_INT },
            if edge[4] { r.uy } else { CRIT_INT },
            if edge[5] { r.ulx } else { -CRIT_INT },
            if edge[1] { r.lrx } else { CRIT_INT },
            if edge[7] { r.llx } else { -CRIT_INT },
            if edge[3] { r.urx } else { CRIT_INT },
        )
        .normalize()
    }

    fn try_remove_edge_line(
        eng: &mut AutorouteEngine,
        board: &Board,
        room: RoomKey,
        completed: RoomKey,
        edge: &[bool; 8],
        room_oct: &IntOctagon,
    ) -> bool {
        if !eng.room(room).is_incomplete() {
            return false;
        }
        let TileShape::Octagon(oct) = eng.room(room).shape().clone() else {
            log::warn!("Sorted45DegreeRoomNeighbours.tryRemoveEdgeLine: IntOctagon expected for roomShape type");
            return false;
        };
        let room_area = oct.area();
        let mut try_remove = false;
        for i in 0..8 {
            if !edge[i] {
                let prev = room_oct.corner(i).to_float();
                let next = room_oct.corner((i + 1) % 8).to_float();
                if prev.distance_square(&next) > 1.0 {
                    try_remove = true;
                    break;
                }
            }
        }
        if !try_remove {
            return false;
        }
        let enlarged = remove_not_touching_border_lines(&oct, edge);
        let (ignore_object, ignore_shape) = max_free_door(eng, completed);
        let inc = IncompleteRoom {
            shape: Some(TileShape::Octagon(enlarged)),
            layer: eng.room(room).layer,
            contained: eng.room(room).contained_shape().cloned(),
        };
        let new_rooms = complete_shape(board, eng.tree, &inc, eng.net_no, ignore_object.as_ref(), ignore_shape.as_ref());
        if new_rooms.len() == 1 {
            let nr = new_rooms.into_iter().next().unwrap();
            if nr.shape.as_ref().unwrap().area() > room_area {
                set_incomplete_shape(eng, room, nr);
                return true;
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_incomplete_room(
        eng: &mut AutorouteEngine,
        completed: RoomKey,
        room_oct: &IntOctagon,
        layer: i32,
        lx: i32,
        ly: i32,
        rx: i32,
        uy: i32,
        ulx: i32,
        lrx: i32,
        llx: i32,
        urx: i32,
    ) {
        let new_shape = IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx).normalize();
        if new_shape.dimension() == 2 {
            let contained = room_oct.intersection(&new_shape);
            if !contained.is_empty() {
                let door_dim = contained.dimension();
                if door_dim > 0 {
                    add_incomplete_with_door(
                        eng,
                        completed,
                        TileShape::Octagon(new_shape),
                        layer,
                        TileShape::Octagon(contained),
                        door_dim,
                    );
                }
            }
        }
    }

    fn calculate_edge_incomplete_rooms_of_obstacle_room(
        eng: &mut AutorouteEngine,
        board: &Board,
        from_room: RoomKey,
        completed: RoomKey,
        rs: &IntOctagon,
        from_side: usize,
        to_side: usize,
    ) {
        if !eng.room(from_room).is_obstacle() {
            log::warn!("Sorted45DegreeRoomNeighbours.calculate_side_incomplete_rooms_of_obstacle_expansion_room: ObstacleExpansionRoom expected for this.fromRoom");
            return;
        }
        let layer = eng.room(from_room).layer;
        let b = board.bounding_box.to_int_octagon();
        let mut current_corner: IntPoint = rs.corner(from_side);
        let mut current_side = from_side;
        loop {
            let next_side = (current_side + 1) % 8;
            let next_corner = rs.corner(next_side);
            if current_corner != next_corner {
                let mut s = b;
                match current_side {
                    0 => s.uy = rs.ly,
                    1 => s.ulx = rs.lrx,
                    2 => s.lx = rs.rx,
                    3 => s.llx = rs.urx,
                    4 => s.ly = rs.uy,
                    5 => s.lrx = rs.ulx,
                    6 => s.rx = rs.lx,
                    7 => s.urx = rs.llx,
                    _ => return,
                }
                insert_incomplete_room(eng, completed, rs, layer, s.lx, s.ly, s.rx, s.uy, s.ulx, s.lrx, s.llx, s.urx);
            }
            if current_side == to_side {
                break;
            }
            current_side = next_side;
            // The original does not advance the current corner.
            let _ = &mut current_corner;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn calculate_new_incomplete_rooms_for_obstacle_room(
        eng: &mut AutorouteEngine,
        board: &Board,
        from_room: RoomKey,
        completed: RoomKey,
        rs: &IntOctagon,
        prev: &Neighbour,
        next: &Neighbour,
        same: bool,
    ) {
        let from_side = prev.last_touching_side;
        let to_side = next.first_touching_side;
        if from_side == to_side && !same {
            return;
        }
        let layer = eng.room(from_room).layer;
        let b = board.bounding_box.to_int_octagon();
        let pi = &prev.intersection;
        let mut s = b;
        match from_side {
            0 => {
                s.uy = rs.ly;
                s.ulx = pi.lrx;
            }
            1 => {
                s.ulx = rs.lrx;
                s.lx = pi.rx;
            }
            2 => {
                s.lx = rs.rx;
                s.llx = pi.urx;
            }
            3 => {
                s.llx = rs.urx;
                s.ly = pi.uy;
            }
            4 => {
                s.ly = rs.uy;
                s.lrx = pi.ulx;
            }
            5 => {
                s.lrx = rs.ulx;
                s.rx = pi.lx;
            }
            6 => {
                s.rx = rs.lx;
                s.urx = pi.llx;
            }
            7 => {
                s.urx = rs.llx;
                s.uy = pi.ly;
            }
            _ => {}
        }
        insert_incomplete_room(eng, completed, rs, layer, s.lx, s.ly, s.rx, s.uy, s.ulx, s.lrx, s.llx, s.urx);
        let ni = &next.intersection;
        let mut s = b;
        match to_side {
            0 => {
                s.uy = rs.ly;
                s.urx = ni.llx;
            }
            1 => {
                s.ulx = rs.lrx;
                s.uy = ni.ly;
            }
            2 => {
                s.lx = rs.rx;
                s.ulx = ni.lrx;
            }
            3 => {
                s.llx = rs.urx;
                s.lx = ni.rx;
            }
            4 => {
                s.ly = rs.uy;
                s.llx = ni.urx;
            }
            5 => {
                s.lrx = rs.ulx;
                s.ly = ni.uy;
            }
            6 => {
                s.rx = rs.lx;
                s.lrx = ni.ulx;
            }
            7 => {
                s.urx = rs.llx;
                s.rx = ni.lx;
            }
            _ => {}
        }
        insert_incomplete_room(eng, completed, rs, layer, s.lx, s.ly, s.rx, s.uy, s.ulx, s.lrx, s.llx, s.urx);
        let current_from = (from_side + 1).rem_euclid(8);
        if current_from == to_side {
            return;
        }
        let current_to = (to_side + 7).rem_euclid(8);
        calculate_edge_incomplete_rooms_of_obstacle_room(
            eng,
            board,
            from_room,
            completed,
            rs,
            current_from as usize,
            current_to as usize,
        );
    }

    fn calculate_new_incomplete_rooms(
        eng: &mut AutorouteEngine,
        board: &Board,
        from_room: RoomKey,
        completed: RoomKey,
        rs: &IntOctagon,
        neighbours: &[Neighbour],
    ) {
        let b = board.bounding_box.to_int_octagon();
        let last_index = neighbours.len() - 1;
        let is_obstacle = eng.room(from_room).is_obstacle();
        if is_obstacle && neighbours.len() == 1 {
            let n = &neighbours[last_index];
            calculate_new_incomplete_rooms_for_obstacle_room(eng, board, from_room, completed, rs, n, n, true);
            return;
        }
        let layer = eng.room(from_room).layer;
        let mut prev_index = last_index;
        for (next_index, next) in neighbours.iter().enumerate() {
            let prev = &neighbours[prev_index];
            let intersection = next.intersection.intersection(&prev.intersection);
            let insert = if intersection.is_empty() {
                true
            } else if intersection.dimension() >= 1 {
                false
            } else if prev.last_touching_side == next.first_touching_side {
                false
            } else {
                prev.last_touching_side != (next.first_touching_side + 1) % 8
            };
            if insert {
                if is_obstacle && next.first_touching_side != prev.last_touching_side {
                    calculate_new_incomplete_rooms_for_obstacle_room(
                        eng,
                        board,
                        from_room,
                        completed,
                        rs,
                        prev,
                        next,
                        prev_index == next_index,
                    );
                } else {
                    let (mut lx, mut ly, mut rx, mut uy, mut ulx, mut lrx, mut llx, mut urx) =
                        (b.lx, b.ly, b.rx, b.uy, b.ulx, b.lrx, b.llx, b.urx);
                    let pi = &prev.intersection;
                    let ni = &next.intersection;
                    match next.first_touching_side {
                        0 => {
                            if pi.llx < ni.llx {
                                urx = ni.llx;
                                uy = pi.ly;
                                if prev.last_touching_side == 0 {
                                    ulx = pi.lrx;
                                }
                            } else if pi.llx > ni.llx {
                                rx = ni.lx;
                                urx = pi.llx;
                            } else {
                                urx = ni.llx;
                            }
                        }
                        1 => {
                            if pi.ly < ni.ly {
                                uy = ni.ly;
                                ulx = pi.lrx;
                                if prev.last_touching_side == 1 {
                                    lx = pi.rx;
                                }
                            } else if pi.ly > ni.ly {
                                uy = pi.ly;
                                urx = ni.llx;
                            } else {
                                uy = ni.ly;
                            }
                        }
                        2 => {
                            if pi.lrx > ni.lrx {
                                ulx = ni.lrx;
                                lx = pi.rx;
                                if prev.last_touching_side == 2 {
                                    llx = pi.urx;
                                }
                            } else if pi.lrx < ni.lrx {
                                uy = ni.ly;
                                ulx = pi.lrx;
                            } else {
                                ulx = ni.lrx;
                            }
                        }
                        3 => {
                            if pi.rx > ni.rx {
                                lx = ni.rx;
                                llx = pi.urx;
                                if prev.last_touching_side == 3 {
                                    ly = pi.uy;
                                }
                            } else if pi.rx < ni.rx {
                                lx = pi.rx;
                                ulx = ni.lrx;
                            } else {
                                lx = ni.rx;
                            }
                        }
                        4 => {
                            if pi.urx > ni.urx {
                                llx = ni.urx;
                                ly = pi.uy;
                                if prev.last_touching_side == 4 {
                                    lrx = pi.ulx;
                                }
                            } else if pi.urx < ni.urx {
                                lx = ni.rx;
                                llx = pi.urx;
                            } else {
                                llx = ni.urx;
                            }
                        }
                        5 => {
                            if pi.uy > ni.uy {
                                ly = ni.uy;
                                lrx = pi.ulx;
                                if prev.last_touching_side == 5 {
                                    rx = pi.lx;
                                }
                            } else if pi.uy < ni.uy {
                                ly = pi.uy;
                                llx = ni.urx;
                            } else {
                                ly = ni.uy;
                            }
                        }
                        6 => {
                            if pi.ulx < ni.ulx {
                                lrx = ni.ulx;
                                rx = pi.lx;
                                if prev.last_touching_side == 6 {
                                    urx = pi.llx;
                                }
                            } else if pi.ulx > ni.ulx {
                                ly = ni.uy;
                                lrx = pi.ulx;
                            } else {
                                lrx = ni.ulx;
                            }
                        }
                        7 => {
                            if pi.lx < ni.lx {
                                rx = ni.lx;
                                urx = pi.llx;
                                if prev.last_touching_side == 7 {
                                    uy = pi.ly;
                                }
                            } else if pi.lx > ni.lx {
                                rx = pi.lx;
                                lrx = ni.ulx;
                            } else {
                                rx = ni.lx;
                            }
                        }
                        _ => log::warn!("Sorted45DegreeRoomNeighbour.calculate_new_incomplete: illegal touching side"),
                    }
                    insert_incomplete_room(eng, completed, rs, layer, lx, ly, rx, uy, ulx, lrx, llx, urx);
                }
            }
            prev_index = next_index;
        }
    }
}

// ==================================================================== any angle

mod any {
    use super::*;

    const DIST_TOLERANCE: f64 = 1.0;

    struct Neighbour {
        object: TreeObject,
        neighbour_shape: TileShape,
        touching_side_no_of_room: usize,
        touching_side_no_of_neighbour_room: usize,
        room_touch_is_corner: bool,
        neighbour_room_touch_is_corner: bool,
        first_corner: Point,
        last_corner: Point,
    }

    fn compare_from(this: &IntDirection, p1: &IntDirection, p2: &IntDirection) -> i32 {
        let ord = |a: &IntDirection, b: &IntDirection| match a.compare_to(b) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        };
        if ord(p1, this) >= 0 {
            if ord(p2, this) >= 0 {
                ord(p1, p2)
            } else {
                -1
            }
        } else if ord(p2, this) >= 0 {
            1
        } else {
            ord(p1, p2)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new_neighbour(
        object: TreeObject,
        room_shape: &TileShape,
        neighbour_shape: TileShape,
        touching_side_no_of_room: usize,
        touching_side_no_of_neighbour_room: usize,
        room_touch_is_corner: bool,
        neighbour_room_touch_is_corner: bool,
    ) -> Neighbour {
        let first_corner = if room_touch_is_corner {
            room_shape.corner(touching_side_no_of_room)
        } else if neighbour_room_touch_is_corner {
            neighbour_shape.corner(touching_side_no_of_neighbour_room)
        } else {
            let c = neighbour_shape.corner(neighbour_shape.next_no(touching_side_no_of_neighbour_room));
            let prev_line = room_shape.border_line(room_shape.prev_no(touching_side_no_of_room));
            if prev_line.side_of(&c) == Side::OnTheRight {
                c
            } else {
                room_shape.corner(touching_side_no_of_room)
            }
        };
        let last_corner = if room_touch_is_corner {
            room_shape.corner(touching_side_no_of_room)
        } else if neighbour_room_touch_is_corner {
            neighbour_shape.corner(touching_side_no_of_neighbour_room)
        } else {
            let c = neighbour_shape.corner(touching_side_no_of_neighbour_room);
            let next_line = room_shape.border_line(room_shape.next_no(touching_side_no_of_room));
            if next_line.side_of(&c) == Side::OnTheRight {
                c
            } else {
                room_shape.corner(room_shape.next_no(touching_side_no_of_room))
            }
        };
        Neighbour {
            object,
            neighbour_shape,
            touching_side_no_of_room,
            touching_side_no_of_neighbour_room,
            room_touch_is_corner,
            neighbour_room_touch_is_corner,
            first_corner,
            last_corner,
        }
    }

    fn compare(room_shape: &TileShape, a: &Neighbour, b: &Neighbour) -> Ordering {
        let cv = a.touching_side_no_of_room as i32 - b.touching_side_no_of_room as i32;
        if cv != 0 {
            return cv.cmp(&0);
        }
        let compare_corner = room_shape.corner_approx(a.touching_side_no_of_room);
        let this_d = a.first_corner.to_float().distance(&compare_corner);
        let other_d = b.first_corner.to_float().distance(&compare_corner);
        let mut delta = this_d - other_d;
        if delta.abs() <= DIST_TOLERANCE && a.first_corner == b.first_corner {
            let this_d2 = a.last_corner.to_float().distance(&compare_corner);
            let other_d2 = b.last_corner.to_float().distance(&compare_corner);
            delta = this_d2 - other_d2;
            if delta.abs() <= DIST_TOLERANCE && a.neighbour_room_touch_is_corner && b.neighbour_room_touch_is_corner {
                let mut compare_line_no = a.touching_side_no_of_room;
                if a.room_touch_is_corner {
                    compare_line_no = room_shape.prev_no(compare_line_no);
                }
                let compare_dir = room_shape.border_line(compare_line_no).direction().opposite();
                let this_line = a.neighbour_shape.border_line(a.touching_side_no_of_neighbour_room);
                let other_line = b.neighbour_shape.border_line(b.touching_side_no_of_neighbour_room);
                delta = compare_from(&compare_dir, &this_line.direction(), &other_line.direction()) as f64;
            }
        }
        let mut res = Signum::as_int(delta);
        if res == 0 {
            res = object_id(&a.object).wrapping_sub(object_id(&b.object));
        }
        res.cmp(&0)
    }

    pub(super) fn calculate(eng: &mut AutorouteEngine, board: &Board, room: RoomKey) -> Option<RoomKey> {
        let id_no = eng.generate_room_id_no();
        let (completed, neighbours, own_net_objects, room_shape) = calculate_neighbours(eng, board, room, id_no)?;
        let edge_removed = try_remove_edge(eng, board, room, &neighbours);
        if edge_removed {
            eng.remove_all_doors(completed);
            return calculate(eng, board, room);
        }
        if neighbours.is_empty() {
            if eng.room(completed).is_obstacle() {
                calculate_incomplete_rooms_with_empty_neighbours(eng, board, room);
            }
        } else {
            calculate_new_incomplete_rooms(eng, room, completed, &room_shape, &neighbours);
        }
        if eng.room(completed).is_complete_free() {
            if !own_net_objects.is_empty() {
                if let RoomKind::Complete { net_dependent, .. } = &mut eng.room_mut(completed).kind {
                    *net_dependent = true;
                }
            }
            for e in &own_net_objects {
                add_target_door_if_connected(eng, board, completed, e);
            }
        }
        Some(completed)
    }

    fn calculate_incomplete_rooms_with_empty_neighbours(eng: &mut AutorouteEngine, board: &Board, room: RoomKey) {
        let room_shape = eng.room(room).shape().clone();
        let layer = eng.room(room).layer;
        for i in 0..room_shape.border_line_count() {
            let line = room_shape.border_line(i);
            if insert_door_ok_obstacle(eng, board, room, Some(&line)) {
                let new_shape = TileShape::Simplex(Simplex::new(vec![line.opposite()]));
                let contained = room_shape.intersection(&new_shape);
                let new_room = eng.add_incomplete_expansion_room(Some(new_shape), layer, Some(contained));
                let d = eng.new_door(room, new_room, 1);
                eng.add_door(room, d);
                eng.add_door(new_room, d);
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn calculate_neighbours(
        eng: &mut AutorouteEngine,
        board: &Board,
        room: RoomKey,
        id_no: i32,
    ) -> Option<(RoomKey, Vec<Neighbour>, Vec<TreeEntry>, TileShape)> {
        let room_shape = eng.room(room).shape().clone();
        let layer = eng.room(room).layer;
        let room_is_incomplete = eng.room(room).is_incomplete();
        let completed = completed_room_for(eng, room, id_no)?;
        let completed_shape = eng.room(completed).shape().clone();
        let mut neighbours: Vec<Neighbour> = Vec::new();
        let mut own_net_objects = Vec::new();
        let overlaps = sorted_overlaps(eng, board, &room_shape, layer);
        for e in &overlaps {
            if room_is_incomplete && !is_trace_obstacle(eng, board, &e.object) {
                own_net_objects.push(*e);
                continue;
            }
            let current_shape = tree_shape(eng, board, e);
            let intersection = room_shape.intersection(&current_shape);
            let dimension = intersection.dimension();
            if dimension > 1 {
                if eng.room(completed).is_obstacle() {
                    if let TreeObject::Item(id) = e.object {
                        if board.item(id).is_routable() {
                            if let Some(overlap_room) = eng.get_obstacle_room(board, id, e.shape_index) {
                                eng.create_overlap_door(board, completed, overlap_room);
                            }
                        }
                    }
                }
                continue;
            }
            if dimension < 0 {
                log::debug!("SortedRoomNeighbours.calculate: dimension >= 0 expected");
                continue;
            }
            if dimension == 1 {
                let Some((s0, s1)) = room_shape.touching_sides(&current_shape) else {
                    log::debug!("SortedRoomNeighbours.calculate: touchingSides length 2 expected");
                    continue;
                };
                let n = new_neighbour(e.object, &completed_shape, current_shape.clone(), s0, s1, false, false);
                sorted_insert(&mut neighbours, n, |a, b| compare(&completed_shape, a, b));
                if let Some(nr) = neighbour_room(eng, board, e) {
                    if insert_door_ok(eng, board, completed, nr, &intersection) {
                        connect_rooms(eng, completed, nr, Some(1));
                    }
                }
            } else {
                let touching_point = intersection.corner(0);
                let room_corner_no = room_shape.equals_corner(&touching_point);
                let (room_touch_is_corner, side_no_of_room) = if room_corner_no >= 0 {
                    (true, room_corner_no)
                } else {
                    let n = room_shape.contains_on_border_line_no(&touching_point);
                    if n < 0 {
                        log::debug!("SortedRoomNeighbours.calculate: touchingSideNoOfRoom >= 0 expected");
                    }
                    (false, n)
                };
                let neighbour_corner_no = current_shape.equals_corner(&touching_point);
                let (neighbour_touch_is_corner, side_no_of_neighbour) = if neighbour_corner_no >= 0 {
                    (true, current_shape.prev_no(neighbour_corner_no as usize) as i32)
                } else {
                    let n = current_shape.contains_on_border_line_no(&touching_point);
                    if n < 0 {
                        log::debug!("SortedRoomNeighbours.calculate: touchingSideNoOfNeighbourRoom >= 0 expected");
                    }
                    (false, n)
                };
                if side_no_of_room < 0 || side_no_of_neighbour < 0 {
                    continue;
                }
                let n = new_neighbour(
                    e.object,
                    &completed_shape,
                    current_shape.clone(),
                    side_no_of_room as usize,
                    side_no_of_neighbour as usize,
                    room_touch_is_corner,
                    neighbour_touch_is_corner,
                );
                sorted_insert(&mut neighbours, n, |a, b| compare(&completed_shape, a, b));
            }
        }
        Some((completed, neighbours, own_net_objects, completed_shape))
    }

    fn try_remove_edge(eng: &mut AutorouteEngine, board: &Board, room: RoomKey, neighbours: &[Neighbour]) -> bool {
        if !eng.room(room).is_incomplete() {
            return false;
        }
        let mut remove_edge_no: i32 = -1;
        let room_simplex = eng.room(room).shape().to_simplex();
        let room_area = TileShape::Simplex(room_simplex.clone()).area();
        let mut prev_edge_no: i32 = -1;
        let mut current_edge_no: i32 = 0;
        for n in neighbours {
            let s = n.touching_side_no_of_room as i32;
            if s == prev_edge_no {
                continue;
            }
            if s == current_edge_no {
                prev_edge_no = current_edge_no;
                current_edge_no += 1;
            } else {
                remove_edge_no = current_edge_no;
                break;
            }
        }
        if remove_edge_no < 0 && (current_edge_no as usize) < room_simplex.border_line_count() {
            remove_edge_no = current_edge_no;
        }
        if remove_edge_no < 0 {
            return false;
        }
        let enlarged = room_simplex.remove_border_line(remove_edge_no as usize);
        let inc = IncompleteRoom {
            shape: Some(TileShape::Simplex(enlarged)),
            layer: eng.room(room).layer,
            contained: eng.room(room).contained_shape().cloned(),
        };
        let new_rooms = complete_shape(board, eng.tree, &inc, eng.net_no, None, None);
        if new_rooms.len() != 1 {
            return false;
        }
        let nr = new_rooms.into_iter().next().unwrap();
        if nr.shape.as_ref().unwrap().area() > room_area {
            set_incomplete_shape(eng, room, nr);
            return true;
        }
        false
    }

    fn calculate_new_incomplete_rooms(
        eng: &mut AutorouteEngine,
        from_room: RoomKey,
        completed: RoomKey,
        _room_shape: &TileShape,
        neighbours: &[Neighbour],
    ) {
        let last_index = neighbours.len() - 1;
        let mut prev_index = last_index;
        let room_simplex = eng.room(from_room).shape().to_simplex();
        let room_simplex_shape = TileShape::Simplex(room_simplex.clone());
        let from_is_incomplete = eng.room(from_room).is_incomplete();
        let layer = eng.room(from_room).layer;
        for (next_index, next) in neighbours.iter().enumerate() {
            let prev = &neighbours[prev_index];
            let mut first_touching = prev.touching_side_no_of_room;
            let mut last_touching = next.touching_side_no_of_room;
            let current_next_no = room_simplex_shape.next_no(first_touching);
            let prev_is_last = prev_index == last_index;
            let ends_at_corner = (first_touching != last_touching || prev_is_last)
                && prev.last_corner == room_simplex_shape.corner(current_next_no);
            let starts_at_corner = (first_touching != last_touching || prev_is_last)
                && next.first_corner == room_simplex_shape.corner(last_touching);
            if ends_at_corner {
                first_touching = current_next_no;
            }
            if starts_at_corner {
                last_touching = room_simplex_shape.prev_no(last_touching);
            }
            let neighbours_touch = neighbours.len() > 1 && prev.last_corner == next.first_corner;
            if !neighbours_touch {
                let mut last_bounding_line_no = prev.touching_side_no_of_neighbour_room;
                if !(ends_at_corner || prev.room_touch_is_corner) {
                    last_bounding_line_no = prev.neighbour_shape.prev_no(last_bounding_line_no);
                }
                let mut first_bounding_line_no = next.touching_side_no_of_neighbour_room;
                if !(starts_at_corner || next.neighbour_room_touch_is_corner) {
                    first_bounding_line_no = next.neighbour_shape.next_no(first_bounding_line_no);
                }
                let mut start_edge_line = Some(next.neighbour_shape.border_line(first_bounding_line_no).opposite());
                let mut middle_edge_line: Option<Line> = None;
                let mut current_touching = last_touching;
                let mut first_time = true;
                loop {
                    let mut corner_cut_off = false;
                    if from_is_incomplete && current_touching == last_touching && first_touching != last_touching {
                        let cut_start = prev.last_corner.to_float().round();
                        let cut_end = next.first_corner.to_float().round();
                        let cut_line = Line::from_points(&Point::Int(cut_start), &Point::Int(cut_end));
                        let cut_half_plane = TileShape::from_line(&cut_line);
                        let new_shape = eng.room(completed).shape().intersection(&cut_half_plane);
                        eng.room_mut(completed).shape = Some(new_shape);
                        corner_cut_off = eng
                            .room(from_room)
                            .contained_shape()
                            .map(|c| c.side_of(&cut_line) == Side::OnTheLeft)
                            .unwrap_or(false);
                        if corner_cut_off {
                            middle_edge_line = Some(cut_line.opposite());
                        }
                    }
                    let next_touching = room_simplex_shape.prev_no(current_touching);
                    if !corner_cut_off {
                        middle_edge_line = Some(room_simplex.border_line(current_touching).opposite());
                    }
                    let middle = middle_edge_line.unwrap();
                    let middle_dir = middle.direction();
                    let last_time = (current_touching == first_touching && !(prev_is_last && first_time)) || corner_cut_off;
                    let mut end_edge_line = None;
                    if last_time {
                        let l = prev.neighbour_shape.border_line(last_bounding_line_no).opposite();
                        if l.direction().get_vector().side_of(&middle_dir.get_vector()) == Side::OnTheLeft {
                            end_edge_line = Some(l);
                        }
                    }
                    if let Some(s) = &start_edge_line {
                        if middle_dir.get_vector().side_of(&s.direction().get_vector()) != Side::OnTheLeft {
                            start_edge_line = None;
                        }
                    }
                    let mut lines = Vec::with_capacity(3);
                    if let Some(s) = start_edge_line {
                        lines.push(s);
                    }
                    lines.push(middle);
                    if let Some(e) = end_edge_line {
                        lines.push(e);
                    }
                    let new_shape = Simplex::get_instance(&lines);
                    if !new_shape.is_empty() {
                        let new_shape = TileShape::Simplex(new_shape);
                        let contained = eng.room(completed).shape().intersection(&new_shape);
                        if !contained.is_empty() {
                            let new_room = eng.add_incomplete_expansion_room(Some(new_shape), layer, Some(contained));
                            let d = eng.new_door(completed, new_room, 1);
                            eng.add_door(completed, d);
                            eng.add_door(new_room, d);
                        }
                    }
                    if last_time {
                        break;
                    }
                    current_touching = next_touching;
                    start_edge_line = None;
                    first_time = false;
                }
            }
            prev_index = next_index;
        }
    }
}

#[allow(dead_code)]
fn _unused(_: IntPoint) {}
