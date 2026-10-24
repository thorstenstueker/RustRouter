//! Board items (port of `app.freerouting.board.model.items`).
//!
//! In the original, items form a class hierarchy with a back reference to the board.
//! Here an item is a struct with common fields and a `kind` enum. Item methods,
//! which need board information, take the board as parameter.

use super::structure::FixedState;
use crate::geometry::{
    Area, FloatPoint, IntBox, IntOctagon, Point, Polyline, Shape, TileShape, Vector,
};
use crate::library::PadstackId;
use smallvec::SmallVec;
use std::cmp::Ordering;

/// Id of a board item. The ordering is descending by number, which reproduces
/// the iteration order of the item collections of the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ItemId(pub i32);

impl Ord for ItemId {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.cmp(&self.0)
    }
}

impl PartialOrd for ItemId {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for ItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

pub type NetNumbers = SmallVec<[i32; 2]>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoardItemType {
    Trace,
    Pin,
    Via,
    ObstacleArea,
    ViaObstacleArea,
    ConductionArea,
    ComponentObstacleArea,
    BoardOutline,
    ComponentOutline,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: ItemId,
    pub net_numbers: NetNumbers,
    pub clearance_class: i32,
    pub component_id: i32,
    pub fixed_state: FixedState,
    pub on_the_board: bool,
    pub smallest_clearance: f64,
    pub kind: ItemKind,
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Pin(Pin),
    Via(Via),
    Trace(PolylineTrace),
    Obstacle(ObstacleArea),
    ComponentOutline(ComponentOutline),
    BoardOutline(BoardOutline),
}

/// Common data of pins and vias.
#[derive(Clone, Debug)]
pub struct Drill {
    pub center: Point,
    pub padstack: PadstackId,
    pub first_layer: i32,
    pub last_layer: i32,
    /// Shapes on the layers `first_layer ..= last_layer` in board coordinates.
    pub shapes: Vec<Option<Shape>>,
    /// Number of tile shapes (padstack layer range length).
    pub tile_shape_count: usize,
}

#[derive(Clone, Debug)]
pub struct Pin {
    pub pin_index: i32,
    pub drill: Drill,
    pub changed_to: ItemId,
}

#[derive(Clone, Debug)]
pub struct Via {
    pub drill: Drill,
    pub attach_allowed: bool,
    pub is_escape_via: bool,
    pub escape_via_smd_layer: i32,
}

#[derive(Clone, Debug)]
pub struct PolylineTrace {
    pub polyline: Polyline,
    pub layer: i32,
    pub half_width: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObstacleKind {
    Obstacle,
    ViaObstacle,
    ComponentObstacle,
    Conduction { is_obstacle: bool, is_filled: bool },
}

#[derive(Clone, Debug)]
pub struct ObstacleArea {
    pub kind: ObstacleKind,
    pub name: Option<String>,
    pub relative_area: Area,
    pub layer: i32,
    pub translation: Vector,
    pub rotation_in_degree: f64,
    pub side_changed: bool,
    /// The area in board coordinates (precalculated from the placement).
    pub area: Area,
    /// The convex pieces of `area`, empty if the division failed.
    pub convex_pieces: Vec<TileShape>,
}

#[derive(Clone, Debug)]
pub struct ComponentOutline {
    pub relative_area: Area,
    pub area: Area,
    pub translation: Vector,
    pub rotation_in_degree: f64,
    pub is_front: bool,
    pub is_courtyard: bool,
    pub is_fabrication: bool,
    pub is_closed: bool,
}

#[derive(Clone, Debug)]
pub struct BoardOutline {
    pub shapes: Vec<Shape>,
    pub keepout_outside_outline: bool,
    pub edge_pin_nets_cross_outline: bool,
}

impl BoardOutline {
    pub const HALF_WIDTH: i32 = 100;

    pub fn line_count(&self) -> usize {
        self.shapes.iter().map(|s| s.border_line_count()).sum()
    }

    pub fn contains(&self, p: &Point) -> bool {
        self.shapes.iter().any(|s| s.contains(p))
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        self.shapes.iter().any(|s| s.contains_float(p))
    }

