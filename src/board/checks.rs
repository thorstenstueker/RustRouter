//! Clearance checks of new shapes and traces (`checkShape`, `checkTraceShape`,
//! `checkPolylineTrace`, `checkTraceSegment`, … of `BasicBoard`/`RoutingBoard`).

use super::item::{Item, ItemId, ItemKind};
use super::{Board, TreeEntry, TreeObject};
use crate::geometry::{Area, FloatPoint, LineSegment, Point, Polyline, TileShape, Vector};
use std::collections::BTreeSet;

impl Board {
    /// Checks, if an area can be inserted without clearance violations.
    pub fn check_shape(&self, shape: &Area, layer: i32, nets: &[i32], clearance_class: i32) -> bool {
        let tiles = shape.split_to_convex();
        let tree = self.search_trees.default_tree();
        let lookup = self.lookup();
        for tile in tiles {
            if !tile.is_contained_in(&self.bounding_box) {
                return false;
            }
            let comp = tree.clearance_compensation_value(&self.rules, clearance_class, layer);
            let query = if comp == 0 { tile.clone() } else { tile.enlarge(comp as f64) };
            let mut entries = Vec::new();
            tree.overlapping_tree_entries_with_clearance(
                &query,
                layer,
                nets,
                clearance_class,
                None,
                &self.rules,
                self.layer_count(),
                &self.bounding_box,
                &lookup,
                &mut entries,
            );
            let mut objs: Vec<TreeObject> = entries.into_iter().map(|e| e.object).collect();
            objs.sort();
            objs.dedup();
            for o in objs {
                let mut is_obstacle = true;
                for n in nets {
                    let ob = match o {
                        TreeObject::Item(id) => lookup.items.get(id).unwrap().is_obstacle_net_basic(*n),
                        TreeObject::Room(..) => true,
                    };
                    if !ob {
                        is_obstacle = false;
                    }
                }
                if is_obstacle {
                    return false;
                }
            }
        }
        true
    }

