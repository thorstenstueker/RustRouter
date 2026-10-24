//! Connections of traces to pins with exit restrictions (`checkConnectionToPin`,
//! `correctConnectionToPin`, `swapConnectionToPin` of `PolylineTrace`).

use super::item::ItemId;
use super::{Board, FixedState};
use crate::geometry::{Direction, FloatPoint, Line, Polyline, Shape, Signum, TileShape};
use crate::rules::AngleRestriction;

impl Board {
    fn contact_pin(&self, contacts: &std::collections::BTreeSet<ItemId>) -> Option<ItemId> {
        contacts.iter().copied().find(|c| self.item(*c).is_pin())
    }

    pub fn check_connection_to_pin(&self, id: ItemId, at_start: bool) -> bool {
        let t = self.item(id).as_trace().unwrap();
        if t.corner_count() < 2 {
            return true;
        }
        let contacts = if at_start {
            self.trace_start_contacts(id)
        } else {
            self.trace_end_contacts(id)
        };
        let Some(pin) = self.contact_pin(&contacts) else {
            return true;
        };
        let restrictions = self.pin_trace_exit_restrictions(pin, t.layer);
        if restrictions.is_empty() {
            return true;
        }
        let (end_corner, prev_end_corner) = if at_start {
            (t.first_corner(), t.polyline.corner(1))
        } else {
            (t.last_corner(), t.polyline.corner(t.polyline.corner_count() - 2))
        };
        let Some(end_dir) = Direction::from_points(&end_corner, &prev_end_corner) else {
            return true;
        };
        let Some(matching) = restrictions.iter().find(|r| r.direction == end_dir) else {
            return false;
        };
        let edge = self.rules.get_pin_edge_to_turn_dist();
        if edge < 0.0 {
            return false;
        }
        let end_line_length = end_corner.to_float().distance(&prev_end_corner.to_float());
        let clearance = self.clearance_value(self.item(id).clearance_class, self.item(pin).clearance_class, t.layer) as f64;
        let add_width = edge.max(clearance + 1.0);
        let preserve = matching.min_length + t.half_width as f64 + add_width;
        preserve <= end_line_length
    }