    pub fn bounding_box(&self) -> IntBox {
        self.shapes
            .iter()
            .fold(IntBox::EMPTY, |r, s| r.union(&s.bounding_box()))
    }
}

impl Item {
    pub fn board_item_type(&self) -> BoardItemType {
        match &self.kind {
            ItemKind::Pin(_) => BoardItemType::Pin,
            ItemKind::Via(_) => BoardItemType::Via,
            ItemKind::Trace(_) => BoardItemType::Trace,
            ItemKind::Obstacle(o) => match o.kind {
                ObstacleKind::Obstacle => BoardItemType::ObstacleArea,
                ObstacleKind::ViaObstacle => BoardItemType::ViaObstacleArea,
                ObstacleKind::ComponentObstacle => BoardItemType::ComponentObstacleArea,
                ObstacleKind::Conduction { .. } => BoardItemType::ConductionArea,
            },
            ItemKind::ComponentOutline(_) => BoardItemType::ComponentOutline,
            ItemKind::BoardOutline(_) => BoardItemType::BoardOutline,
        }
    }

    // ------------------------------------------------------------ type queries

    #[inline]
    pub fn is_pin(&self) -> bool {
        matches!(self.kind, ItemKind::Pin(_))
    }

    #[inline]
    pub fn is_via(&self) -> bool {
        matches!(self.kind, ItemKind::Via(_))
    }

    #[inline]
    pub fn is_drill_item(&self) -> bool {
        matches!(self.kind, ItemKind::Pin(_) | ItemKind::Via(_))
    }

    #[inline]
    pub fn is_trace(&self) -> bool {
        matches!(self.kind, ItemKind::Trace(_))
    }

    #[inline]
    pub fn is_board_outline(&self) -> bool {
        matches!(self.kind, ItemKind::BoardOutline(_))
    }

    /// True for all obstacle areas including conduction, via and component obstacle areas.
    #[inline]
    pub fn is_obstacle_area(&self) -> bool {
        matches!(self.kind, ItemKind::Obstacle(_))
    }

    #[inline]
    pub fn is_conduction_area(&self) -> bool {
        matches!(&self.kind, ItemKind::Obstacle(o) if matches!(o.kind, ObstacleKind::Conduction { .. }))
    }

    #[inline]
    pub fn is_component_obstacle_area(&self) -> bool {
        matches!(&self.kind, ItemKind::Obstacle(o) if o.kind == ObstacleKind::ComponentObstacle)
    }

    #[inline]
    pub fn is_via_obstacle_area(&self) -> bool {
        matches!(&self.kind, ItemKind::Obstacle(o) if o.kind == ObstacleKind::ViaObstacle)
    }

    /// Conduction areas with `is_obstacle == false` are no obstacles.
    #[inline]
    pub fn is_non_obstacle_conduction_area(&self) -> bool {
        matches!(&self.kind, ItemKind::Obstacle(o) if matches!(o.kind, ObstacleKind::Conduction { is_obstacle: false, .. }))
    }

    /// Items implementing `Connectable` in the original: pins, vias, traces and conduction areas.
    #[inline]
    pub fn is_connectable_type(&self) -> bool {
        match &self.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) | ItemKind::Trace(_) => true,
            ItemKind::Obstacle(o) => matches!(o.kind, ObstacleKind::Conduction { .. }),
            _ => false,
        }
    }

