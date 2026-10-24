//! `completeShape` of the search trees (`ShapeSearchTree`, `ShapeSearchTree45Degree`,
//! `ShapeSearchTree90Degree`): calculates the maximal free space rooms containing a
//! given shape.

use super::engine::IncompleteRoom;
use crate::board::{Board, ShapeSearchTree, TreeId, TreeKind, TreeObject};
use crate::datastructures::shape_tree::NodeIdx;
use crate::geometry::{IntBox, IntOctagon, Line, LineSegment, RegularTileShape, Side, TileShape};

/// `completeShape(room, netNumber, ignoreObject, ignoreShape)`
pub fn complete_shape(
    board: &Board,
    tree: TreeId,
    room: &IncompleteRoom,
    net_no: i32,
    ignore_object: Option<&TreeObject>,
    ignore_shape: Option<&TileShape>,
) -> Vec<IncompleteRoom> {
    let t = board.search_trees.get(tree);
    match t.kind {
        TreeKind::Default => complete_shape_any(board, t, room, net_no, ignore_object, ignore_shape),
        TreeKind::Degree45 => complete_shape_45(board, t, room, net_no, ignore_object, ignore_shape),
        TreeKind::Degree90 => complete_shape_90(board, t, room, net_no, ignore_object, ignore_shape),
    }
}

#[inline]
fn is_trace_obstacle(board: &Board, t: &ShapeSearchTree, obj: &TreeObject, net_no: i32) -> bool {
    t.object_is_trace_obstacle(obj, net_no, &board.lookup())
}

// -------------------------------------------------------------------- any angle

fn complete_shape_any(
    board: &Board,
    t: &ShapeSearchTree,
    room: &IncompleteRoom,
    net_no: i32,
    ignore_object: Option<&TreeObject>,
    ignore_shape: Option<&TileShape>,
) -> Vec<IncompleteRoom> {
    let Some(contained) = &room.contained else {
        log::warn!("ShapeSearchTree.completeShape: shapeToBeContained != null expected");
        return Vec::new();
    };
    let raw = t.raw();
    if raw.root() == crate::datastructures::shape_tree::NONE {
        return Vec::new();
    }
    let mut start_shape = TileShape::Box(board.bounding_box);
    if let Some(s) = &room.shape {
        start_shape = start_shape.intersection(s);
    }
    let dirs = t.bounding_directions();
    let bounding_shape = start_shape.bounding_shape(dirs);
    let mut result: Vec<IncompleteRoom> = Vec::new();
    if start_shape.dimension() == 2 {
        result.push(IncompleteRoom {
            shape: Some(start_shape.clone()),
            layer: room.layer,
            contained: Some(contained.clone()),
        });
    }
    let mut leaves: Vec<NodeIdx> = Vec::new();
    let mut stack: Vec<NodeIdx> = vec![raw.root()];
    while let Some(n) = stack.pop() {
        if raw.bounds(n).intersects(&bounding_shape) {
            match raw.children(n) {
                None => leaves.push(n),
                Some((a, b)) => {
                    stack.push(a);
                    stack.push(b);
                }
            }
        }
    }
    let room_layer = room.layer;
    leaves.retain(|l| raw.leaf(*l).layer == room_layer);
    leaves.sort_unstable_by(|a, b| {
        let la = raw.leaf(*a);
        let lb = raw.leaf(*b);
        la.object.cmp(&lb.object).then(la.shape_index.cmp(&lb.shape_index))
    });
    for l in leaves {
        let leaf = raw.leaf(l);
        if !(leaf.layer == room_layer
            && Some(&leaf.object) != ignore_object
            && is_trace_obstacle(board, t, &leaf.object, net_no))
        {
            continue;
        }
        let obstacle_shape = &leaf.shape;
        let mut new_result = Vec::new();
        for current in result {
            let mut something_changed = false;
            let intersection = current.shape.as_ref().unwrap().intersection(obstacle_shape);
            if intersection.dimension() == 2 {
                let ignore_room = matches!(leaf.object, TreeObject::Room(..))
                    && ignore_shape.map(|s| s.contains_tile(&intersection)).unwrap_or(false);
                if !ignore_room {
                    something_changed = true;
                    new_result.extend(restrain_shape_any(&current, obstacle_shape));
                }
            }
            if !something_changed {
                new_result.push(current);
            }
        }
        result = new_result;
    }
    divide_large_room(result, &board.bounding_box)
}

