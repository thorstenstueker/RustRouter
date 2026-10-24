//! Construction and insertion of items (the `insert*` methods of `BasicBoard`).

use super::item::{
    BoardOutline, ComponentOutline, Drill, Item, ItemId, ItemKind, NetNumbers, ObstacleArea,
    ObstacleKind, Pin, PolylineTrace, Via,
};
use super::{Board, FixedState};
use crate::geometry::{Area, FloatPoint, IntPoint, Point, Polyline, Shape, Vector};
use crate::library::PadstackId;

impl Board {
    fn new_item(&mut self, nets: &[i32], cl: i32, component_id: i32, fixed: FixedState, kind: ItemKind) -> Item {
        Item {
            id: self.new_item_id(),
            net_numbers: NetNumbers::from_slice(nets),
            clearance_class: cl,
            component_id,
            fixed_state: fixed,
            on_the_board: false,
            smallest_clearance: -1.0,
            kind,
        }
    }

    /// Calculates the drill geometry of a pin of a component.
    pub fn calculate_pin_drill(&self, component_id: i32, pin_index: i32) -> Option<Drill> {
        let comp = self.components.try_get(component_id)?;
        let pkg = self.library.packages.get(comp.get_package());
        let package_pin = pkg.get_pin(pin_index as usize)?;
        let ps_id = package_pin.padstack_id;
        let ps = self.library.padstacks.try_get(ps_id)?;
        let on_front = comp.placed_on_front();
        let (first_layer, last_layer) = if on_front || ps.placed_absolute {
            (ps.from_layer(), ps.to_layer())
        } else {
            (
                ps.board_layer_count() - ps.to_layer() - 1,
                ps.board_layer_count() - ps.from_layer() - 1,
            )
        };
        let shape_count = (ps.to_layer() - ps.from_layer() + 1).max(0) as usize;
        let transform = comp.placement_transform(self.components.get_flip_style_rotate_first());
        let mut shapes = Vec::with_capacity(shape_count);
        for i in 0..shape_count {
            let padstack_layer = if on_front || ps.placed_absolute {
                i as i32 + first_layer
            } else {
                ps.board_layer_count() - i as i32 - first_layer - 1
            };
            let s = ps.get_shape(padstack_layer).map(|s| {
                transform.shape(s, &package_pin.get_exact_relative_location(), package_pin.rotation_in_degree)
            });
            shapes.push(s);
        }
        let mut center = Point::Int(transform.point(&package_pin.get_exact_relative_location()).round());
        match shapes.iter().flatten().next() {
            None => log::warn!("Pin: At least 1 shape != null expected"),
            Some(s) => {
                if !s.contains_inside(&center) {
                    center = Point::Int(s.centre_of_gravity().round());
                }
            }
        }
        Some(Drill {
            center,
            padstack: ps_id,
            first_layer,
            last_layer,
            shapes,
            tile_shape_count: shape_count,
        })
    }

    /// Calculates the drill geometry of a via.
    pub fn calculate_via_drill(&self, padstack: PadstackId, center: &Point) -> Drill {
        let ps = self.library.padstacks.get(padstack);
        let first_layer = ps.from_layer();
        let last_layer = ps.to_layer();
        let shape_count = (ps.to_layer() - ps.from_layer() + 1).max(0) as usize;
        let tv = center.difference_by(&Point::ZERO);
        let shapes = (0..shape_count)
            .map(|i| ps.get_shape(i as i32 + first_layer).map(|s| s.translate_by(&tv)))
            .collect();
        Drill {
            center: center.clone(),
            padstack,
            first_layer,
            last_layer,
            shapes,
            tile_shape_count: shape_count,
        }
    }

    /// Calculates the absolute area of an obstacle area or component outline.
    pub fn calculate_absolute_area(
        &self,
        relative: &Area,
        component_id: i32,
        translation: &Vector,
        rotation: f64,
        side_changed: bool,
    ) -> Area {
        if component_id > 0 {
            if let Some(comp) = self.components.try_get(component_id) {
                return comp
                    .placement_transform(self.components.get_flip_style_rotate_first())
                    .area(relative);
            }
        }
        let flip = self.components.get_flip_style_rotate_first();
        let mut turned = relative.clone();
        if side_changed && !flip {
            turned = turned.mirror_vertical(&IntPoint::ZERO);
        }
        if rotation != 0.0 {
            if rotation % 90.0 == 0.0 {
                turned = turned.turn_90_degree(rotation as i32 / 90, &IntPoint::ZERO);
            } else {
                turned = turned.rotate_approx(rotation.to_radians(), &FloatPoint::ZERO);
            }
        }
        if side_changed && flip {
            turned = turned.mirror_vertical(&IntPoint::ZERO);
        }
        turned.translate_by(translation)
    }