    /// Checks, if a trace shape can be inserted without clearance violations.
    /// `contact_pins` are the pins the trace is connected to.
    pub fn check_trace_shape(
        &self,
        shape: &TileShape,
        layer: i32,
        nets: &[i32],
        clearance_class: i32,
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> bool {
        if !shape.is_contained_in(&self.bounding_box) {
            return false;
        }
        let tree = self.search_trees.default_tree();
        let lookup = self.lookup();
        let mut entries: Vec<TreeEntry> = Vec::new();
        tree.overlapping_tree_entries_with_clearance(
            shape,
            layer,
            &[],
            clearance_class,
            None,
            &self.rules,
            self.layer_count(),
            &self.bounding_box,
            &lookup,
            &mut entries,
        );
        for entry in entries {
            let Some(id) = entry.object.item() else { continue };
            let item = self.item(id);
            if let Some(cp) = contact_pins {
                if cp.contains(&id) {
                    continue;
                }
                if item.is_pin() {
                    return false;
                }
            }
            let mut is_obstacle = true;
            for n in nets {
                if !self.is_trace_obstacle(id, *n) {
                    is_obstacle = false;
                }
            }
            if is_obstacle && item.is_trace() {
                if let Some(cp) = contact_pins {
                    let mut intersection: Option<TileShape> = None;
                    for pin_id in cp {
                        let pin = self.item(*pin_id);
                        if pin.net_count() <= 1 || !pin.shares_net(item) {
                            continue;
                        }
                        if intersection.is_none() {
                            let obstacle_shape = self.tile_shape(id, entry.shape_index).unwrap();
                            intersection = Some(shape.intersection(&obstacle_shape));
                        }
                        if let Some(pin_shape) = self.drill_tile_shape_on_layer(*pin_id, layer) {
                            if pin_shape.contains_approx(intersection.as_ref().unwrap()) {
                                is_obstacle = false;
                                break;
                            }
                        }
                    }
                }
            }
            if is_obstacle {
                return false;
            }
        }
        true
    }

    /// The tile shape of a drill item on a layer (`getTileShapeOnLayer`).
    pub fn drill_tile_shape_on_layer(&self, id: ItemId, layer: i32) -> Option<TileShape> {
        let d = self.item(id).as_drill()?;
        if layer < d.first_layer || layer > d.last_layer {
            log::warn!("DrillItem.get_tile_shape_on_layer: layer out of range");
            return None;
        }
        self.tile_shape(id, (layer - d.first_layer) as usize)
    }

    /// The tree shape of a drill item on a layer (`getTreeShapeOnLayer`).
    pub fn drill_tree_shape_on_layer(&self, tree: super::TreeId, id: ItemId, layer: i32) -> Option<TileShape> {
        let d = self.item(id).as_drill()?;
        if layer < d.first_layer || layer > d.last_layer {
            log::warn!("DrillItem.get_tree_shape_on_layer: layer out of range");
            return None;
        }
        self.tree_shape(tree, id, (layer - d.first_layer) as usize)
    }

    /// Checks, if a trace with the given polyline can be inserted.
    pub fn check_polyline_trace(&mut self, polyline: &Polyline, layer: i32, half_width: i32, nets: &[i32], clearance_class: i32) -> bool {
        // The temporary trace of the original consumes an item id.
        self.new_item_id();
        let tmp = self.detached_trace(polyline.clone(), layer, half_width, nets, clearance_class);
        let contact_pins = self.touching_pins_at_end_corners(&tmp);
        let (shapes, _) = self.calc_shapes_detached(self.search_trees.default_tree_id(), &tmp);
        for s in shapes.into_iter().flatten() {
            if !self.check_trace_shape(&s, layer, nets, clearance_class, Some(&contact_pins)) {
                return false;
            }
        }
        true
    }

    /// Returns the length of the start piece of a trace segment, which can be inserted
    /// without clearance violations (`checkTraceSegment`).
    #[allow(clippy::too_many_arguments)]
    pub fn check_trace_segment_points(
        &mut self,
        from: &Point,
        to: &Point,
        layer: i32,
        nets: &[i32],
        half_width: i32,
        cl_class: i32,
        only_not_shovable_obstacles: bool,
    ) -> f64 {
        if from == to {
            return 0.0;
        }
        let pl = Polyline::from_corners(from, to);
        let seg = LineSegment::from_polyline(&pl, 1);
        self.check_trace_segment(&seg, layer, nets, half_width, cl_class, only_not_shovable_obstacles)
    }

    pub fn check_trace_segment(
        &mut self,
        seg: &LineSegment,
        layer: i32,
        nets: &[i32],
        half_width: i32,
        cl_class: i32,
        only_not_shovable_obstacles: bool,
    ) -> f64 {
        let check_polyline = seg.to_polyline();
        if check_polyline.line_count() != 3 {
            return 0.0;
        }
        let mut tree_id = self.search_trees.default_tree_id();
        {
            let t = self.search_trees.get(tree_id);
            if t.is_clearance_compensation_used() && t.compensated_clearance_class != cl_class {
                tree_id = self.get_uncompensated_tree();
            }
        }
        let tree = self.search_trees.get(tree_id);
        let query_hw = half_width + tree.clearance_compensation_value(&self.rules, cl_class, layer);
        let Some(shape) = check_polyline.offset_shape(query_hw, 0) else {
            return 0.0;
        };
        let from = seg.start_point_approx();
        let to = seg.end_point_approx();
        let line_length = to.distance(&from);
        let mut ok_length = i32::MAX as f64;
        let mut entries = Vec::new();
        tree.overlapping_tree_entries_with_clearance(
            &shape,
            layer,
            nets,
            cl_class,
            None,
            &self.rules,
            self.layer_count(),
            &self.bounding_box,
            &self.lookup(),
            &mut entries,
        );
        for e in entries {
            let Some(id) = e.object.item() else { continue };
            let obstacle = self.item(id);
            if only_not_shovable_obstacles && obstacle.is_routable() && !self.is_shove_fixed(obstacle) {
                continue;
            }
            let Some(obstacle_shape) = tree.object_shape(&e.object, e.shape_index).cloned() else {
                continue;
            };
            let (offset_shape, shorten) = if tree.is_clearance_compensation_used() {
                (shape.clone(), query_hw as f64)
            } else {
                let cv = self.clearance_value(obstacle.clearance_class, cl_class, layer);
                (shape.offset(cv as f64), (half_width + cv) as f64)
            };
            let intersection = obstacle_shape.intersection(&offset_shape);
            if intersection.is_empty() {
                continue;
            }
            let Some(nearest) = intersection.nearest_point_approx(&from) else { continue };
            let mut projection = from.scalar_product(&to, &nearest) / line_length;
            projection = (projection - shorten - 1.0).max(0.0);
            if projection < ok_length {
                ok_length = projection;
                if ok_length <= 0.0 {
                    return 0.0;
                }
            }
        }
        ok_length
    }

    /// Checks, if an item can be moved by `vector` (`checkMoveItem`).
    pub fn check_move_item(&self, id: ItemId, vector: &Vector, ignore: Option<&mut BTreeSet<ItemId>>) -> bool {
        let item = self.item(id);
        if item.net_count() > 1 {
            return false;
        }
        let contact_count = if item.is_connectable_type() {
            self.all_contacts(id, None).len()
        } else {
            0
        };
        if item.is_trace() && contact_count > 0 {
            return false;
        }
        let mut ignore = ignore;
        if let Some(ig) = ignore.as_deref_mut() {
            ig.insert(id);
        }
        for i in 0..self.tile_shape_count(id) {
            let moved = self.tile_shape(id, i).unwrap().translate_by(vector);
            if !moved.is_contained_in(&self.bounding_box) {
                return false;
            }
            let layer = self.shape_layer(id, i);
            for oid in self.overlapping_items_with_clearance(&moved, layer, &item.net_numbers, item.clearance_class) {
                let other = self.item(oid);
                match ignore.as_deref() {
                    Some(ig) => {
                        if !ig.contains(&oid) && self.is_obstacle(other, item) {
                            return false;
                        }
                    }
                    None => {
                        if oid != id && self.is_obstacle(other, item) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    pub fn check_change_net(&self, id: ItemId, new_net: i32) -> bool {
        let item = self.item(id);
        for i in 0..self.tile_shape_count(id) {
            let shape = self.tile_shape(id, i).unwrap();
            let layer = self.shape_layer(id, i);
            for oid in self.overlapping_items_with_clearance(&shape, layer, &[new_net], item.clearance_class) {
                let o = self.item(oid);
                if oid != id && o.is_connectable_type() && !o.contains_net(new_net) {
                    return false;
                }
            }
        }
        true
    }

    /// The nearest connectable item at a location (`pickNearestRoutingItem`).
    pub fn pick_nearest_routing_item(&self, location: &Point, layer: i32, from_item: Option<ItemId>) -> Option<ItemId> {
        let shape = TileShape::from_point(location);
        let found = self.overlapping_items(&shape, layer);
        let pick: FloatPoint = location.to_float();
        let mut min_dist = i32::MAX as f64;
        let mut nearest: Option<ItemId> = None;
        let mut ignore: Option<BTreeSet<ItemId>> = None;
        let lc = self.layer_count();
        for id in found {
            let item = self.item(id);
            if !item.is_connectable() {
                continue;
            }
            let mut candidate = false;
            let mut dist = 0.0;
            let nearest_is_drill = nearest.map(|n| self.item(n).is_drill_item()).unwrap_or(false);
            let nearest_is_trace = nearest.map(|n| self.item(n).is_trace()).unwrap_or(false);
            match &item.kind {
                ItemKind::Trace(t) => {
                    if layer < 0 || t.layer == layer {
                        if nearest_is_drill {
                            continue;
                        }
                        dist = t.polyline.distance(&pick);
                        if dist < min_dist && dist <= t.half_width as f64 {
                            candidate = true;
                        }
                    }
                }
                ItemKind::Pin(_) | ItemKind::Via(_) => {
                    if layer < 0 || item.is_on_layer(layer, lc) {
                        dist = item.as_drill().unwrap().center.to_float().distance(&pick);
                        if dist < min_dist || nearest_is_trace {
                            candidate = true;
                        }
                    }
                }
                ItemKind::Obstacle(o) if item.is_conduction_area() => {
                    if (layer < 0 || o.layer == layer) && nearest.is_none() {
                        candidate = true;
                        dist = i32::MAX as f64;
                    }
                }
                _ => {}
            }
            if candidate {
                if let Some(f) = from_item {
                    if ignore.is_none() {
                        ignore = Some(self.connected_set(f, -1, false));
                    }
                    if ignore.as_ref().unwrap().contains(&id) {
                        continue;
                    }
                }
                min_dist = dist;
                nearest = Some(id);
            }
        }
        nearest
    }

    // ---------------------------------------------------------------- changed area

    pub fn start_marking_changed_area(&mut self) {
        if self.changed_area.is_none() {
            self.changed_area = Some(super::ChangedArea::new(self.layer_count() as usize));
        }
    }

    pub fn mark_all_changed_area(&mut self) {
        self.start_marking_changed_area();
        let b = self.bounding_box;
        let corners = [
            b.ll.to_float(),
            FloatPoint::new(b.ur.x as f64, b.ll.y as f64),
            b.ur.to_float(),
            FloatPoint::new(b.ll.x as f64, b.ur.y as f64),
        ];
        for layer in 0..self.layer_count() {
            for c in &corners {
                self.join_changed_area(c, layer);
            }
        }
    }

    pub fn item_clone(&self, id: ItemId) -> Item {
        self.item(id).clone()
    }
}