fn restrain_shape_any(room: &IncompleteRoom, obstacle_shape: &TileShape) -> Vec<IncompleteRoom> {
    let mut result = Vec::new();
    let obstacle_simplex = TileShape::Simplex(obstacle_shape.to_simplex());
    let contained = room.contained.as_ref().map(|c| TileShape::Simplex(c.to_simplex()));
    let room_shape = room.shape.as_ref();
    let Some(contained) = contained.filter(|c| !c.is_empty()) else {
        return result;
    };
    let layer = room.layer;
    let mut cut_line: Option<Line> = None;
    let mut cut_line_distance = -1.0;
    let n = obstacle_simplex.border_line_count();
    for i in 0..n {
        let seg = LineSegment::from_tile(&obstacle_simplex, i);
        if room_shape.map(|r| r.is_intersected_interior_by(&seg)).unwrap_or(false) {
            let current_line = obstacle_simplex.border_line(i);
            let d = contained.distance_to_the_left(&current_line);
            if d > cut_line_distance {
                cut_line_distance = d;
                cut_line = Some(current_line.opposite());
            }
        }
    }
    if let Some(cut) = cut_line {
        let mut piece = TileShape::from_line(&cut);
        if let Some(r) = room_shape {
            piece = r.intersection(&piece);
        }
        if piece.dimension() >= 2 {
            result.push(IncompleteRoom {
                shape: Some(piece),
                layer,
                contained: Some(contained),
            });
        }
    } else {
        if contained.dimension() < 1 {
            return result;
        }
        let mut cut_line = None;
        for i in 0..n {
            let seg = LineSegment::from_tile(&obstacle_simplex, i);
            if room_shape.map(|r| r.is_intersected_interior_by(&seg)).unwrap_or(false) {
                let current_line = obstacle_simplex.border_line(i);
                if contained.side_of(&current_line) == Side::Collinear {
                    cut_line = Some(current_line.opposite());
                    break;
                }
            }
        }
        let Some(cut) = cut_line else {
            return result;
        };
        let cut_half_plane = TileShape::from_line(&cut);
        let new_contained = contained.intersection(&cut_half_plane);
        let piece = match room_shape {
            None => cut_half_plane.clone(),
            Some(r) => r.intersection(&cut_half_plane),
        };
        if piece.dimension() >= 2 {
            result.push(IncompleteRoom {
                shape: Some(piece),
                layer,
                contained: Some(new_contained),
            });
        }
        let opposite_half_plane = TileShape::from_line(&cut.opposite());
        let rest_piece = match room_shape {
            None => opposite_half_plane.clone(),
            Some(r) => r.intersection(&opposite_half_plane),
        };
        if rest_piece.dimension() >= 2 {
            let rest_contained = contained.intersection(&opposite_half_plane);
            let rest = IncompleteRoom {
                shape: Some(rest_piece),
                layer,
                contained: Some(rest_contained),
            };
            result.extend(restrain_shape_any(&rest, obstacle_shape));
        }
    }
    result
}

/// `divideLargeRoom` of the general tree.
fn divide_large_room(rooms: Vec<IncompleteRoom>, board_box: &IntBox) -> Vec<IncompleteRoom> {
    if rooms.len() != 1 {
        return rooms;
    }
    let current = &rooms[0];
    let room_box = current.shape.as_ref().unwrap().bounding_box();
    if 2 * room_box.height() <= board_box.height() || 2 * room_box.width() <= board_box.width() {
        return rooms;
    }
    let max_section_width = 0.5 * (board_box.height().max(board_box.width())) as f64;
    let sections = current.shape.as_ref().unwrap().divide_into_sections(max_section_width);
    let mut result = Vec::with_capacity(sections.len());
    for s in sections {
        let c = current.contained.as_ref().map(|c| s.intersection(c));
        result.push(IncompleteRoom {
            shape: Some(s),
            layer: current.layer,
            contained: c,
        });
    }
    result
}

