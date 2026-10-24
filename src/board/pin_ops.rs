//! Pin specific board methods (trace exit restrictions, swappable pins).

use super::item::ItemId;
use super::Board;
use crate::geometry::{Direction, FloatPoint, Line, PlacementTransform, Polyline, Shape, TileShape};
use std::collections::BTreeSet;

/// A restriction of the direction, in which a trace may leave a pin.
#[derive(Clone, Debug)]
pub struct TraceExitRestriction {
    pub direction: Direction,
    pub min_length: f64,
}

impl Board {
    /// The padstack layer of the shape with index `index` of a pin.
    pub fn pin_padstack_layer(&self, pin_id: ItemId, index: i32) -> i32 {
        let item = self.item(pin_id);
        let d = item.as_drill().unwrap();
        let ps = self.library.padstacks.get(d.padstack);
        let comp = self.components.get(item.component_id);
        if comp.placed_on_front() || ps.placed_absolute {
            index + d.first_layer
        } else {
            ps.board_layer_count() - index - d.first_layer - 1
        }
    }

    pub fn pin_trace_exit_restrictions(&self, pin_id: ItemId, layer: i32) -> Vec<TraceExitRestriction> {
        let mut result = Vec::new();
        let item = self.item(pin_id);
        let pin = item.as_pin().unwrap();
        let padstack_layer = self.pin_padstack_layer(pin_id, layer - pin.drill.first_layer);
        let mut pad_xy_factor = 1.5;
        let comp = self.components.try_get(item.component_id);
        if let Some(c) = comp {
            if self.library.packages.get(c.get_package()).pin_count() <= 3 {
                pad_xy_factor *= 2.0;
            }
        }
        let ps = self.library.padstacks.get(pin.drill.padstack);
        let dirs = ps.get_trace_exit_directions(padstack_layer, pad_xy_factor);
        if dirs.is_empty() {
            return result;
        }
        let Some(comp) = comp else { return result };
        let Some(Shape::Tile(pad_shape)) = pin.drill.get_shape((layer - pin.drill.first_layer) as usize) else {
            return result;
        };
        let center = &pin.drill.center;
        let center_approx = center.to_float();
        let pkg = self.library.packages.get(comp.get_package());
        let Some(package_pin) = pkg.get_pin(pin.pin_index as usize) else {
            return result;
        };
        let transform = comp.placement_transform(self.components.get_flip_style_rotate_first());
        for d in dirs {
            let local = PlacementTransform::rotate(&d.get_vector().to_float(), package_pin.rotation_in_degree);
            let world = transform.direction(&local);
            let exit_dir = Direction::get_instance_approx(world.y.atan2(world.x));
            let border_no = pad_shape.intersecting_border_line_no(center, &exit_dir);
            if border_no < 0 {
                log::warn!("Pin.get_trace_exit_restrictions: border line not found");
                continue;
            }
            let exit_line = Line::from_point_direction(center, &exit_dir);
            let nearest = exit_line.intersection_approx(&pad_shape.border_line(border_no as usize));
            result.push(TraceExitRestriction {
                direction: exit_dir,
                min_length: center_approx.distance(&nearest),
            });
        }
        result
    }

    pub fn pin_has_trace_exit_restrictions(&self, pin_id: ItemId) -> bool {
        let d = self.item(pin_id).as_drill().unwrap();
        (d.first_layer..=d.last_layer).any(|l| !self.pin_trace_exit_restrictions(pin_id, l).is_empty())
    }

    fn pin_offset_shape(&self, pin_id: ItemId, layer: i32, trace_half_width: i32) -> Option<TileShape> {
        let d = self.item(pin_id).as_drill().unwrap();
        let Some(Shape::Tile(pin_shape)) = d.get_shape((layer - d.first_layer) as usize) else {
            return None;
        };
        let edge = self.rules.get_pin_edge_to_turn_dist();
        if edge < 0.0 {
            return None;
        }
        Some(pin_shape.offset(edge + trace_half_width as f64))
    }

