//! The autoroute engine with its expansion room database
//! (port of `AutorouteEngine`, the expansion rooms and doors, `ItemAutorouteInfo`,
//! `ExpansionDrill`, `DrillPage` and `DrillPageArray`).
//!
//! The Java object graph of rooms and doors is stored in arenas addressed by keys.
//! Slots are never reused during the lifetime of an engine, so stale references held
//! by drills or doors behave like the dangling-but-alive objects of the original.

use super::connection::Connection;
use crate::board::{Board, ItemId, TreeId, TreeKind, TreeObject};
use crate::datastructures::{StopFlag, TimeLimit};
use crate::geometry::{FloatLine, FloatPoint, IntBox, Line, Point, PolylineArea, Shape, Simplex, TileShape};
use rustc_hash::FxHashMap;

pub const TRACE_WIDTH_TOLERANCE: i32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RoomKey(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DoorKey(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TargetKey(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DrillKey(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageKey(pub u32);

/// `ExpandableObject` of the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Expandable {
    Door(DoorKey),
    Target(TargetKey),
    Drill(DrillKey),
    Page(PageKey),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Adjustment {
    #[default]
    None,
    Right,
    Left,
}

/// `MazeSearchElement`: the search state of a door section.
#[derive(Clone, Debug, Default)]
pub struct MazeSearchElement {
    pub is_occupied: bool,
    pub backtrack_door: Option<Expandable>,
    pub section_no_of_backtrack_door: usize,
    pub room_ripped: bool,
    pub adjustment: Adjustment,
    pub ripup_cost: i32,
}

impl MazeSearchElement {
    pub fn reset(&mut self) {
        *self = MazeSearchElement::default();
    }
}

#[derive(Clone, Debug)]
pub enum RoomKind {
    /// `IncompleteFreeSpaceExpansionRoom`
    Incomplete { contained: Option<TileShape> },
    /// `CompleteFreeSpaceExpansionRoom`
    Complete {
        id: i32,
        target_doors: Vec<TargetKey>,
        net_dependent: bool,
    },
    /// `ObstacleExpansionRoom`
    Obstacle {
        item: ItemId,
        index: usize,
        doors_calculated: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Room {
    pub kind: RoomKind,
    pub shape: Option<TileShape>,
    pub layer: i32,
    pub doors: Vec<DoorKey>,
}

impl Room {
    #[inline]
    pub fn is_incomplete(&self) -> bool {
        matches!(self.kind, RoomKind::Incomplete { .. })
    }

    #[inline]
    pub fn is_complete_free(&self) -> bool {
        matches!(self.kind, RoomKind::Complete { .. })
    }

    #[inline]
    pub fn is_obstacle(&self) -> bool {
        matches!(self.kind, RoomKind::Obstacle { .. })
    }

    /// `CompleteExpansionRoom` (free space or obstacle room).
    #[inline]
    pub fn is_complete(&self) -> bool {
        !self.is_incomplete()
    }

    #[inline]
    pub fn obstacle_item(&self) -> Option<ItemId> {
        match self.kind {
            RoomKind::Obstacle { item, .. } => Some(item),
            _ => None,
        }
    }

    #[inline]
    pub fn obstacle_index(&self) -> usize {
        match self.kind {
            RoomKind::Obstacle { index, .. } => index,
            _ => 0,
        }
    }

    pub fn shape(&self) -> &TileShape {
        self.shape.as_ref().expect("room shape")
    }

    pub fn contained_shape(&self) -> Option<&TileShape> {
        match &self.kind {
            RoomKind::Incomplete { contained } => contained.as_ref(),
            _ => None,
        }
    }

    pub fn target_doors(&self) -> &[TargetKey] {
        match &self.kind {
            RoomKind::Complete { target_doors, .. } => target_doors,
            _ => &[],
        }
    }

    /// `getId` of the room classes.
    pub fn get_id(&self) -> i32 {
        match &self.kind {
            RoomKind::Incomplete { .. } => {
                let sid = self.shape.as_ref().map(|s| s.get_id()).unwrap_or(0);
                31i32.wrapping_mul(sid).wrapping_add(self.layer)
            }
            RoomKind::Complete { id, .. } => *id,
            RoomKind::Obstacle { item, index, .. } => (item.0 << 10) | (*index as i32),
        }
    }

    pub fn tree_object(&self, key: RoomKey) -> Option<TreeObject> {
        match &self.kind {
            RoomKind::Complete { id, .. } => Some(TreeObject::Room(crate::board::RoomId(key.0), *id)),
            RoomKind::Obstacle { item, .. } => Some(TreeObject::Item(*item)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Door {
    pub first: RoomKey,
    pub second: RoomKey,
    pub dimension: i32,
    pub sections: Vec<MazeSearchElement>,
}

#[derive(Clone, Debug)]
pub struct TargetDoor {
    pub item: ItemId,
    pub tree_entry_no: usize,
    pub room: Option<RoomKey>,
    pub shape: TileShape,
    pub elem: MazeSearchElement,
}

#[derive(Clone, Debug)]
pub struct Drill {
    pub shape: TileShape,
    pub location: Point,
    pub first_layer: i32,
    pub last_layer: i32,
    pub rooms: Vec<Option<RoomKey>>,
    pub elems: Vec<MazeSearchElement>,
}

impl Drill {
    pub fn new(shape: TileShape, location: Point, first_layer: i32, last_layer: i32) -> Drill {
        let n = (last_layer - first_layer + 1).max(0) as usize;
        Drill {
            shape,
            location,
            first_layer,
            last_layer,
            rooms: vec![None; n],
            elems: vec![MazeSearchElement::default(); n],
        }
    }

    pub fn get_id(&self) -> i32 {
        31i32
            .wrapping_mul(31i32.wrapping_mul(self.location.get_id()).wrapping_add(self.first_layer))
            .wrapping_add(self.last_layer)
    }
}

#[derive(Clone, Debug)]
pub struct DrillPage {
    pub shape: IntBox,
    pub elems: Vec<MazeSearchElement>,
    pub drills: Option<Vec<DrillKey>>,
    pub net_no: i32,
}

#[derive(Clone, Debug)]
pub struct DrillPageArray {
    pub bounds: IntBox,
    pub column_count: i32,
    pub row_count: i32,
    pub page_width: i32,
    pub page_height: i32,
    pub pages: Vec<DrillPage>,
}

impl DrillPageArray {
    pub fn new(bounds: IntBox, layer_count: usize, max_page_width: i32) -> DrillPageArray {
        let length = (bounds.ur.x - bounds.ll.x) as f64;
        let height = (bounds.ur.y - bounds.ll.y) as f64;
        let column_count = (length / max_page_width as f64).ceil() as i32;
        let row_count = (height / max_page_width as f64).ceil() as i32;
        let page_width = (length / column_count as f64).ceil() as i32;
        let page_height = (height / row_count as f64).ceil() as i32;
        let mut pages = Vec::with_capacity((row_count.max(0) * column_count.max(0)) as usize);
        for j in 0..row_count {
            for i in 0..column_count {
                let ll_x = bounds.ll.x + i * page_width;
                let ur_x = if i == column_count - 1 { bounds.ur.x } else { ll_x + page_width };
                let ll_y = bounds.ll.y + j * page_height;
                let ur_y = if j == row_count - 1 { bounds.ur.y } else { ll_y + page_height };
                pages.push(DrillPage {
                    shape: IntBox::new(ll_x, ll_y, ur_x, ur_y),
                    elems: vec![MazeSearchElement::default(); layer_count],
                    drills: None,
                    net_no: -1,
                });
            }
        }
        DrillPageArray {
            bounds,
            column_count,
            row_count,
            page_width,
            page_height,
            pages,
        }
    }

    pub fn overlapping_pages(&self, shape: &TileShape) -> Vec<PageKey> {
        let mut result = Vec::new();
        let shape_box = shape.bounding_box().intersection(&self.bounds);
        let min_j = (((shape_box.ll.y - self.bounds.ll.y) as f64) / self.page_height as f64).floor() as i32;
        let max_j = ((shape_box.ur.y - self.bounds.ll.y) as f64) / self.page_height as f64;
        let min_i = (((shape_box.ll.x - self.bounds.ll.x) as f64) / self.page_width as f64).floor() as i32;
        let max_i = ((shape_box.ur.x - self.bounds.ll.x) as f64) / self.page_width as f64;
        let mut j = min_j;
        while (j as f64) < max_j {
            let mut i = min_i;
            while (i as f64) < max_i {
                let idx = (j * self.column_count + i) as usize;
                let page = &self.pages[idx];
                let intersection = shape.intersection(&TileShape::Box(page.shape));
                if intersection.dimension() > 1 {
                    result.push(PageKey(idx as u32));
                }
                i += 1;
            }
            j += 1;
        }
        result
    }

    pub fn page_id(&self, key: PageKey) -> i32 {
        let p = &self.pages[key.0 as usize];
        31i32.wrapping_mul(p.shape.get_id()).wrapping_add(p.net_no)
    }
}

/// `ItemAutorouteInfo`: temporary autoroute data of an item.
#[derive(Clone, Debug, Default)]
pub struct ItemAutorouteInfo {
    pub start_info: bool,
    pub precalculated_connection: Option<Connection>,
    pub rooms: Vec<Option<RoomKey>>,
    /// `Via.autorouteDrillInfo`
    pub drill: Option<DrillKey>,
}

/// The autoroute engine (`AutorouteEngine`) with its room database.
#[derive(Clone, Debug)]
pub struct AutorouteEngine {
    pub tree: TreeId,
    pub maintain_database: bool,
    pub net_no: i32,
    pub stop: Option<StopFlag>,
    pub time_limit: Option<TimeLimit>,
    pub rooms: Vec<Room>,
    pub doors: Vec<Door>,
    pub targets: Vec<TargetDoor>,
    pub drills: Vec<Drill>,
    pub drill_pages: DrillPageArray,
    incomplete_rooms: Option<Vec<RoomKey>>,
    complete_rooms: Option<Vec<RoomKey>>,
    room_instance_count: i32,
    pub item_info: FxHashMap<ItemId, ItemAutorouteInfo>,
}

/// An incomplete room not (yet) stored in the engine, as returned by `completeShape`.
#[derive(Clone, Debug)]
pub struct IncompleteRoom {
    pub shape: Option<TileShape>,
    pub layer: i32,
    pub contained: Option<TileShape>,
}

impl AutorouteEngine {
    pub fn new(board: &mut Board, trace_clearance_class: i32, maintain_database: bool) -> AutorouteEngine {
        let tree = board.get_autoroute_tree(trace_clearance_class);
        let mut max_drill_page_width = (5.0 * board.rules.get_default_via_diameter(&board.library.padstacks)) as i32;
        max_drill_page_width = max_drill_page_width.max(10000);
        AutorouteEngine {
            tree,
            maintain_database,
            net_no: -1,
            stop: None,
            time_limit: None,
            rooms: Vec::new(),
            doors: Vec::new(),
            targets: Vec::new(),
            drills: Vec::new(),
            drill_pages: DrillPageArray::new(board.bounding_box, board.layer_count() as usize, max_drill_page_width),
            incomplete_rooms: None,
            complete_rooms: None,
            room_instance_count: 0,
            item_info: FxHashMap::default(),
        }
    }

    pub fn tree_kind(&self, board: &Board) -> TreeKind {
        board.search_trees.get(self.tree).kind
    }

    pub fn compensated_clearance_class(&self, board: &Board) -> i32 {
        board.search_trees.get(self.tree).compensated_clearance_class
    }

    pub fn init_connection(&mut self, board: &mut Board, net_no: i32, stop: Option<StopFlag>, time_limit: Option<TimeLimit>) {
        if self.maintain_database && net_no != self.net_no {
            if let Some(rooms) = &self.complete_rooms {
                let to_remove: Vec<RoomKey> = rooms
                    .iter()
                    .copied()
                    .filter(|r| matches!(self.rooms[r.0 as usize].kind, RoomKind::Complete { net_dependent: true, .. }))
                    .collect();
                for r in to_remove {
                    self.remove_complete_expansion_room(board, r);
                }
            }
            let ids: Vec<ItemId> = board.items.ids();
            for id in ids {
                if board.item(id).contains_net(net_no) {
                    self.update_after_item_change(board, id);
                }
            }
        }
        self.net_no = net_no;
        self.stop = stop;
        self.time_limit = time_limit;
    }

    pub fn is_stop_requested(&self) -> bool {
        if let Some(t) = &self.time_limit {
            if t.limit_exceeded() {
                return true;
            }
        }
        match &self.stop {
            Some(s) => s.is_stop_requested(),
            None => false,
        }
    }

    /// `clear`: removes the complete rooms from the tree and the item infos.
    pub fn clear(&mut self, board: &mut Board) {
        if let Some(rooms) = self.complete_rooms.take() {
            let tree = board.search_trees.get_mut(self.tree);
            for r in rooms {
                if let Some(obj) = self.rooms[r.0 as usize].tree_object(r) {
                    tree.remove_object(&obj);
                }
            }
        }
        self.incomplete_rooms = None;
        self.room_instance_count = 0;
        self.item_info.clear();
    }

    /// `RoutingBoard.additionalUpdateAfterChange` for a maintained database.
    pub fn update_after_item_change(&mut self, board: &mut Board, id: ItemId) {
        if !self.maintain_database {
            return;
        }
        let obj = TreeObject::Item(id);
        let tree = board.search_trees.get(self.tree);
        let count = tree.object_shape_count(&obj);
        let mut shapes = Vec::new();
        if count > 0 {
            for i in 0..count {
                if let Some(entries) = tree.entries_of(&obj) {
                    let l = entries[i];
                    if l != crate::datastructures::shape_tree::NONE {
                        let leaf = tree.leaf(l);
                        shapes.push((leaf.shape.clone(), leaf.layer));
                    }
                }
            }
        } else if let Some(item) = board.try_item(id) {
            let (s, layers) = tree.calculate_tree_shapes(item, &board.tree_env());
            for (sh, l) in s.into_iter().zip(layers) {
                if let Some(sh) = sh {
                    shapes.push((sh, l));
                }
            }
        }
        for (shape, layer) in shapes {
            self.invalidate_drill_pages(&shape);
            let overlaps = board
                .search_trees
                .get(self.tree)
                .overlapping_objects(&shape, layer, &board.lookup());
            for o in overlaps {
                if let TreeObject::Room(r, _) = o {
                    let key = RoomKey(r.0);
                    if self.complete_rooms.as_ref().map(|c| c.contains(&key)).unwrap_or(false) {
                        self.remove_complete_expansion_room(board, key);
                    }
                }
            }
        }
        self.item_info.remove(&id);
    }

    pub fn generate_room_id_no(&mut self) -> i32 {
        self.room_instance_count += 1;
        if crate::board::snapshot::evlog::maze_log_enabled() {
            crate::board::snapshot::evlog::line(&format!("ROOMID {}", self.room_instance_count));
        }
        self.room_instance_count
    }

    // ---------------------------------------------------------------- arena helpers

    #[inline]
    pub fn room(&self, r: RoomKey) -> &Room {
        &self.rooms[r.0 as usize]
    }

    #[inline]
    pub fn room_mut(&mut self, r: RoomKey) -> &mut Room {
        &mut self.rooms[r.0 as usize]
    }

    #[inline]
    pub fn door(&self, d: DoorKey) -> &Door {
        &self.doors[d.0 as usize]
    }

    #[inline]
    pub fn door_mut(&mut self, d: DoorKey) -> &mut Door {
        &mut self.doors[d.0 as usize]
    }

    #[inline]
    pub fn target(&self, t: TargetKey) -> &TargetDoor {
        &self.targets[t.0 as usize]
    }

    #[inline]
    pub fn drill(&self, d: DrillKey) -> &Drill {
        &self.drills[d.0 as usize]
    }

    pub fn new_room(&mut self, room: Room) -> RoomKey {
        self.rooms.push(room);
        RoomKey((self.rooms.len() - 1) as u32)
    }

    pub fn new_complete_room(&mut self, shape: TileShape, layer: i32, id: i32) -> RoomKey {
        self.new_room(Room {
            kind: RoomKind::Complete {
                id,
                target_doors: Vec::new(),
                net_dependent: false,
            },
            shape: Some(shape),
            layer,
            doors: Vec::new(),
        })
    }

    /// `new ExpansionDoor(first, second, dimension)`; the door is not yet added to the rooms.
    pub fn new_door(&mut self, first: RoomKey, second: RoomKey, dimension: i32) -> DoorKey {
        self.doors.push(Door {
            first,
            second,
            dimension,
            sections: Vec::new(),
        });
        DoorKey((self.doors.len() - 1) as u32)
    }

    /// `new ExpansionDoor(first, second)`: the dimension is calculated from the room shapes.
    pub fn new_door_auto(&mut self, first: RoomKey, second: RoomKey) -> DoorKey {
        let dim = self.room(first).shape().intersection(self.room(second).shape()).dimension();
        self.new_door(first, second, dim)
    }

    /// Creates a door and adds it to both rooms (`first.addDoor; second.addDoor` order given by `add_first_to_second_first`).
    pub fn connect(&mut self, a: RoomKey, b: RoomKey, dimension: Option<i32>) -> DoorKey {
        let d = match dimension {
            Some(dim) => self.new_door(a, b, dim),
            None => self.new_door_auto(a, b),
        };
        d
    }

    pub fn add_door(&mut self, room: RoomKey, door: DoorKey) {
        self.room_mut(room).doors.push(door);
    }

    /// `removeDoor`: removes the first occurrence of the door from the room.
    pub fn remove_door_from_room(&mut self, room: RoomKey, door: DoorKey) -> bool {
        let doors = &mut self.room_mut(room).doors;
        if let Some(p) = doors.iter().position(|d| *d == door) {
            doors.remove(p);
            true
        } else {
            false
        }
    }

    pub fn remove_target_door_from_room(&mut self, room: RoomKey, t: TargetKey) -> bool {
        if let RoomKind::Complete { target_doors, .. } = &mut self.room_mut(room).kind {
            if let Some(p) = target_doors.iter().position(|d| *d == t) {
                target_doors.remove(p);
                return true;
            }
        }
        false
    }

    /// `ExpansionDoor.otherRoom(room)`
    #[inline]
    pub fn door_other_room(&self, door: DoorKey, room: RoomKey) -> Option<RoomKey> {
        let d = self.door(door);
        if room == d.first {
            Some(d.second)
        } else if room == d.second {
            Some(d.first)
        } else {
            None
        }
    }

    /// `ExpandableObject.otherRoom(CompleteExpansionRoom)`: only doors have a complete other room.
    pub fn expandable_other_room(&self, e: Expandable, room: Option<RoomKey>) -> Option<RoomKey> {
        match (e, room) {
            (Expandable::Door(d), Some(r)) => {
                let o = self.door_other_room(d, r)?;
                if self.room(o).is_complete() {
                    Some(o)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// `doorExists(other)`
    pub fn door_exists(&self, room: RoomKey, other: RoomKey) -> bool {
        self.room(room).doors.iter().any(|d| {
            let door = self.door(*d);
            door.first == other || door.second == other
        })
    }

    /// The shape of a door (intersection of the room shapes).
    pub fn door_shape(&self, door: DoorKey) -> TileShape {
        let d = self.door(door);
        self.room(d.first).shape().intersection(self.room(d.second).shape())
    }

    pub fn door_id(&self, door: DoorKey) -> i32 {
        let d = self.door(door);
        let id1 = self.room(d.first).get_id();
        let id2 = self.room(d.second).get_id();
        id1.min(id2).wrapping_mul(31).wrapping_add(id1.max(id2))
    }

    /// `ExpansionDoor.getSectionSegments`; allocates the section elements.
    pub fn door_section_segments(&mut self, door: DoorKey, offset_param: f64) -> Vec<FloatLine> {
        let door_shape = self.door_shape(door);
        let offset = offset_param + TRACE_WIDTH_TOLERANCE as f64;
        if door_shape.is_empty() {
            return Vec::new();
        }
        let d = self.door(door);
        let dimension = d.dimension;
        let (first, second) = (d.first, d.second);
        let door_line_segment;
        let shrinked_line_segment;
        if dimension == 1 {
            let Some(s) = door_shape.diagonal_corner_segment() else {
                return Vec::new();
            };
            door_line_segment = s;
            shrinked_line_segment = s.shrink_segment(offset);
        } else if dimension == 2 && self.room(first).is_complete_free() && self.room(second).is_complete_free() {
            let Some(s) = self.calc_door_line_segment(&door_shape, first, second) else {
                return Vec::new();
            };
            door_line_segment = s;
            if s.b.distance_square(&s.a) < 4.0 * offset * offset {
                return Vec::new();
            }
            shrinked_line_segment = s.shrink_segment(offset);
        } else {
            let g = door_shape.centre_of_gravity();
            door_line_segment = FloatLine::new(g, g);
            shrinked_line_segment = door_line_segment;
        }
        let max_door_section_width = 10.0 * offset;
        let section_count = (door_line_segment.b.distance(&door_line_segment.a) / max_door_section_width) as i32 + 1;
        let section_count = section_count.max(1) as usize;
        self.allocate_sections(door, section_count);
        shrinked_line_segment.divide_segment_into_sections(section_count)
    }

    fn calc_door_line_segment(&self, door_shape: &TileShape, first: RoomKey, second: RoomKey) -> Option<FloatLine> {
        let first_shape = self.room(first).shape();
        let second_shape = self.room(second).shape();
        let mut first_corner: Option<Point> = None;
        let mut second_corner: Option<Point> = None;
        let n = door_shape.border_line_count();
        for i in 0..n {
            let c = door_shape.corner(i);
            if !first_shape.contains_inside(&c) && !second_shape.contains_inside(&c) {
                match &first_corner {
                    None => first_corner = Some(c),
                    Some(f) => {
                        if *f != c {
                            second_corner = Some(c);
                            break;
                        }
                    }
                }
            }
        }
        match (first_corner, second_corner) {
            (Some(a), Some(b)) => Some(FloatLine::new(a.to_float(), b.to_float())),
            _ => None,
        }
    }

    pub fn allocate_sections(&mut self, door: DoorKey, count: usize) {
        let d = self.door_mut(door);
        if d.sections.len() == count {
            return;
        }
        d.sections = vec![MazeSearchElement::default(); count];
    }

    pub fn reset_door(&mut self, door: DoorKey) {
        for s in self.door_mut(door).sections.iter_mut() {
            s.reset();
        }
    }

    // ---------------------------------------------------------------- expandables

    pub fn expandable_shape(&self, e: Expandable) -> TileShape {
        match e {
            Expandable::Door(d) => self.door_shape(d),
            Expandable::Target(t) => self.target(t).shape.clone(),
            Expandable::Drill(d) => self.drill(d).shape.clone(),
            Expandable::Page(p) => TileShape::Box(self.drill_pages.pages[p.0 as usize].shape),
        }
    }

    pub fn expandable_dimension(&self, e: Expandable) -> i32 {
        match e {
            Expandable::Door(d) => self.door(d).dimension,
            _ => 2,
        }
    }

    pub fn expandable_id(&self, e: Expandable) -> i32 {
        match e {
            Expandable::Door(d) => self.door_id(d),
            Expandable::Target(t) => {
                let td = self.target(t);
                let rid = td.room.map(|r| self.room(r).get_id()).unwrap_or(0);
                31i32.wrapping_mul(td.item.0).wrapping_add(rid)
            }
            Expandable::Drill(d) => self.drill(d).get_id(),
            Expandable::Page(p) => self.drill_pages.page_id(p),
        }
    }

    pub fn element_count(&self, e: Expandable) -> usize {
        match e {
            Expandable::Door(d) => self.door(d).sections.len(),
            Expandable::Target(_) => 1,
            Expandable::Drill(d) => self.drill(d).elems.len(),
            Expandable::Page(p) => self.drill_pages.pages[p.0 as usize].elems.len(),
        }
    }

    pub fn element(&self, e: Expandable, index: usize) -> &MazeSearchElement {
        match e {
            Expandable::Door(d) => &self.door(d).sections[index],
            Expandable::Target(t) => &self.target(t).elem,
            Expandable::Drill(d) => &self.drill(d).elems[index],
            Expandable::Page(p) => &self.drill_pages.pages[p.0 as usize].elems[index],
        }
    }

    pub fn element_mut(&mut self, e: Expandable, index: usize) -> &mut MazeSearchElement {
        match e {
            Expandable::Door(d) => &mut self.doors[d.0 as usize].sections[index],
            Expandable::Target(t) => &mut self.targets[t.0 as usize].elem,
            Expandable::Drill(d) => &mut self.drills[d.0 as usize].elems[index],
            Expandable::Page(p) => &mut self.drill_pages.pages[p.0 as usize].elems[index],
        }
    }

    pub fn reset_expandable(&mut self, e: Expandable) {
        match e {
            Expandable::Door(d) => self.reset_door(d),
            Expandable::Target(t) => self.targets[t.0 as usize].elem.reset(),
            Expandable::Drill(d) => {
                for x in self.drills[d.0 as usize].elems.iter_mut() {
                    x.reset();
                }
            }
            Expandable::Page(p) => self.reset_page(p),
        }
    }

    fn reset_page(&mut self, p: PageKey) {
        let drills = self.drill_pages.pages[p.0 as usize].drills.clone();
        if let Some(drills) = drills {
            for d in drills {
                for x in self.drills[d.0 as usize].elems.iter_mut() {
                    x.reset();
                }
            }
        }
        for x in self.drill_pages.pages[p.0 as usize].elems.iter_mut() {
            x.reset();
        }
    }

    /// `TargetItemExpansionDoor.isDestinationDoor`
    pub fn is_destination_door(&self, t: TargetKey) -> bool {
        let item = self.target(t).item;
        !self.item_info.get(&item).map(|i| i.start_info).unwrap_or(false)
    }

    /// `new TargetItemExpansionDoor(item, treeEntryNo, room, searchTree)`
    pub fn new_target_door(&mut self, board: &Board, item: ItemId, tree_entry_no: usize, room: Option<RoomKey>) -> TargetKey {
        let shape = match room {
            None => TileShape::empty(),
            Some(r) => {
                let item_shape = board.tree_shape(self.tree, item, tree_entry_no).unwrap_or_else(TileShape::empty);
                item_shape.intersection(self.room(r).shape())
            }
        };
        self.targets.push(TargetDoor {
            item,
            tree_entry_no,
            room,
            shape,
            elem: MazeSearchElement::default(),
        });
        TargetKey((self.targets.len() - 1) as u32)
    }

    // ---------------------------------------------------------------- item infos

    pub fn item_info_mut(&mut self, id: ItemId) -> &mut ItemAutorouteInfo {
        self.item_info.entry(id).or_default()
    }

    /// `ItemAutorouteInfo.getExpansionRoom(index, autorouteTree)`
    pub fn get_obstacle_room(&mut self, board: &Board, id: ItemId, index: usize) -> Option<RoomKey> {
        let tree = board.search_trees.get(self.tree);
        let obj = TreeObject::Item(id);
        let count = if tree.contains_object(&obj) {
            tree.object_shape_count(&obj)
        } else {
            let (s, _) = tree.calculate_tree_shapes(board.item(id), &board.tree_env());
            s.len()
        };
        let info = self.item_info.entry(id).or_default();
        if info.rooms.len() != count {
            info.rooms.resize(count, None);
        }
        if index >= info.rooms.len() {
            log::warn!(
                "ItemAutorouteInfo.get_expansion_room: index {} out of range [0, {})",
                index,
                info.rooms.len()
            );
            return None;
        }
        if let Some(r) = info.rooms[index] {
            return Some(r);
        }
        let shape = board.tree_shape(self.tree, id, index);
        let layer = item_shape_layer(board, self.tree, id, index);
        let key = self.new_room(Room {
            kind: RoomKind::Obstacle {
                item: id,
                index,
                doors_calculated: false,
            },
            shape,
            layer,
            doors: Vec::new(),
        });
        self.item_info.get_mut(&id).unwrap().rooms[index] = Some(key);
        Some(key)
    }

    /// `ObstacleExpansionRoom.createOverlapDoor(other)`
    pub fn create_overlap_door(&mut self, board: &Board, room: RoomKey, other: RoomKey) -> bool {
        if self.door_exists(room, other) {
            return false;
        }
        let (Some(a), Some(b)) = (self.room(room).obstacle_item(), self.room(other).obstacle_item()) else {
            return false;
        };
        let ia = board.item(a);
        let ib = board.item(b);
        if !(ia.is_routable() && ib.is_routable()) {
            return false;
        }
        if !ia.shares_net(ib) {
            return false;
        }
        if a == b {
            if !ia.is_trace() {
                return false;
            }
            let i1 = self.room(room).obstacle_index() as i64;
            let i2 = self.room(other).obstacle_index() as i64;
            if i1 != i2 + 1 && i1 != i2 - 1 {
                return false;
            }
        }
        let d = self.new_door(room, other, 2);
        self.add_door(room, d);
        self.add_door(other, d);
        true
    }

    /// `Via.getAutorouteDrillInfo`
    pub fn via_drill_info(&mut self, board: &Board, via: ItemId) -> DrillKey {
        if let Some(d) = self.item_info.get(&via).and_then(|i| i.drill) {
            return d;
        }
        let item = board.item(via);
        let center = item.as_drill().unwrap().center.clone();
        let first = item.first_layer();
        let last = item.last_layer_with(board.layer_count());
        let mut drill = Drill::new(TileShape::from_point(&center), center, first, last);
        let n = (last - first + 1).max(0) as usize;
        let mut rooms = Vec::with_capacity(n);
        for i in 0..n {
            rooms.push(self.get_obstacle_room(board, via, i));
        }
        drill.rooms = rooms;
        self.drills.push(drill);
        let key = DrillKey((self.drills.len() - 1) as u32);
        self.item_info_mut(via).drill = Some(key);
        key
    }

    // ---------------------------------------------------------------- rooms

    pub fn add_incomplete_expansion_room(&mut self, shape: Option<TileShape>, layer: i32, contained: Option<TileShape>) -> RoomKey {
        let key = self.new_room(Room {
            kind: RoomKind::Incomplete { contained },
            shape,
            layer,
            doors: Vec::new(),
        });
        self.incomplete_rooms.get_or_insert_with(Vec::new).push(key);
        key
    }

    pub fn get_first_incomplete_expansion_room(&self) -> Option<RoomKey> {
        self.incomplete_rooms.as_ref().and_then(|v| v.first().copied())
    }

    pub fn remove_incomplete_expansion_room(&mut self, room: RoomKey) {
        self.remove_all_doors(room);
        if let Some(v) = &mut self.incomplete_rooms {
            if let Some(p) = v.iter().position(|r| *r == room) {
                v.remove(p);
            }
        }
    }

    /// `removeAllDoors(room)`
    pub fn remove_all_doors(&mut self, room: RoomKey) {
        let doors = self.room(room).doors.clone();
        for d in doors {
            let Some(other) = self.door_other_room(d, room) else {
                continue;
            };
            self.remove_door_from_room(other, d);
            if self.room(other).is_incomplete() {
                self.remove_incomplete_expansion_room(other);
            }
        }
        self.room_mut(room).doors = Vec::new();
        if let RoomKind::Complete { target_doors, .. } = &mut self.room_mut(room).kind {
            target_doors.clear();
        }
    }

    pub fn remove_complete_expansion_room(&mut self, board: &mut Board, room: RoomKey) {
        let room_shape = self.room(room).shape().clone();
        let room_layer = self.room(room).layer;
        let doors = self.room(room).doors.clone();
        for d in doors {
            let Some(neighbour) = self.door_other_room(d, room) else {
                continue;
            };
            self.remove_door_from_room(neighbour, d);
            let neighbour_shape = self.room(neighbour).shape().clone();
            let intersection = room_shape.intersection(&neighbour_shape);
            if intersection.dimension() == 1 {
                let Some((_, side1)) = room_shape.touching_sides(&neighbour_shape) else {
                    continue;
                };
                let line = neighbour_shape.border_line(side1).opposite();
                let new_shape = TileShape::Simplex(Simplex::get_instance(&[line]));
                let new_room = self.add_incomplete_expansion_room(Some(new_shape), room_layer, Some(intersection));
                let new_door = self.new_door(neighbour, new_room, 1);
                self.add_door(neighbour, new_door);
                self.add_door(new_room, new_door);
            }
        }
        self.remove_all_doors(room);
        if let Some(obj) = self.room(room).tree_object(room) {
            board.search_trees.get_mut(self.tree).remove_object(&obj);
        }
        match &mut self.complete_rooms {
            Some(v) => {
                if let Some(p) = v.iter().position(|r| *r == room) {
                    v.remove(p);
                }
            }
            None => log::warn!("AutorouteEngine.remove_complete_expansion_room: this.completeExpansionRooms is null"),
        }
        self.invalidate_drill_pages(&room_shape);
    }

    /// `completeExpansionRoom`
    pub fn complete_expansion_room(&mut self, board: &mut Board, room: RoomKey) -> Vec<RoomKey> {
        let mut result = Vec::new();
        let mut from_door_shape: Option<TileShape> = None;
        let mut ignore_object: Option<TreeObject> = None;
        for d in self.room(room).doors.clone() {
            let Some(other) = self.door_other_room(d, room) else {
                continue;
            };
            if self.room(other).is_complete_free() && self.door(d).dimension == 2 {
                from_door_shape = Some(self.door_shape(d));
                ignore_object = self.room(other).tree_object(other);
                break;
            }
        }
        let inc = {
            let r = self.room(room);
            IncompleteRoom {
                shape: r.shape.clone(),
                layer: r.layer,
                contained: r.contained_shape().cloned(),
            }
        };
        let completed_shapes = super::complete_shape::complete_shape(
            board,
            self.tree,
            &inc,
            self.net_no,
            ignore_object.as_ref(),
            from_door_shape.as_ref(),
        );
        self.remove_incomplete_expansion_room(room);
        let mut is_first = true;
        for current in completed_shapes {
            if current.shape.as_ref().map(|s| s.dimension()).unwrap_or(-1) != 2 {
                continue;
            }
            if is_first {
                is_first = false;
                if let Some(c) = self.add_complete_room(board, current) {
                    result.push(c);
                }
            } else {
                let shapes = super::complete_shape::complete_shape(
                    board,
                    self.tree,
                    &current,
                    self.net_no,
                    ignore_object.as_ref(),
                    from_door_shape.as_ref(),
                );
                for tmp in shapes {
                    if let Some(c) = self.add_complete_room(board, tmp) {
                        result.push(c);
                    }
                }
            }
        }
        result
    }

    fn add_complete_room(&mut self, board: &mut Board, room: IncompleteRoom) -> Option<RoomKey> {
        // The incomplete room object of the original is only used to calculate the doors.
        let key = self.new_room(Room {
            kind: RoomKind::Incomplete { contained: room.contained },
            shape: room.shape,
            layer: room.layer,
            doors: Vec::new(),
        });
        let completed = self.calculate_doors(board, key)?;
        if self.room(completed).shape().dimension() != 2 {
            return None;
        }
        self.complete_rooms.get_or_insert_with(Vec::new).push(completed);
        let r = self.room(completed);
        let obj = r.tree_object(completed).unwrap();
        let shape = r.shape().clone();
        let layer = r.layer;
        board.search_trees.get_mut(self.tree).insert_shapes(obj, vec![Some(shape)], &[layer]);
        Some(completed)
    }

    /// `calculateDoors` = `SortedRoomNeighbours.complete`
    pub fn calculate_doors(&mut self, board: &Board, room: RoomKey) -> Option<RoomKey> {
        super::neighbours::complete(self, board, room)
    }

    /// `completeNeighbourRooms`
    pub fn complete_neighbour_rooms(&mut self, board: &mut Board, room: RoomKey) {
        let mut i = 0;
        while i < self.room(room).doors.len() {
            let d = self.room(room).doors[i];
            i += 1;
            let Some(neighbour) = self.door_other_room(d, room) else {
                continue;
            };
            match self.room(neighbour).kind {
                RoomKind::Incomplete { .. } => {
                    self.complete_expansion_room(board, neighbour);
                    i = 0;
                }
                RoomKind::Obstacle { doors_calculated, .. } => {
                    if !doors_calculated {
                        self.calculate_doors(board, neighbour);
                        if let RoomKind::Obstacle { doors_calculated, .. } = &mut self.room_mut(neighbour).kind {
                            *doors_calculated = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn invalidate_drill_pages(&mut self, shape: &TileShape) {
        for p in self.drill_pages.overlapping_pages(shape) {
            self.drill_pages.pages[p.0 as usize].drills = None;
        }
    }

    /// `getRoomsWithTargetItems`: sorted by descending room id.
    pub fn get_rooms_with_target_items(&self, items: &std::collections::BTreeSet<ItemId>) -> Vec<RoomKey> {
        let mut result: Vec<RoomKey> = Vec::new();
        if let Some(rooms) = &self.complete_rooms {
            for r in rooms {
                for t in self.room(*r).target_doors() {
                    if items.contains(&self.target(*t).item) && !result.contains(r) {
                        result.push(*r);
                    }
                }
            }
        }
        result.sort_by(|a, b| self.room(*b).get_id().cmp(&self.room(*a).get_id()));
        result
    }

    /// `resetAllDoors`
    pub fn reset_all_doors(&mut self) {
        if let Some(rooms) = self.complete_rooms.clone() {
            for r in rooms {
                for d in self.room(r).doors.clone() {
                    self.reset_door(d);
                }
                for t in self.room(r).target_doors().to_vec() {
                    self.targets[t.0 as usize].elem.reset();
                }
            }
        }
        let ids: Vec<ItemId> = self.item_info.keys().copied().collect();
        for id in ids {
            let rooms = self.item_info[&id].rooms.clone();
            for r in rooms.into_iter().flatten() {
                for d in self.room(r).doors.clone() {
                    self.reset_door(d);
                }
            }
            self.item_info.get_mut(&id).unwrap().precalculated_connection = None;
        }
        for i in 0..self.drill_pages.pages.len() {
            self.reset_page(PageKey(i as u32));
        }
    }

    // ---------------------------------------------------------------- drills

    /// `DrillPage.getDrills`
    pub fn page_drills(&mut self, board: &mut Board, page: PageKey, attach_smd: bool) -> Vec<DrillKey> {
        let p = &self.drill_pages.pages[page.0 as usize];
        if p.drills.is_some() && self.net_no == p.net_no {
            return p.drills.clone().unwrap();
        }
        let page_box = p.shape;
        self.drill_pages.pages[page.0 as usize].net_no = self.net_no;
        self.drill_pages.pages[page.0 as usize].drills = Some(Vec::new());
        let net_no = self.net_no;
        let page_shape = TileShape::Box(page_box);
        let mut overlaps = Vec::new();
        board
            .search_trees
            .get(self.tree)
            .overlapping_tree_entries(&page_shape, -1, &[], &board.lookup(), &mut overlaps);
        let mut cutout_shapes: Vec<Shape> = Vec::new();
        let mut prev_obstacle_shape = TileShape::Box(IntBox::EMPTY);
        for e in &overlaps {
            let TreeObject::Item(id) = e.object else {
                continue;
            };
            let item = board.item(id);
            if item.is_drillable(net_no) {
                continue;
            }
            if item.is_pin() && attach_smd && pin_drill_allowed(board, id) && item.contains_net(net_no) {
                continue;
            }
            let Some(current) = board.search_trees.get(self.tree).object_shape(&e.object, e.shape_index).cloned() else {
                continue;
            };
            if !prev_obstacle_shape.contains_tile(&current) {
                let cut = current.intersection(&page_shape);
                if cut.dimension() == 2 {
                    cutout_shapes.push(Shape::Tile(cut));
                }
            }
            prev_obstacle_shape = current;
        }
        let area = PolylineArea::new(Shape::Tile(page_shape.clone()), cutout_shapes);
        let Some(drill_shapes) = area.split_to_convex().map(|s| s.to_vec()) else {
            return Vec::new();
        };
        let first_layer = 0;
        let last_layer = board.layer_count() - 1;
        let mut result = Vec::new();
        for ds in drill_shapes {
            let mut pin_center = None;
            if attach_smd {
                pin_center = calc_pin_center_in_drill(board, &ds, first_layer);
                if pin_center.is_none() {
                    pin_center = calc_pin_center_in_drill(board, &ds, last_layer);
                }
            }
            let location = pin_center.clone().unwrap_or_else(|| Point::Int(ds.centre_of_gravity().round()));
            let d = Drill::new(ds.clone(), location, first_layer, last_layer);
            if let Some(k) = self.calculate_drill_rooms(board, d) {
                result.push(k);
            }
            if let Some(pc) = pin_center {
                let cog = Point::Int(ds.centre_of_gravity().round());
                if cog != pc {
                    let d = Drill::new(ds.clone(), cog, first_layer, last_layer);
                    if let Some(k) = self.calculate_drill_rooms(board, d) {
                        result.push(k);
                    }
                }
            }
        }
        self.drill_pages.pages[page.0 as usize].drills = Some(result.clone());
        result
    }

    /// `ExpansionDrill.calculateExpansionRooms`; stores the drill if successful.
    fn calculate_drill_rooms(&mut self, board: &mut Board, mut drill: Drill) -> Option<DrillKey> {
        let search_shape = TileShape::from_point(&drill.location);
        let mut overlaps = board
            .search_trees
            .get(self.tree)
            .overlapping_objects(&search_shape, -1, &board.lookup());
        for i in drill.first_layer..=drill.last_layer {
            let mut found: Option<RoomKey> = None;
            let mut k = 0;
            while k < overlaps.len() {
                let o = overlaps[k];
                // only complete expansion rooms are kept; obstacle rooms of items are no
                // CompleteExpansionRoom objects in the tree
                let TreeObject::Room(r, _) = o else {
                    overlaps.remove(k);
                    continue;
                };
                let key = RoomKey(r.0);
                if self.room(key).layer == i {
                    found = Some(key);
                    overlaps.remove(k);
                    break;
                }
                k += 1;
            }
            if found.is_none() {
                let inc = self.add_incomplete_expansion_room(None, i, Some(search_shape.clone()));
                let new_rooms = self.complete_expansion_room(board, inc);
                if new_rooms.len() != 1 {
                    return None;
                }
                found = new_rooms.first().copied();
            }
            drill.rooms[(i - drill.first_layer) as usize] = found;
        }
        self.drills.push(drill);
        Some(DrillKey((self.drills.len() - 1) as u32))
    }
}

fn pin_drill_allowed(board: &Board, id: ItemId) -> bool {
    board.item(id).as_pin().map(|p| p.drill_allowed()).unwrap_or(false)
}

/// `DrillPage.calcPinCenterInDrill`
fn calc_pin_center_in_drill(board: &Board, drill_shape: &TileShape, layer: i32) -> Option<Point> {
    let items = board.overlapping_items(drill_shape, layer);
    let mut result = None;
    for id in items {
        let item = board.item(id);
        if let Some(pin) = item.as_pin() {
            let center = &item.as_drill().unwrap().center;
            if pin.drill_allowed() && drill_shape.contains_inside(center) {
                result = Some(center.clone());
            }
        }
    }
    result
}

/// The layer of the tree shape `index` of an item.
pub fn item_shape_layer(board: &Board, tree: TreeId, id: ItemId, index: usize) -> i32 {
    let t = board.search_trees.get(tree);
    if let Some(entries) = t.entries_of(&TreeObject::Item(id)) {
        if let Some(l) = entries.get(index) {
            if *l != crate::datastructures::shape_tree::NONE {
                return t.leaf(*l).layer;
            }
        }
    }
    let item = board.any_item(id).expect("item");
    let (shapes, layers) = t.calculate_tree_shapes(item, &board.tree_env());
    let _ = shapes;
    layers.get(index).copied().unwrap_or_else(|| item.first_layer())
}

/// `item.treeShapeCount(tree)`, also for items removed from the board.
pub fn item_tree_shape_count(board: &Board, tree: TreeId, id: ItemId) -> usize {
    let t = board.search_trees.get(tree);
    let obj = TreeObject::Item(id);
    if t.contains_object(&obj) {
        return t.object_shape_count(&obj);
    }
    match board.any_item(id) {
        Some(item) => t.calculate_tree_shapes(item, &board.tree_env()).0.len(),
        None => 0,
    }
}

#[allow(dead_code)]
fn _unused(_: FloatPoint, _: Line) {}