    pub fn as_trace(&self) -> Option<&PolylineTrace> {
        match &self.kind {
            ItemKind::Trace(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_trace_mut(&mut self) -> Option<&mut PolylineTrace> {
        match &mut self.kind {
            ItemKind::Trace(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_drill(&self) -> Option<&Drill> {
        match &self.kind {
            ItemKind::Pin(p) => Some(&p.drill),
            ItemKind::Via(v) => Some(&v.drill),
            _ => None,
        }
    }

    pub fn as_pin(&self) -> Option<&Pin> {
        match &self.kind {
            ItemKind::Pin(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_via(&self) -> Option<&Via> {
        match &self.kind {
            ItemKind::Via(v) => Some(v),
            _ => None,
        }
    }

    pub fn as_obstacle(&self) -> Option<&ObstacleArea> {
        match &self.kind {
            ItemKind::Obstacle(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_obstacle_mut(&mut self) -> Option<&mut ObstacleArea> {
        match &mut self.kind {
            ItemKind::Obstacle(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_board_outline(&self) -> Option<&BoardOutline> {
        match &self.kind {
            ItemKind::BoardOutline(o) => Some(o),
            _ => None,
        }
    }

    // ------------------------------------------------------------ nets

    #[inline]
    pub fn contains_net(&self, net_no: i32) -> bool {
        if net_no <= 0 {
            return false;
        }
        self.net_numbers.contains(&net_no)
    }

    #[inline]
    pub fn shares_net_no(&self, nets: &[i32]) -> bool {
        self.net_numbers.iter().any(|n| nets.contains(n))
    }

    #[inline]
    pub fn shares_net(&self, other: &Item) -> bool {
        self.shares_net_no(&other.net_numbers)
    }

    #[inline]
    pub fn net_count(&self) -> usize {
        self.net_numbers.len()
    }

    #[inline]
    pub fn get_net_number(&self, i: usize) -> i32 {
        self.net_numbers[i]
    }

    pub fn nets_normal(&self) -> bool {
        self.net_numbers
            .iter()
            .all(|n| crate::rules::Nets::is_normal_net_number(*n))
    }

    pub fn nets_equal(&self, nets: &[i32]) -> bool {
        if self.net_numbers.len() != nets.len() {
            return false;
        }
        nets.iter().all(|n| self.contains_net(*n))
    }

    pub fn nets_equal_item(&self, other: &Item) -> bool {
        self.nets_equal(&other.net_numbers)
    }

    pub fn is_connectable(&self) -> bool {
        self.is_connectable_type() && self.net_count() > 0
    }

    // ------------------------------------------------------------ fixed state

    #[inline]
    pub fn is_user_fixed(&self) -> bool {
        self.fixed_state >= FixedState::UserFixed
    }

    /// The fixed state part of `isShoveFixed`; traces additionally check the net class
    /// (see `Board::is_shove_fixed`).
    #[inline]
    pub fn is_shove_fixed_state(&self) -> bool {
        self.fixed_state >= FixedState::ShoveFixed
    }

    pub fn unfix(&mut self) {
        if self.fixed_state != FixedState::SystemFixed {
            self.fixed_state = FixedState::Unfixed;
        }
    }

    pub fn is_routable(&self) -> bool {
        match &self.kind {
            ItemKind::Trace(_) | ItemKind::Via(_) => !self.is_user_fixed() && self.net_count() > 0,
            _ => false,
        }
    }

    // ------------------------------------------------------------ layers

    pub fn first_layer(&self) -> i32 {
        match &self.kind {
            ItemKind::Pin(p) => p.drill.first_layer,
            ItemKind::Via(v) => v.drill.first_layer,
            ItemKind::Trace(t) => t.layer,
            ItemKind::Obstacle(o) => o.layer,
            ItemKind::ComponentOutline(_) => -1, // resolved by the board, see layer_of_component_outline
            ItemKind::BoardOutline(_) => 0,
        }
    }

    /// `last_layer` needs the board layer count for board outlines.
    pub fn last_layer_with(&self, layer_count: i32) -> i32 {
        match &self.kind {
            ItemKind::Pin(p) => p.drill.last_layer,
            ItemKind::Via(v) => v.drill.last_layer,
            ItemKind::Trace(t) => t.layer,
            ItemKind::Obstacle(o) => o.layer,
            ItemKind::ComponentOutline(c) => {
                if c.is_front {
                    0
                } else {
                    layer_count - 1
                }
            }
            ItemKind::BoardOutline(_) => layer_count - 1,
        }
    }

    pub fn first_layer_with(&self, layer_count: i32) -> i32 {
        match &self.kind {
            ItemKind::ComponentOutline(c) => {
                if c.is_front {
                    0
                } else {
                    layer_count - 1
                }
            }
            _ => self.first_layer(),
        }
    }

    pub fn is_on_layer(&self, layer: i32, layer_count: i32) -> bool {
        match &self.kind {
            ItemKind::BoardOutline(_) => true,
            _ => layer >= self.first_layer_with(layer_count) && layer <= self.last_layer_with(layer_count),
        }
    }

    pub fn shares_layer(&self, other: &Item, layer_count: i32) -> bool {
        let max_first = self.first_layer_with(layer_count).max(other.first_layer_with(layer_count));
        let min_last = self.last_layer_with(layer_count).min(other.last_layer_with(layer_count));
        max_first <= min_last
    }

    pub fn first_common_layer(&self, other: &Item, layer_count: i32) -> i32 {
        let max_first = self.first_layer_with(layer_count).max(other.first_layer_with(layer_count));
        let min_last = self.last_layer_with(layer_count).min(other.last_layer_with(layer_count));
        if max_first > min_last {
            -1
        } else {
            max_first
        }
    }

    pub fn last_common_layer(&self, other: &Item, layer_count: i32) -> i32 {
        let max_first = self.first_layer_with(layer_count).max(other.first_layer_with(layer_count));
        let min_last = self.last_layer_with(layer_count).min(other.last_layer_with(layer_count));
        if max_first > min_last {
            -1
        } else {
            min_last
        }
    }

    // ------------------------------------------------------------ geometry

    pub fn bounding_box(&self) -> IntBox {
        match &self.kind {
            ItemKind::Pin(p) => p.drill.bounding_box(),
            ItemKind::Via(v) => v.drill.bounding_box(),
            ItemKind::Trace(t) => t.bounding_box(),
            ItemKind::Obstacle(o) => o.area.bounding_box(),
            ItemKind::ComponentOutline(c) => c.area.bounding_box(),
            ItemKind::BoardOutline(o) => o.bounding_box(),
        }
    }

    // ------------------------------------------------------------ obstacle relations

    /// Returns true, if this item is an obstacle to `other` (Java: `this.isObstacle(other)`).
    pub fn is_obstacle_for(&self, other: &Item, outline_blocks: impl Fn(&[i32]) -> bool) -> bool {
        if self.id == other.id {
            return false;
        }
        match &self.kind {
            ItemKind::Pin(p) => {
                if other.is_non_obstacle_conduction_area() {
                    return false;
                }
                if other.is_obstacle_area() {
                    if self.shares_net(other) {
                        return false;
                    }
                    return other.net_count() > 0;
                }
                if !other.shares_net(self) {
                    if let ItemKind::Pin(op) = &other.kind {
                        // logical pad check is done by the board (needs package pin names)
                        let _ = op;
                    }
                    return true;
                }
                if other.is_trace() || other.is_pin() {
                    return false;
                }
                !p.drill_allowed() || !other.is_via()
            }
            ItemKind::Via(v) => {
                if other.is_component_obstacle_area() {
                    return false;
                }
                if other.is_non_obstacle_conduction_area() {
                    return false;
                }
                if !other.shares_net(self) {
                    return true;
                }
                if other.is_trace() {
                    return false;
                }
                match &other.kind {
                    ItemKind::Pin(op) => !v.attach_allowed || !op.drill_allowed(),
                    _ => true,
                }
            }
            ItemKind::Trace(_) => {
                if other.is_via_obstacle_area() || other.is_component_obstacle_area() {
                    return false;
                }
                if other.is_non_obstacle_conduction_area() {
                    return false;
                }
                !other.shares_net(self)
            }
            ItemKind::Obstacle(o) => match o.kind {
                ObstacleKind::ComponentObstacle => {
                    other.is_component_obstacle_area() && other.component_id != self.component_id
                }
                ObstacleKind::ViaObstacle => {
                    if other.shares_net(self) {
                        return false;
                    }
                    other.is_via()
                }
                ObstacleKind::Conduction { is_obstacle, .. } => {
                    if is_obstacle {
                        self.obstacle_area_is_obstacle(other)
                    } else {
                        false
                    }
                }
                ObstacleKind::Obstacle => self.obstacle_area_is_obstacle(other),
            },
            ItemKind::ComponentOutline(_) => false,
            ItemKind::BoardOutline(_) => {
                if other.is_board_outline() || other.is_obstacle_area() {
                    return false;
                }
                if other.is_trace() && !outline_blocks(&other.net_numbers) {
                    return false;
                }
                true
            }
        }
    }

    fn obstacle_area_is_obstacle(&self, other: &Item) -> bool {
        if other.shares_net(self) {
            return false;
        }
        if other.is_pin() {
            return self.net_count() > 0;
        }
        if other.is_obstacle_area() {
            return self.net_count() > 0 && other.net_count() > 0;
        }
        other.is_trace() || other.is_via()
    }

    /// `isObstacle(int netNumber)` of the original, except for board outlines
    /// (use `Board::is_obstacle_net`).
    #[inline]
    pub fn is_obstacle_net_basic(&self, net_no: i32) -> bool {
        !self.contains_net(net_no)
    }

    /// `isTraceObstacle(int netNumber)` of the original, except for board outlines.
    #[inline]
    pub fn is_trace_obstacle_basic(&self, net_no: i32) -> bool {
        match &self.kind {
            ItemKind::Obstacle(o) => match o.kind {
                ObstacleKind::ComponentObstacle | ObstacleKind::ViaObstacle => false,
                ObstacleKind::Conduction { is_obstacle, .. } => is_obstacle && !self.contains_net(net_no),
                ObstacleKind::Obstacle => !self.contains_net(net_no),
            },
            _ => !self.contains_net(net_no),
        }
    }

    pub fn is_drillable(&self, net_no: i32) -> bool {
        match &self.kind {
            ItemKind::Trace(_) => self.contains_net(net_no),
            ItemKind::Obstacle(o) => match o.kind {
                ObstacleKind::Conduction { is_obstacle, .. } => !is_obstacle || self.contains_net(net_no),
                _ => false,
            },
            _ => false,
        }
    }

    // ------------------------------------------------------------ misc

    pub fn remove_from_net(&mut self, net_no: i32) -> bool {
        let mut found = None;
        for (i, n) in self.net_numbers.iter().enumerate() {
            if *n == net_no {
                found = Some(i);
            }
        }
        match found {
            Some(i) => {
                self.net_numbers.remove(i);
                true
            }
            None => false,
        }
    }

    pub fn shape_layer_with(&self, index: usize, layer_count: i32, tile_shape_count: usize) -> i32 {
        match &self.kind {
            ItemKind::Pin(p) => p.drill.shape_layer(index),
            ItemKind::Via(v) => v.drill.shape_layer(index),
            ItemKind::Trace(t) => t.layer,
            ItemKind::Obstacle(o) => o.layer,
            ItemKind::ComponentOutline(c) => {
                if c.is_front {
                    0
                } else {
                    layer_count - 1
                }
            }
            ItemKind::BoardOutline(_) => {
                if tile_shape_count > 0 {
                    (index as i64 * layer_count as i64 / tile_shape_count as i64) as i32
                } else {
                    0
                }
            }
        }
    }

    pub fn smallest_radius(&self) -> f64 {
        match self.as_drill() {
            Some(d) => d.smallest_radius(),
            None => 0.0,
        }
    }
}

impl Drill {
    #[inline]
    pub fn shape_layer(&self, index: usize) -> i32 {
        let idx = (index as i32).min(self.last_layer - self.first_layer);
        self.first_layer + idx.max(0)
    }

    pub fn get_shape(&self, index: usize) -> Option<&Shape> {
        self.shapes.get(index).and_then(|s| s.as_ref())
    }

    pub fn get_shape_on_layer(&self, layer: i32) -> Option<&Shape> {
        if layer < self.first_layer || layer > self.last_layer {
            log::warn!("DrillItem.get_shape_on_layer: layer out of range");
            return None;
        }
        self.get_shape((layer - self.first_layer) as usize)
    }

    pub fn bounding_box(&self) -> IntBox {
        let mut r = IntBox::EMPTY;
        for s in self.shapes.iter().take(self.tile_shape_count).flatten() {
            r = r.union(&s.bounding_box());
        }
        r
    }

    pub fn smallest_radius(&self) -> f64 {
        let c = self.center.to_float();
        let mut r = f64::MAX;
        for s in self.shapes.iter().take(self.tile_shape_count).flatten() {
            r = r.min(s.border_distance(&c));
        }
        r
    }

    pub fn min_width(&self, signal_layers: &[bool]) -> f64 {
        let mut min = i32::MAX as f64;
        for layer in self.first_layer..=self.last_layer {
            if !signal_layers.get(layer as usize).copied().unwrap_or(true) {
                continue;
            }
            if let Some(s) = self.get_shape_on_layer(layer) {
                let b = s.bounding_box();
                min = min.min(b.width() as f64).min(b.height() as f64);
            }
        }
        min
    }
}

impl Pin {
    #[inline]
    pub fn drill_allowed(&self) -> bool {
        self.drill.first_layer == self.drill.last_layer
    }

    /// Base name of a pin used to detect pads of the same logical pin.
    pub fn get_base_pin_name(pin_name: &str) -> String {
        if let Some(at) = pin_name.find('@') {
            if at > 0 {
                return pin_name[..at].to_string();
            }
            return "@".to_string();
        }
        if let Some(h) = pin_name.find('#') {
            if h > 0 {
                return pin_name[..h].to_string();
            }
            return "#".to_string();
        }
        if let Some(u) = pin_name.rfind('_') {
            if u > 0 && u < pin_name.len() - 1 {
                let suffix = &pin_name[u + 1..];
                if suffix.chars().all(|c| c.is_ascii_digit()) {
                    return pin_name[..u].to_string();
                }
            }
        }
        if let Some(h) = pin_name.rfind('-') {
            if h > 0 && h < pin_name.len() - 1 {
                let suffix = &pin_name[h + 1..];
                if suffix.chars().all(|c| c.is_ascii_digit()) {
                    return pin_name[..h].to_string();
                }
            }
        }
        pin_name.to_string()
    }
}

impl PolylineTrace {
    #[inline]
    pub fn first_corner(&self) -> Point {
        self.polyline.corner(0)
    }

    #[inline]
    pub fn last_corner(&self) -> Point {
        self.polyline.corner(self.polyline.line_count().saturating_sub(2))
    }

    #[inline]
    pub fn first_corner_ref(&self) -> &Point {
        self.polyline.corner_ref(0)
    }

    #[inline]
    pub fn last_corner_ref(&self) -> &Point {
        self.polyline.corner_ref(self.polyline.line_count().saturating_sub(2))
    }

    #[inline]
    pub fn corner_count(&self) -> usize {
        self.polyline.line_count().saturating_sub(1)
    }

    #[inline]
    pub fn tile_shape_count(&self) -> usize {
        self.polyline.line_count().saturating_sub(2)
    }

    pub fn get_length(&self) -> f64 {
        self.polyline.length_approx()
    }

    pub fn bounding_box(&self) -> IntBox {
        self.polyline.bounding_box().offset(self.half_width as f64)
    }

    /// Returns true, if `p` is an end corner or lies on the trace.
    pub fn contains(&self, p: &Point) -> bool {
        if p == self.first_corner_ref() || p == self.last_corner_ref() {
            return true;
        }
        if !p.is_int() {
            return false;
        }
        if !p.is_contained_in(&self.bounding_box()) {
            return false;
        }
        self.polyline.contains(p)
    }

    /// Shape used for the connection with the autorouter.
    pub fn connection_shape(&self, index: usize) -> TileShape {
        let seg = crate::geometry::LineSegment::from_polyline(&self.polyline, index + 1);
        TileShape::Simplex(seg.to_simplex()).simplify()
    }

    pub fn contact_bounds(&self) -> IntOctagon {
        self.polyline
            .bounding_octagon_range(0, self.corner_count() as i32 - 1)
            .offset(self.half_width as f64)
    }
}

impl ObstacleArea {
    pub fn is_filled(&self) -> bool {
        match self.kind {
            ObstacleKind::Conduction { is_filled, .. } => is_filled,
            _ => false,
        }
    }

    pub fn conduction_is_obstacle(&self) -> bool {
        match self.kind {
            ObstacleKind::Conduction { is_obstacle, .. } => is_obstacle,
            _ => false,
        }
    }
}