    /// `Pin.calcNearestExitRestrictionDirection`.
    pub fn pin_calc_nearest_exit_restriction_direction(
        &self,
        pin_id: ItemId,
        trace_polyline: &Polyline,
        trace_half_width: i32,
        layer: i32,
    ) -> Option<Direction> {
        let restrictions = self.pin_trace_exit_restrictions(pin_id, layer);
        if restrictions.is_empty() {
            return None;
        }
        let offset_pin_shape = self.pin_offset_shape(pin_id, layer, trace_half_width)?;
        let entries = offset_pin_shape.entrance_points(trace_polyline);
        let latest = *entries.last()?;
        let entry_location = trace_polyline
            .line(latest.0)
            .intersection_approx(&offset_pin_shape.border_line(latest.1));
        let mut min_dist = f64::MAX;
        let mut nearest_exit_corner: Option<FloatPoint> = None;
        let mut result = None;
        let tolerance = 1.0;
        let center = self.item(pin_id).as_drill().unwrap().center.clone();
        for r in restrictions {
            let bno = offset_pin_shape.intersecting_border_line_no(&center, &r.direction);
            if bno < 0 {
                continue;
            }
            let ray = Line::from_point_direction(&center, &r.direction);
            let exit_corner = ray.intersection_approx(&offset_pin_shape.border_line(bno as usize));
            let d = exit_corner.distance_square(&entry_location);
            let mut new_nearest = false;
            if d + tolerance < min_dist {
                new_nearest = true;
            } else if d < min_dist + tolerance {
                if let Some(nec) = nearest_exit_corner {
                    for i in 1..trace_polyline.corner_count() {
                        let tc = trace_polyline.corner_approx(i);
                        let cur = tc.distance_square(&exit_corner);
                        let old = tc.distance_square(&nec);
                        if cur + tolerance < old {
                            new_nearest = true;
                            break;
                        } else if cur > old + tolerance {
                            break;
                        }
                    }
                }
            }
            if new_nearest {
                min_dist = d;
                result = Some(r.direction.clone());
                nearest_exit_corner = Some(exit_corner);
            }
        }
        result
    }

    /// `Pin.nearestTraceExitCorner`.
    pub fn pin_nearest_trace_exit_corner(&self, pin_id: ItemId, from: &FloatPoint, trace_half_width: i32, layer: i32) -> Option<FloatPoint> {
        let restrictions = self.pin_trace_exit_restrictions(pin_id, layer);
        if restrictions.is_empty() {
            return None;
        }
        let offset_pin_shape = self.pin_offset_shape(pin_id, layer, trace_half_width)?;
        let center = self.item(pin_id).as_drill().unwrap().center.clone();
        let mut min_dist = f64::MAX;
        let mut nearest = None;
        for r in restrictions {
            let bno = offset_pin_shape.intersecting_border_line_no(&center, &r.direction);
            if bno < 0 {
                continue;
            }
            let ray = Line::from_point_direction(&center, &r.direction);
            let c = ray.intersection_approx(&offset_pin_shape.border_line(bno as usize));
            let d = c.distance_square(from);
            if d < min_dist {
                min_dist = d;
                nearest = Some(c);
            }
        }
        nearest
    }

    pub fn pin_min_width(&self, pin_id: ItemId, layer: i32) -> f64 {
        let d = self.item(pin_id).as_drill().unwrap();
        let pl = self.pin_padstack_layer(pin_id, layer - d.first_layer);
        match self.library.padstacks.get(d.padstack).get_shape(pl) {
            Some(s) => s.bounding_box().min_width(),
            None => {
                log::warn!("Pin.get_min_width: padstackShape is null");
                0.0
            }
        }
    }

    pub fn pin_max_width(&self, pin_id: ItemId, layer: i32) -> f64 {
        let d = self.item(pin_id).as_drill().unwrap();
        let pl = self.pin_padstack_layer(pin_id, layer - d.first_layer);
        match self.library.padstacks.get(d.padstack).get_shape(pl) {
            Some(s) => s.bounding_box().max_width(),
            None => 0.0,
        }
    }

    pub fn pin_trace_neckdown_halfwidth(&self, pin_id: ItemId, layer: i32) -> i32 {
        (0.5 * self.pin_min_width(pin_id, layer) - 1.0).max(1.0) as i32
    }

    /// The name of a pin (package pin name).
    pub fn pin_name(&self, pin_id: ItemId) -> Option<String> {
        let item = self.item(pin_id);
        let comp = self.components.try_get(item.component_id)?;
        let pkg = self.library.packages.get(comp.get_package());
        pkg.get_pin(item.as_pin()?.pin_index as usize).map(|p| p.name.clone())
    }

    /// The pins of the same gate, which can be swapped with this pin.
    pub fn pin_swappable_pins(&self, pin_id: ItemId) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let item = self.item(pin_id);
        let Some(comp) = self.components.try_get(item.component_id) else {
            return result;
        };
        let Some(lp) = comp.get_logical_part() else { return result };
        let lp = self.library.logical_parts.get(lp);
        let pin_index = item.as_pin().unwrap().pin_index;
        let Some(this_pin) = lp.get_pin(pin_index as usize) else {
            return result;
        };
        if this_pin.gate_pin_swap_code <= 0 {
            return result;
        }
        for i in 0..lp.pin_count() {
            if i as i32 == pin_index {
                continue;
            }
            if let Some(p) = lp.get_pin(i) {
                if p.gate_pin_swap_code == this_pin.gate_pin_swap_code && p.gate_name == this_pin.gate_name {
                    match self.get_pin(item.component_id, p.pin_index) {
                        Some(s) => {
                            result.insert(s);
                        }
                        None => log::warn!("Pin.get_swappable_pins: swappable pin not found"),
                    }
                }
            }
        }
        result
    }
}