    pub fn correct_connection_to_pin(&mut self, id: ItemId, at_start: bool, ar: AngleRestriction) -> bool {
        if self.check_connection_to_pin(id, at_start) {
            return false;
        }
        let item = self.item(id).clone();
        let t = item.as_trace().unwrap().clone();
        let (trace_polyline, contacts) = if at_start {
            (t.polyline.clone(), self.trace_start_contacts(id))
        } else {
            (t.polyline.reverse(), self.trace_end_contacts(id))
        };
        let Some(pin) = self.contact_pin(&contacts) else {
            return false;
        };
        let restrictions = self.pin_trace_exit_restrictions(pin, t.layer);
        if restrictions.is_empty() {
            return false;
        }
        let pd = self.item(pin).as_drill().unwrap();
        let Some(Shape::Tile(pin_shape)) = pd.get_shape((t.layer - pd.first_layer) as usize).cloned() else {
            return false;
        };
        let edge = self.rules.get_pin_edge_to_turn_dist();
        if edge < 0.0 {
            return false;
        }
        let clearance = self.clearance_value(item.clearance_class, self.item(pin).clearance_class, t.layer) as f64;
        let add_width = edge.max(clearance + 1.0);
        let mut offset_pin_shape = pin_shape.offset(t.half_width as f64 + add_width);
        if ar == AngleRestriction::NinetyDegree || offset_pin_shape.is_int_box() {
            offset_pin_shape = TileShape::Box(offset_pin_shape.bounding_box());
        } else if ar == AngleRestriction::FortyfiveDegree {
            offset_pin_shape = TileShape::Octagon(offset_pin_shape.bounding_octagon());
        }
        let entries = offset_pin_shape.entrance_points(&trace_polyline);
        let Some(&latest) = entries.last() else {
            return false;
        };
        let entry_location = trace_polyline
            .line(latest.0)
            .intersection_approx(&offset_pin_shape.border_line(latest.1));
        let mut min_dist = f64::MAX;
        let mut nearest_ray: Option<Line> = None;
        let mut nearest_border_no: i32 = -1;
        let mut pin_exit_dir: Option<Direction> = None;
        let mut nearest_exit_corner: Option<FloatPoint> = None;
        let tol = 1.0;
        let pin_center = pd.center.clone();
        for r in &restrictions {
            let bno = offset_pin_shape.intersecting_border_line_no(&pin_center, &r.direction);
            if bno < 0 {
                continue;
            }
            let ray = Line::from_point_direction(&pin_center, &r.direction);
            let ec = ray.intersection_approx(&offset_pin_shape.border_line(bno as usize));
            let d = ec.distance_square(&entry_location);
            let mut new_nearest = false;
            if d + tol < min_dist {
                new_nearest = true;
            } else if d < min_dist + tol {
                if let Some(nec) = nearest_exit_corner {
                    for i in 1..trace_polyline.corner_count() {
                        let tc = trace_polyline.corner_approx(i);
                        let cur = tc.distance_square(&ec);
                        let old = tc.distance_square(&nec);
                        if cur + tol < old {
                            new_nearest = true;
                            break;
                        } else if cur > old + tol {
                            break;
                        }
                    }
                }
            }
            if new_nearest {
                min_dist = d;
                nearest_ray = Some(ray);
                nearest_border_no = bno;
                pin_exit_dir = Some(r.direction.clone());
                nearest_exit_corner = Some(ec);
            }
        }
        let (Some(ray), Some(exit_dir)) = (nearest_ray, pin_exit_dir) else {
            return false;
        };
        let nbn = nearest_border_no as usize;
        let corner_count = offset_pin_shape.border_line_count();
        let cw = (nbn + corner_count - latest.1) % corner_count;
        let ccw = (latest.1 + corner_count - nbn) % corner_count;
        let mut current_border_no = nbn;
        let mut current_lines: Vec<Line> = Vec::new();
        current_lines.push(ray);
        if ccw <= cw {
            for _ in 0..=ccw {
                current_lines.push(offset_pin_shape.border_line(current_border_no));
                current_border_no = (current_border_no + 1) % corner_count;
            }
        } else {
            for _ in 0..=cw {
                current_lines.push(offset_pin_shape.border_line(current_border_no));
                current_border_no = (current_border_no + corner_count - 1) % corner_count;
            }
        }
        current_lines.push(trace_polyline.line(latest.0));
        let border_polyline = Polyline::from_lines(&current_lines);
        if !self.check_polyline_trace(&border_polyline, t.layer, t.half_width, &item.net_numbers, item.clearance_class) {
            return false;
        }
        let mut cut_lines: Vec<Line> = vec![current_lines[current_lines.len() - 2]];
        cut_lines.extend_from_slice(&trace_polyline.lines()[latest.0..]);
        let cut_polyline = Polyline::from_lines(&cut_lines);
        let mut changed = if cut_polyline.first_corner() == cut_polyline.last_corner() {
            border_polyline
        } else {
            border_polyline.combine(&cut_polyline)
        };
        if !at_start {
            changed = changed.reverse();
        }
        self.change_trace(id, changed);
        let exit_segment = Polyline::from_lines(&[
            Line::from_point_direction(&pin_center, &exit_dir.turn_45_degree(2)),
            ray,
            offset_pin_shape.border_line(nbn),
        ]);
        self.insert_trace(
            exit_segment,
            t.layer,
            t.half_width,
            &item.net_numbers,
            item.clearance_class,
            FixedState::ShoveFixed,
        );
        true
    }

    pub fn swap_connection_to_pin(&mut self, id: ItemId, at_start: bool) -> bool {
        let item = self.item(id).clone();
        let t = item.as_trace().unwrap();
        let (trace_polyline, contacts) = if at_start {
            (t.polyline.clone(), self.trace_start_contacts(id))
        } else {
            (t.polyline.reverse(), self.trace_end_contacts(id))
        };
        if contacts.len() != 1 {
            return false;
        }
        let contact = *contacts.iter().next().unwrap();
        let ci = self.item(contact);
        let Some(ct) = ci.as_trace() else { return false };
        if ci.fixed_state != FixedState::ShoveFixed {
            return false;
        }
        let contact_polyline = ct.polyline.clone();
        if contact_polyline.last_corner() != trace_polyline.first_corner() {
            return false;
        }
        let contact_last_line = contact_polyline.line(contact_polyline.line_count() - 2);
        let first_line = trace_polyline.line(1);
        let cld = Direction::Int(contact_last_line.direction());
        let mut check_swap = cld.projection(&Direction::Int(first_line.direction())) == Signum::Negative;
        if !check_swap {
            let hw = t.half_width as f64;
            if trace_polyline.line_count() > 3
                && trace_polyline.corner_approx(0).distance_square(&trace_polyline.corner_approx(1)) <= hw * hw
            {
                check_swap = cld.projection(&Direction::Int(trace_polyline.line(2).direction())) == Signum::Negative;
            }
        }
        if !check_swap {
            return false;
        }
        let Some(pin) = self.contact_pin(&self.trace_start_contacts(contact)) else {
            return false;
        };
        let combined = contact_polyline.combine(&trace_polyline);
        let Some(nearest) = self.pin_calc_nearest_exit_restriction_direction(pin, &combined, t.half_width, t.layer) else {
            return false;
        };
        if nearest == Direction::Int(contact_polyline.line(1).direction()) {
            return false;
        }
        self.item_mut(contact).fixed_state = item.fixed_state;
        self.combine_trace(id);
        true
    }
}