// -------------------------------------------------------------------- 45 degree

fn octagon_of(s: &TileShape) -> IntOctagon {
    s.bounding_octagon()
}

fn complete_shape_45(
    board: &Board,
    t: &ShapeSearchTree,
    room: &IncompleteRoom,
    net_no: i32,
    ignore_object: Option<&TreeObject>,
    ignore_shape: Option<&TileShape>,
) -> Vec<IncompleteRoom> {
    let Some(contained_raw) = &room.contained else {
        log::warn!("ShapeSearchTree45Degree.completeShape: contained shape is null, skipping expansion room");
        return Vec::new();
    };
    let shape_to_be_contained = octagon_of(contained_raw);
    let raw = t.raw();
    if raw.root() == crate::datastructures::shape_tree::NONE {
        return Vec::new();
    }
    let mut start_shape = board.bounding_box.to_int_octagon();
    if let Some(s) = &room.shape {
        if !matches!(s, TileShape::Octagon(_)) {
            log::warn!("ShapeSearchTree45Degree.complete_shape: startShape of type IntOctagon expected");
            return Vec::new();
        }
        start_shape = s.bounding_octagon().intersection(&start_shape);
    }
    let mut bounding_shape = start_shape;
    let room_layer = room.layer;
    let mut cs_log: Option<String> = None;
    if crate::board::snapshot::evlog::enabled() && std::env::var_os("RR_CSLOG").is_some() {
        let cb = contained_raw.bounding_box();
        cs_log = Some(format!(
            "CS L{} c={},{},{},{} s={},{},{},{}:",
            room.layer, cb.ll.x, cb.ll.y, cb.ur.x, cb.ur.y, start_shape.lx, start_shape.ly, start_shape.rx, start_shape.uy
        ));
    }
    let mut result: Vec<(IntOctagon, TileShape)> = vec![(start_shape, TileShape::Octagon(shape_to_be_contained))];
    let mut stack: Vec<NodeIdx> = vec![raw.root()];
    while let Some(n) = stack.pop() {
        let bs = RegularTileShape::Octagon(bounding_shape);
        if !raw.bounds(n).intersects(&bs) {
            continue;
        }
        match raw.children(n) {
            None => {
                let leaf = raw.leaf(n);
                if let Some(l) = &mut cs_log {
                    if std::env::var_os("RR_CSALL").is_some() {
                        let o = is_trace_obstacle(board, t, &leaf.object, net_no);
                        let sl = leaf.layer == room_layer;
                        match leaf.object {
                            TreeObject::Item(i) => l.push_str(&format!(" vi{}.{}", i.0, leaf.shape_index)),
                            TreeObject::Room(_, id) => l.push_str(&format!(" vr{}.{}", id, leaf.shape_index)),
                        }
                        l.push_str(if o { "o" } else { "n" });
                        l.push_str(if sl { "s" } else { "d" });
                    }
                }
                if leaf.layer == room_layer
                    && Some(&leaf.object) != ignore_object
                    && is_trace_obstacle(board, t, &leaf.object, net_no)
                {
                    let obstacle = octagon_of(&leaf.shape);
                    if let Some(l) = &mut cs_log {
                        match leaf.object {
                            TreeObject::Item(i) => l.push_str(&format!(" i{}.{}", i.0, leaf.shape_index)),
                            TreeObject::Room(_, id) => l.push_str(&format!(" r{}.{}", id, leaf.shape_index)),
                        }
                        l.push_str(&format!("[{},{},{},{}]", obstacle.lx, obstacle.ly, obstacle.rx, obstacle.uy));
                    }
                    if !result.iter().any(|(s, _)| s.overlaps(&obstacle)) {
                        continue;
                    }
                    let mut new_result = Vec::new();
                    let mut new_bounding = IntOctagon::EMPTY;
                    for (current_shape, current_contained) in result {
                        if current_shape.overlaps(&obstacle) {
                            if matches!(leaf.object, TreeObject::Room(..)) {
                                if let Some(is) = ignore_shape {
                                    let intersection = current_shape.intersection(&obstacle);
                                    if is.contains_tile(&TileShape::Octagon(intersection)) {
                                        if !is.contains_tile(&TileShape::Octagon(current_shape)) {
                                            new_bounding = new_bounding.union(&current_shape);
                                            new_result.push((current_shape, current_contained));
                                        }
                                        continue;
                                    }
                                }
                            }
                            let restrained = restrain_shape_45(&current_shape, &current_contained, &obstacle);
                            for (s, _) in &restrained {
                                new_bounding = new_bounding.union(s);
                            }
                            new_result.extend(restrained);
                        } else {
                            new_bounding = new_bounding.union(&current_shape);
                            new_result.push((current_shape, current_contained));
                        }
                    }
                    result = new_result;
                    if result.is_empty() {
                        break;
                    }
                    bounding_shape = new_bounding;
                    if let Some(l) = &mut cs_log {
                        let b = &bounding_shape;
                        l.push_str(&format!("=>{}{{{},{},{},{},{},{},{},{}}}", result.len(), b.lx, b.ly, b.rx, b.uy, b.ulx, b.lrx, b.llx, b.urx));
                    }
                }
            }
            Some((a, b)) => {
                let bs = RegularTileShape::Octagon(bounding_shape);
                if raw.bounds(a).intersects(&bs) {
                    stack.push(a);
                }
                if raw.bounds(b).intersects(&bs) {
                    stack.push(b);
                }
            }
        }
    }
    if let Some(l) = &mut cs_log {
        if std::env::var_os("RR_TREEHASH").is_some() {
            let mut hb = String::new();
            dump(raw, raw.root(), 0, &mut hb);
            let h = hb.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32));
            l.push_str(&format!(" TH{h}"));
            static TH_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let c = TH_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if let Ok(r) = std::env::var("RR_TREEOPS") {
                let (a, b) = r.split_once('-').unwrap();
                let on = c >= a.parse::<usize>().unwrap() && c < b.parse::<usize>().unwrap();
                crate::datastructures::shape_tree::TREEOPS.store(on, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
    if let Some(l) = cs_log {
        if let Some(at) = std::env::var_os("RR_TREEDUMP_AT") {
            static DUMPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            let cb = contained_raw.bounding_box();
            static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let nth: usize = std::env::var("RR_TREEDUMP_N").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
            if format!("{},{}", cb.ll.x, cb.ll.y) == at.to_string_lossy()
                && COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 >= nth
                && !DUMPED.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                let mut sb = String::from("TREE");
                dump(raw, raw.root(), 0, &mut sb);
                crate::board::snapshot::evlog::line(&sb);
            }
        }
        crate::board::snapshot::evlog::line(&format!("{} -> {}", l, result.len()));
    }
    let rooms: Vec<IncompleteRoom> = result
        .into_iter()
        .map(|(s, c)| IncompleteRoom {
            shape: Some(TileShape::Octagon(s)),
            layer: room_layer,
            contained: Some(c),
        })
        .collect();
    let mut rooms = divide_large_room(rooms, &board.bounding_box);
    for r in rooms.iter_mut() {
        r.shape = Some(TileShape::Octagon(r.shape.as_ref().unwrap().bounding_octagon()));
        r.contained = Some(TileShape::Octagon(r.contained.as_ref().unwrap().bounding_octagon()));
    }
    rooms.retain(|r| !r.contained.as_ref().unwrap().contains_tile(r.shape.as_ref().unwrap()));
    rooms
}

fn obstacle_segment_touches_inside(obstacle: &IntOctagon, obstacle_line_no: usize, room: &IntOctagon) -> bool {
    let mut current_line_no = obstacle_line_no;
    let cx = obstacle.corner_x(obstacle_line_no);
    let cy = obstacle.corner_y(obstacle_line_no);
    for _ in 0..5 {
        if room.side_of_border_line(cx, cy, current_line_no) != Side::OnTheLeft {
            return false;
        }
        current_line_no = (current_line_no + 1) % 8;
    }
    let next_no = (obstacle_line_no + 1) % 8;
    let nx = obstacle.corner_x(next_no);
    let ny = obstacle.corner_y(next_no);
    current_line_no = (obstacle_line_no + 5) % 8;
    for _ in 0..3 {
        if room.side_of_border_line(nx, ny, current_line_no) != Side::OnTheLeft {
            return false;
        }
        current_line_no = (current_line_no + 1) % 8;
    }
    true
}

fn signed_line_distance(o: &IntOctagon, line_no: usize, c: &IntOctagon) -> f64 {
    match line_no {
        0 => (o.ly - c.uy) as f64,
        2 => (c.lx - o.rx) as f64,
        4 => (c.ly - o.uy) as f64,
        6 => (o.lx - c.rx) as f64,
        1 => 0.5 * (c.ulx - o.lrx) as f64,
        3 => 0.5 * (c.llx - o.urx) as f64,
        5 => 0.5 * (o.ulx - c.lrx) as f64,
        7 => 0.5 * (o.llx - c.urx) as f64,
        _ => 0.0,
    }
}

fn restrain_shape_45(room_shape: &IntOctagon, contained_shape: &TileShape, obstacle: &IntOctagon) -> Vec<(IntOctagon, TileShape)> {
    let mut result = Vec::new();
    if contained_shape.is_empty() {
        return result;
    }
    let shape_to_be_contained = match contained_shape {
        TileShape::Octagon(o) => *o,
        TileShape::Box(b) => b.to_int_octagon(),
        TileShape::Simplex(_) => contained_shape.bounding_octagon(),
    };
    let mut cut_line_distance = -1.0;
    let mut restraining_line_no: i32 = -1;
    for no in 0..8 {
        let d = signed_line_distance(obstacle, no, &shape_to_be_contained);
        if d > cut_line_distance && obstacle_segment_touches_inside(obstacle, no, room_shape) {
            cut_line_distance = d;
            restraining_line_no = no as i32;
        }
    }
    if cut_line_distance >= 0.0 {
        let restrained = calc_outside_restrained_shape(obstacle, restraining_line_no as usize, room_shape);
        result.push((restrained, TileShape::Octagon(shape_to_be_contained)));
        return result;
    }
    if shape_to_be_contained.dimension() < 1 {
        return result;
    }
    restraining_line_no = -1;
    for no in 0..8 {
        if obstacle_segment_touches_inside(obstacle, no, room_shape) {
            let line = obstacle.border_line(no);
            if TileShape::Octagon(shape_to_be_contained).side_of(&line) == Side::Collinear {
                restraining_line_no = no as i32;
                break;
            }
        }
    }
    if restraining_line_no < 0 {
        return result;
    }
    let no = restraining_line_no as usize;
    let restrained = calc_outside_restrained_shape(obstacle, no, room_shape);
    if restrained.dimension() == 2 {
        let new_contained = shape_to_be_contained.intersection(&restrained);
        if new_contained.dimension() > 0 {
            result.push((restrained, TileShape::Octagon(new_contained)));
        }
    }
    let rest_piece = calc_inside_restrained_shape(obstacle, no, room_shape);
    if rest_piece.dimension() >= 2 {
        let rest_contained = shape_to_be_contained.intersection(&rest_piece);
        if rest_contained.dimension() >= 0 {
            result.extend(restrain_shape_45(&rest_piece, &TileShape::Octagon(rest_contained), obstacle));
        }
    }
    result
}

fn calc_outside_restrained_shape(o: &IntOctagon, line_no: usize, r: &IntOctagon) -> IntOctagon {
    let mut s = *r;
    match line_no {
        0 => s.uy = o.ly,
        2 => s.lx = o.rx,
        4 => s.ly = o.uy,
        6 => s.rx = o.lx,
        1 => s.ulx = o.lrx,
        3 => s.llx = o.urx,
        5 => s.lrx = o.ulx,
        7 => s.urx = o.llx,
        _ => {}
    }
    s.normalize()
}

fn calc_inside_restrained_shape(o: &IntOctagon, line_no: usize, r: &IntOctagon) -> IntOctagon {
    let mut s = *r;
    match line_no {
        0 => s.ly = o.ly,
        2 => s.rx = o.rx,
        4 => s.uy = o.uy,
        6 => s.lx = o.lx,
        1 => s.lrx = o.lrx,
        3 => s.urx = o.urx,
        5 => s.ulx = o.ulx,
        7 => s.llx = o.llx,
        _ => {}
    }
    s.normalize()
}

// -------------------------------------------------------------------- 90 degree

fn complete_shape_90(
    board: &Board,
    t: &ShapeSearchTree,
    room: &IncompleteRoom,
    net_no: i32,
    ignore_object: Option<&TreeObject>,
    ignore_shape: Option<&TileShape>,
) -> Vec<IncompleteRoom> {
    let Some(TileShape::Box(shape_to_be_contained)) = &room.contained else {
        log::warn!("BoxShapeSearchTree.complete_shape: unexpected shapeToBeContained");
        return Vec::new();
    };
    let raw = t.raw();
    if raw.root() == crate::datastructures::shape_tree::NONE {
        return Vec::new();
    }
    let mut start_shape = board.bounding_box;
    if let Some(s) = &room.shape {
        let TileShape::Box(b) = s else {
            log::warn!("BoxShapeSearchTree.complete_shape: startShape of type IntBox expected");
            return Vec::new();
        };
        start_shape = b.intersection(&start_shape);
    }
    let mut bounding_shape = start_shape;
    let room_layer = room.layer;
    let mut result: Vec<(IntBox, IntBox)> = vec![(start_shape, *shape_to_be_contained)];
    let mut stack: Vec<NodeIdx> = vec![raw.root()];
    while let Some(n) = stack.pop() {
        if !raw.bounds(n).intersects(&RegularTileShape::Box(bounding_shape)) {
            continue;
        }
        match raw.children(n) {
            None => {
                let leaf = raw.leaf(n);
                if leaf.layer == room_layer
                    && Some(&leaf.object) != ignore_object
                    && is_trace_obstacle(board, t, &leaf.object, net_no)
                {
                    let obstacle = leaf.shape.bounding_box();
                    let mut new_result: Vec<(IntBox, IntBox)> = Vec::new();
                    let mut new_bounding = IntBox::EMPTY;
                    for (current_shape, current_contained) in result {
                        if current_shape.overlaps(&obstacle) {
                            if matches!(leaf.object, TreeObject::Room(..)) {
                                if let Some(is) = ignore_shape {
                                    let intersection = current_shape.intersection(&obstacle);
                                    if is.contains_tile(&TileShape::Box(intersection)) {
                                        continue;
                                    }
                                }
                            }
                            let restrained = restrain_shape_90(&current_shape, &current_contained, &obstacle);
                            new_result.extend(restrained);
                            for (s, _) in &new_result {
                                new_bounding = new_bounding.union(s);
                            }
                        } else {
                            new_bounding = new_bounding.union(&current_shape);
                            new_result.push((current_shape, current_contained));
                        }
                    }
                    result = new_result;
                    bounding_shape = new_bounding;
                }
            }
            Some((a, b)) => {
                stack.push(a);
                stack.push(b);
            }
        }
    }
    result
        .into_iter()
        .map(|(s, c)| IncompleteRoom {
            shape: Some(TileShape::Box(s)),
            layer: room_layer,
            contained: Some(TileShape::Box(c)),
        })
        .collect()
}

fn restrain_shape_90(room_shape: &IntBox, contained: &IntBox, obstacle: &IntBox) -> Vec<(IntBox, IntBox)> {
    let mut result = Vec::new();
    if contained.is_empty() {
        return result;
    }
    let r = room_shape;
    let c = contained;
    let o = obstacle;
    let mut cut_line_distance = 0;
    let mut restrained: Option<IntBox> = None;
    if r.ll.x < o.ur.x && r.ur.x > o.ur.x && r.ur.y > o.ll.y && r.ll.y < o.ur.y {
        let d = c.ll.x - o.ur.x;
        if d > cut_line_distance {
            cut_line_distance = d;
            restrained = Some(IntBox::new(o.ur.x, r.ll.y, r.ur.x, r.ur.y));
        }
    }
    if r.ll.x < o.ll.x && r.ur.x > o.ll.x && r.ur.y > o.ll.y && r.ll.y < o.ur.y {
        let d = o.ll.x - c.ur.x;
        if d > cut_line_distance {
            cut_line_distance = d;
            restrained = Some(IntBox::new(r.ll.x, r.ll.y, o.ll.x, r.ur.y));
        }
    }
    if r.ll.y < o.ll.y && r.ur.y > o.ll.y && r.ur.x > o.ll.x && r.ll.x < o.ur.x {
        let d = o.ll.y - c.ur.y;
        if d > cut_line_distance {
            cut_line_distance = d;
            restrained = Some(IntBox::new(r.ll.x, r.ll.y, r.ur.x, o.ll.y));
        }
    }
    if r.ll.y < o.ur.y && r.ur.y > o.ur.y && r.ur.x > o.ll.x && r.ll.x < o.ur.x {
        let d = c.ll.y - o.ur.y;
        if d > cut_line_distance {
            restrained = Some(IntBox::new(r.ll.x, o.ur.y, r.ur.x, r.ur.y));
        }
    }
    if let Some(s) = restrained {
        result.push((s, *c));
        return result;
    }
    let is = c.intersection(o);
    if is.is_empty() {
        log::warn!("BoxShapeSearchTree.restrain_shape: Intersection between obstacleShape and shapeToBeContained expected");
        return result;
    }
    let mut new1 = None;
    let mut new2 = None;
    if is.ll.x > r.ll.x && is.ll.x == o.ll.x && is.ll.x < r.ur.x {
        new1 = Some(IntBox::new(r.ll.x, r.ll.y, is.ll.x, r.ur.y));
        new2 = Some(IntBox::new(is.ll.x, r.ll.y, r.ur.x, r.ur.y));
    } else if is.ur.x > r.ll.x && is.ur.x == o.ur.x && is.ur.x < r.ur.x {
        new2 = Some(IntBox::new(r.ll.x, r.ll.y, is.ur.x, r.ur.y));
        new1 = Some(IntBox::new(is.ur.x, r.ll.y, r.ur.x, r.ur.y));
    } else if is.ll.y > r.ll.y && is.ll.y == o.ll.y && is.ll.y < r.ur.y {
        new1 = Some(IntBox::new(r.ll.x, r.ll.y, r.ur.x, is.ll.y));
        new2 = Some(IntBox::new(r.ll.x, is.ll.y, r.ur.x, r.ur.y));
    } else if is.ur.y > r.ll.y && is.ur.y == o.ur.y && is.ur.y < r.ur.y {
        new2 = Some(IntBox::new(r.ll.x, r.ll.y, r.ur.x, is.ur.y));
        new1 = Some(IntBox::new(r.ll.x, is.ur.y, r.ur.x, r.ur.y));
    }
    if let (Some(s1), Some(s2)) = (new1, new2) {
        let nc = c.intersection(&s1);
        if nc.dimension() > 0 {
            result.push((s1, nc));
            let c2 = c.intersection(&s2);
            result.extend(restrain_shape_90(&s2, &c2, o));
        }
    }
    result
}

fn dump(raw: &crate::datastructures::shape_tree::MinAreaTree<crate::board::search_tree::TreeLeaf>, n: NodeIdx, depth: usize, sb: &mut String) {
    let bo = raw.bounds(n).to_octagon();
    sb.push_str(&format!(" {{{},{},{},{},{},{},{},{}}}", bo.lx, bo.ly, bo.rx, bo.uy, bo.ulx, bo.lrx, bo.llx, bo.urx));
    match raw.children(n) {
        None => {
            let leaf = raw.leaf(n);
            match leaf.object {
                TreeObject::Item(i) => sb.push_str(&format!(" {}:i{}.{}", depth, i.0, leaf.shape_index)),
                TreeObject::Room(_, id) => sb.push_str(&format!(" {}:r{}.{}", depth, id, leaf.shape_index)),
            }
        }
        Some((a, b)) => {
            dump(raw, a, depth + 1, sb);
            dump(raw, b, depth + 1, sb);
        }
    }
}