    pub fn insert_pin(&mut self, component_id: i32, pin_index: i32, nets: &[i32], cl: i32, fixed: FixedState) -> Option<ItemId> {
        let drill = self.calculate_pin_drill(component_id, pin_index)?;
        let mut item = self.new_item(
            nets,
            cl,
            component_id,
            fixed,
            ItemKind::Pin(Pin {
                pin_index,
                drill,
                changed_to: ItemId(0),
            }),
        );
        if let ItemKind::Pin(p) = &mut item.kind {
            p.changed_to = item.id;
        }
        Some(self.insert_item(item))
    }

    /// Inserts a via and splits the traces of its nets at the via center.
    pub fn insert_via(
        &mut self,
        padstack: PadstackId,
        center: &Point,
        nets: &[i32],
        cl: i32,
        fixed: FixedState,
        attach_allowed: bool,
    ) -> ItemId {
        self.insert_via_impl(padstack, center, nets, cl, fixed, attach_allowed, false, -1)
    }

    pub fn insert_escape_via(
        &mut self,
        padstack: PadstackId,
        center: &Point,
        nets: &[i32],
        cl: i32,
        fixed: FixedState,
        smd_layer: i32,
    ) -> ItemId {
        self.insert_via_impl(padstack, center, nets, cl, fixed, true, true, smd_layer)
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_via_impl(
        &mut self,
        padstack: PadstackId,
        center: &Point,
        nets: &[i32],
        cl: i32,
        fixed: FixedState,
        attach_allowed: bool,
        is_escape_via: bool,
        smd_layer: i32,
    ) -> ItemId {
        let drill = self.calculate_via_drill(padstack, center);
        let item = self.new_item(
            nets,
            cl,
            0,
            fixed,
            ItemKind::Via(Via {
                drill,
                attach_allowed,
                is_escape_via,
                escape_via_smd_layer: smd_layer,
            }),
        );
        let id = self.insert_item(item);
        let ps = self.library.padstacks.get(padstack);
        let (from, to) = (ps.from_layer(), ps.to_layer());
        for layer in from..=to {
            for n in nets {
                self.split_traces(center, layer, *n);
            }
        }
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_obstacle_area(
        &mut self,
        kind: ObstacleKind,
        area: Area,
        layer: i32,
        translation: Vector,
        rotation: f64,
        side_changed: bool,
        nets: &[i32],
        cl: i32,
        component_id: i32,
        name: Option<String>,
        fixed: FixedState,
    ) -> ItemId {
        let abs = self.calculate_absolute_area(&area, component_id, &translation, rotation, side_changed);
        let convex = abs.split_to_convex();
        let item = self.new_item(
            nets,
            cl,
            component_id,
            fixed,
            ItemKind::Obstacle(ObstacleArea {
                kind,
                name,
                relative_area: area,
                layer,
                translation,
                rotation_in_degree: rotation,
                side_changed,
                area: abs,
                convex_pieces: convex,
            }),
        );
        self.insert_item(item)
    }

    pub fn insert_obstacle(&mut self, area: Area, layer: i32, cl: i32, fixed: FixedState) -> ItemId {
        self.insert_obstacle_area(
            ObstacleKind::Obstacle,
            area,
            layer,
            Vector::ZERO,
            0.0,
            false,
            &[],
            cl,
            0,
            None,
            fixed,
        )
    }

    pub fn insert_conduction_area(
        &mut self,
        area: Area,
        layer: i32,
        nets: &[i32],
        cl: i32,
        is_obstacle: bool,
        fixed: FixedState,
    ) -> ItemId {
        self.insert_obstacle_area(
            ObstacleKind::Conduction {
                is_obstacle,
                is_filled: true,
            },
            area,
            layer,
            Vector::ZERO,
            0.0,
            false,
            nets,
            cl,
            0,
            None,
            fixed,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_component_outline(
        &mut self,
        area: Area,
        is_front: bool,
        translation: Vector,
        rotation: f64,
        component_id: i32,
        is_courtyard: bool,
        is_fabrication: bool,
        is_closed: bool,
        fixed: FixedState,
    ) -> Option<ItemId> {
        if !area.is_bounded() {
            log::warn!("BasicBoard.insert_component_outline: area is not bounded");
            return None;
        }
        let abs = self.calculate_absolute_area(&area, component_id, &translation, rotation, !is_front);
        let item = self.new_item(
            &[],
            0,
            component_id,
            fixed,
            ItemKind::ComponentOutline(ComponentOutline {
                relative_area: area,
                area: abs,
                translation,
                rotation_in_degree: rotation,
                is_front,
                is_courtyard,
                is_fabrication,
                is_closed,
            }),
        );
        Some(self.insert_item(item))
    }

    pub fn insert_outline(&mut self, shapes: Vec<Shape>, cl: i32) -> ItemId {
        let item = self.new_item(
            &[],
            cl,
            0,
            FixedState::SystemFixed,
            ItemKind::BoardOutline(BoardOutline {
                shapes,
                keepout_outside_outline: false,
                edge_pin_nets_cross_outline: true,
            }),
        );
        self.insert_item(item)
    }

    /// Switches the generation of a keepout outside the board outline.
    pub fn generate_outline_keepout_outside(&mut self, value: bool) {
        let Some(id) = self.items.outline_id() else { return };
        let changed = match &mut self.item_mut(id).kind {
            ItemKind::BoardOutline(o) => {
                if o.keepout_outside_outline == value {
                    false
                } else {
                    o.keepout_outside_outline = value;
                    true
                }
            }
            _ => false,
        };
        if changed {
            self.reinsert_into_trees(id);
        }
    }

    /// Inserts a trace without normalization. Returns None for degenerated traces.
    pub fn insert_trace_without_cleaning(
        &mut self,
        polyline: Polyline,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        cl: i32,
        fixed: FixedState,
    ) -> Option<ItemId> {
        if polyline.corner_count() < 2 {
            return None;
        }
        let layer = layer.max(0).min(self.layer_count() - 1);
        let trace = PolylineTrace {
            polyline,
            layer,
            half_width,
        };
        if trace.first_corner() == trace.last_corner() && fixed < FixedState::UserFixed {
            // The original creates the trace object (consuming an id) before this check.
            self.new_item_id();
            return None;
        }
        let item = self.new_item(nets, cl, 0, fixed, ItemKind::Trace(trace));
        let normal = item.nets_normal();
        let id = self.insert_item(item);
        if normal {
            self.note_trace_half_width(half_width);
        }
        Some(id)
    }

    /// Inserts a trace and normalizes it (`insertTrace` of the original).
    pub fn insert_trace(
        &mut self,
        polyline: Polyline,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        cl: i32,
        fixed: FixedState,
    ) {
        let Some(id) = self.insert_trace_without_cleaning(polyline, layer, half_width, nets, cl, fixed) else {
            return;
        };
        let clip = self.changed_area.as_ref().map(|c| c.get_area(layer));
        self.normalize_trace(id, clip.as_ref());
    }

    pub fn insert_trace_points(
        &mut self,
        points: &[Point],
        layer: i32,
        half_width: i32,
        nets: &[i32],
        cl: i32,
        fixed: FixedState,
    ) {
        for p in points {
            if !self.contains_point(p) {
                log::warn!("LayeredBoard.insert_trace: input point out of range");
            }
        }
        let pl = Polyline::from_points(points);
        self.insert_trace(pl, layer, half_width, nets, cl, fixed);
    }

    /// Creates a detached trace item, which is not inserted into the board
    /// (used for checks of new traces).
    pub fn detached_trace(&self, polyline: Polyline, layer: i32, half_width: i32, nets: &[i32], cl: i32) -> Item {
        Item {
            id: ItemId(0),
            net_numbers: NetNumbers::from_slice(nets),
            clearance_class: cl,
            component_id: 0,
            fixed_state: FixedState::Unfixed,
            on_the_board: false,
            smallest_clearance: -1.0,
            kind: ItemKind::Trace(PolylineTrace {
                polyline,
                layer,
                half_width,
            }),
        }
    }

    /// Converts an obstacle area into a conduction area of a net (`makeConductive`).
    pub fn make_conductive(&mut self, id: ItemId, net_no: i32) -> ItemId {
        let item = self.item(id).clone();
        let Some(o) = item.as_obstacle() else { return id };
        let o = o.clone();
        self.remove_item(id);
        let new_item = Item {
            id: self.new_item_id(),
            net_numbers: NetNumbers::from_slice(&[net_no]),
            clearance_class: item.clearance_class,
            component_id: item.component_id,
            fixed_state: item.fixed_state,
            on_the_board: false,
            smallest_clearance: -1.0,
            kind: ItemKind::Obstacle(ObstacleArea {
                kind: ObstacleKind::Conduction {
                    is_obstacle: true,
                    is_filled: true,
                },
                ..o
            }),
        };
        self.insert_item(new_item)
    }
}
