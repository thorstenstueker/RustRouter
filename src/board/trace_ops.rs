//! Combining, splitting and normalizing of traces (port of `PolylineTrace` and the
//! trace methods of `BasicBoard`).

use crate::board::{Item, TreeEntry};
use super::item::{ItemId, ItemKind};
use super::{Board, FixedState, TreeObject};
use crate::geometry::{Direction, IntOctagon, Line, LineSegment, Point, Polyline, TileShape};
use std::collections::BTreeSet;

const MAX_NORMALIZATION_DEPTH: i32 = 16;
const MAX_NORMALIZE_ITERATIONS: i32 = 2000;

impl Board {
    fn trace(&self, id: ItemId) -> &super::item::PolylineTrace {
        self.item(id).as_trace().expect("trace expected")
    }

    /// Replaces the polyline of a trace and reinserts it into the search trees.
    fn replace_trace_geometry(&mut self, id: ItemId, polyline: Polyline) {
        self.remove_from_trees(id);
        self.item_mut(id).as_trace_mut().unwrap().polyline = polyline;
        self.insert_into_trees(id);
    }

    /// Combines the trace with other traces at its ends (`PolylineTrace.combine`).
    pub fn combine_trace(&mut self, id: ItemId) -> bool {
        let mut something_changed = false;
        loop {
            if !self.items.get(id).map(|i| i.on_the_board).unwrap_or(false) {
                break;
            }
            if self.combine_at_start(id) || self.combine_at_end(id) {
                something_changed = true;
                self.additional_update_after_change(id);
            } else {
                break;
            }
        }
        something_changed
    }

    fn combine_candidate(&self, id: ItemId, corner: &Point, at_start: bool) -> Option<(ItemId, bool)> {
        let mut contacts = self.trace_contacts_at(id, corner, false);
        contacts.retain(|c| !self.item(*c).is_conduction_area());
        if contacts.len() != 1 {
            return None;
        }
        let this = self.item(id);
        let t = this.as_trace().unwrap();
        for c in contacts {
            let other = self.item(c);
            let Some(ot) = other.as_trace() else { continue };
            if ot.layer == t.layer
                && other.nets_equal_item(this)
                && ot.half_width == t.half_width
                && other.fixed_state == this.fixed_state
                && !self.is_deletion_forbidden(other)
                && !self.is_deletion_forbidden(this)
            {
                if at_start {
                    if corner == ot.last_corner_ref() {
                        return Some((c, false));
                    } else if corner == ot.first_corner_ref() {
                        return Some((c, true));
                    }
                } else if corner == ot.first_corner_ref() {
                    return Some((c, false));
                } else if corner == ot.last_corner_ref() {
                    return Some((c, true));
                }
            }
        }
        None
    }

    fn has_default_entries(&self, a: ItemId, b: ItemId) -> bool {
        let t = self.search_trees.default_tree();
        t.contains_object(&TreeObject::Item(a)) && t.contains_object(&TreeObject::Item(b))
    }

    fn combine_at_start(&mut self, id: ItemId) -> bool {
        let start_corner = self.trace(id).first_corner();
        let Some((other_id, reverse)) = self.combine_candidate(id, &start_corner, true) else {
            return false;
        };
        let this_lines: Vec<Line> = self.trace(id).polyline.lines().to_vec();
        let other_lines: Vec<Line> = {
            let ol = self.trace(other_id).polyline.lines();
            if reverse {
                ol.iter().rev().map(|l| l.opposite()).collect()
            } else {
                ol.to_vec()
            }
        };
        let skip_line = other_lines[other_lines.len() - 2].is_equal_or_opposite(&this_lines[1]);
        let mut new_line_count = this_lines.len() + other_lines.len() - 2;
        if skip_line {
            new_line_count -= 1;
        }
        let mut new_lines: Vec<Line> = Vec::with_capacity(new_line_count);
        new_lines.extend_from_slice(&other_lines[..other_lines.len() - 1]);
        if skip_line {
            new_lines.pop();
        }
        new_lines.extend_from_slice(&this_lines[1..]);
        let joined = Polyline::from_lines(&new_lines);
        let has_entries = self.has_default_entries(id, other_id);
        if joined.line_count() != new_line_count || !has_entries {
            self.replace_trace_geometry(id, joined);
        } else {
            let mut to_no = other_lines.len();
            if skip_line {
                to_no -= 1;
            }
            let from_no = other_lines.len() - 3;
            // change order: the other trace is reversed in the joined polyline
            let change_order = self.trace(other_id).first_corner() == self.trace(id).first_corner();
            self.merge_entries_in_front(other_id, id, &joined, from_no as i32, to_no as i32, change_order);
            self.item_mut(other_id).on_the_board = false;
            self.item_mut(id).as_trace_mut().unwrap().polyline = joined;
        }
        if self.trace(id).polyline.line_count() < 3 {
            self.remove_item(id);
        }
        self.remove_item(other_id);
        let layer = self.item(id).first_layer();
        self.join_changed_area(&start_corner.to_float(), layer);
        true
    }

    fn combine_at_end(&mut self, id: ItemId) -> bool {
        let end_corner = self.trace(id).last_corner();
        let Some((other_id, reverse)) = self.combine_candidate(id, &end_corner, false) else {
            return false;
        };
        let this_lines: Vec<Line> = self.trace(id).polyline.lines().to_vec();
        let other_lines: Vec<Line> = {
            let ol = self.trace(other_id).polyline.lines();
            if reverse {
                ol.iter().rev().map(|l| l.opposite()).collect()
            } else {
                ol.to_vec()
            }
        };
        let skip_line = this_lines[this_lines.len() - 2].is_equal_or_opposite(&other_lines[1]);
        let mut new_line_count = this_lines.len() + other_lines.len() - 2;
        if skip_line {
            new_line_count -= 1;
        }
        let mut new_lines: Vec<Line> = Vec::with_capacity(new_line_count);
        new_lines.extend_from_slice(&this_lines[..this_lines.len() - 1]);
        if skip_line {
            new_lines.pop();
        }
        new_lines.extend_from_slice(&other_lines[1..]);
        let joined = Polyline::from_lines(&new_lines);
        let has_entries = self.has_default_entries(id, other_id);
        if joined.line_count() != new_line_count || !has_entries {
            self.replace_trace_geometry(id, joined);
        } else {
            let mut to_no = this_lines.len();
            if skip_line {
                to_no -= 1;
            }
            let from_no = this_lines.len() - 3;
            let change_order = self.trace(other_id).last_corner() == self.trace(id).last_corner();
            self.merge_entries_at_end(other_id, id, &joined, from_no as i32, to_no as i32, change_order);
            self.item_mut(other_id).on_the_board = false;
            self.item_mut(id).as_trace_mut().unwrap().polyline = joined;
        }
        if self.trace(id).polyline.line_count() < 3 {
            self.remove_item(id);
        }
        self.remove_item(other_id);
        let layer = self.item(id).first_layer();
        self.join_changed_area(&end_corner.to_float(), layer);
        true
    }

    fn link_shapes(&self, tree_index: usize, to: ItemId, joined: &Polyline, from_no: i32, to_no: i32) -> Vec<TileShape> {
        let tree = &self.search_trees.trees()[tree_index];
        let item = self.item(to);
        let t = item.as_trace().unwrap();
        let hw = t.half_width + tree.clearance_compensation_value(&self.rules, item.clearance_class, t.layer);
        tree.offset_shapes(joined, hw, from_no, to_no)
    }

    fn merge_entries_in_front(&mut self, from: ItemId, to: ItemId, joined: &Polyline, from_no: i32, to_no: i32, change_order: bool) {
        let layer = self.trace(to).layer;
        for ti in 0..self.search_trees.trees().len() {
            let links = self.link_shapes(ti, to, joined, from_no, to_no);
            self.search_trees.trees_mut()[ti].merge_entries_in_front(
                TreeObject::Item(from),
                TreeObject::Item(to),
                layer,
                change_order,
                links,
            );
        }
    }

    fn merge_entries_at_end(&mut self, from: ItemId, to: ItemId, joined: &Polyline, from_no: i32, to_no: i32, change_order: bool) {
        let layer = self.trace(to).layer;
        for ti in 0..self.search_trees.trees().len() {
            let links = self.link_shapes(ti, to, joined, from_no, to_no);
            self.search_trees.trees_mut()[ti].merge_entries_at_end(
                TreeObject::Item(from),
                TreeObject::Item(to),
                layer,
                change_order,
                links,
            );
        }
    }

    pub fn join_changed_area(&mut self, p: &crate::geometry::FloatPoint, layer: i32) {
        if let Some(c) = &mut self.changed_area {
            c.join(p, layer);
        }
    }

    /// Splits the trace at intersections with other traces of its net and at drill items
    /// (`PolylineTrace.split(clipShape)`). Returns the resulting pieces.
    pub fn split_trace(&mut self, id: ItemId, clip: Option<&IntOctagon>) -> Vec<ItemId> {
        let mut result: Vec<ItemId> = Vec::new();
        // A piece may already be removed by the split of its sibling; the original then
        // returns at the first overlapping tree entry with an empty result.
        if !self.items.get(id).map(|i| i.on_the_board).unwrap_or(false) {
            return result;
        }
        if !self.item(id).nets_normal() {
            result.push(id);
            return result;
        }
        let mut own_trace_split = false;
        let default_tree = self.search_trees.default_tree_id();
        let line_count = self.trace(id).polyline.line_count();
        let mut i = 0;
        while i + 2 < line_count {
            if let Some(c) = clip {
                let seg = LineSegment::from_polyline(&self.trace(id).polyline, i + 1);
                if !c.intersects(&seg.bounding_octagon_as_box()) {
                    i += 1;
                    continue;
                }
            }
            let Some(current_shape) = self.tree_shape(default_tree, id, i) else {
                i += 1;
                continue;
            };
            let layer = self.trace(id).layer;
            let current_seg = LineSegment::from_polyline(&self.trace(id).polyline, i + 1);
            // The original keeps the found objects, so entries of items removed in the
            // meantime are evaluated with their last geometry.
            let mut entries: Vec<(TreeEntry, Option<Item>)> = Vec::new();
            self.append_overlapping_with_items(&current_shape, layer, &mut entries);
            let mut k = 0;
            while k < entries.len() {
                if !self.items.get(id).map(|i| i.on_the_board).unwrap_or(false) {
                    return result;
                }
                let (entry, snapshot) = entries[k].clone();
                k += 1;
                let Some(found_id) = entry.object.item() else { continue };
                let found_owned;
                let found: &Item = match self.items.get(found_id) {
                    Some(f) => f,
                    None => match snapshot {
                        Some(sn) => {
                            found_owned = sn;
                            &found_owned
                        }
                        None => continue,
                    },
                };
                if found_id == id {
                    let si = entry.shape_index as i64;
                    if si >= i as i64 - 1 && si <= i as i64 + 1 {
                        continue;
                    }
                    let pl = &self.trace(id).polyline;
                    if (i as i64) < si {
                        if pl.corner_ref(i + 1) == pl.corner_ref(entry.shape_index) {
                            continue;
                        }
                    } else if pl.corner_ref(entry.shape_index + 1) == pl.corner_ref(i) {
                        continue;
                    }
                }
                if !found.shares_net(self.item(id)) {
                    continue;
                }
                match &found.kind {
                    ItemKind::Trace(ft) => {
                        let found_seg = LineSegment::from_polyline(&ft.polyline, entry.shape_index + 1);
                        let intersecting = found_seg.intersection(&current_seg);
                        let mut split_pieces: Vec<ItemId> = Vec::new();
                        let mut found_trace_split = false;
                        if found_id != id {
                            for l in &intersecting {
                                let line_index = entry.shape_index + 1;
                                if let Some((a, b)) = self.split_trace_at_line(found_id, line_index, l) {
                                    for p in [a, b].into_iter().flatten() {
                                        found_trace_split = true;
                                        split_pieces.push(p);
                                    }
                                    if found_trace_split {
                                        self.append_overlapping_with_items(&current_shape, layer, &mut entries);
                                        k = 0;
                                        break;
                                    }
                                }
                            }
                            if !found_trace_split {
                                split_pieces.push(found_id);
                            }
                        }
                        let intersecting2 = current_seg.intersection(&found_seg);
                        for l in &intersecting2 {
                            if let Some((a, b)) = self.split_trace_at_line(id, i + 1, l) {
                                own_trace_split = true;
                                if let Some(a) = a {
                                    let r = self.split_trace(a, clip);
                                    result.extend(r);
                                }
                                if let Some(b) = b {
                                    let r = self.split_trace(b, clip);
                                    result.extend(r);
                                }
                                break;
                            }
                        }
                        if found_trace_split || own_trace_split {
                            for p in split_pieces.iter().chain(result.clone().iter()) {
                                self.remove_if_cycle(*p);
                            }
                        }
                        if own_trace_split {
                            break;
                        }
                    }
                    ItemKind::Pin(_) | ItemKind::Via(_) => {
                        let split_point = found.as_drill().unwrap().center.clone();
                        if current_seg.contains(&split_point) {
                            let dir = current_seg.get_line().direction().turn_45_degree(2);
                            let split_line = Line::from_point_direction(&split_point, &Direction::Int(dir));
                            self.split_trace_at_line(id, i + 1, &split_line);
                        }
                    }
                    ItemKind::Obstacle(_) if found.is_conduction_area() => {
                        let this = self.item(id);
                        if !this.is_user_fixed() {
                            let ignore_areas = this
                                .net_numbers
                                .first()
                                .and_then(|n| self.rules.net_class_of(*n))
                                .map(|c| c.get_ignore_cycles_with_areas())
                                .unwrap_or(false);
                            if !ignore_areas
                                && self.trace_start_contacts(id).contains(&found_id)
                                && self.trace_end_contacts(id).contains(&found_id)
                            {
                                self.remove_item(id);
                                return result;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if own_trace_split {
                break;
            }
            i += 1;
        }
        if !own_trace_split {
            result.push(id);
        }
        if result.len() > 1 {
            for r in result.clone() {
                if self.items.contains(r) {
                    self.additional_update_after_change(r);
                }
            }
        }
        result
    }

    fn append_overlapping_with_items(&self, shape: &TileShape, layer: i32, out: &mut Vec<(TreeEntry, Option<Item>)>) {
        let mut v = Vec::new();
        self.search_trees
            .default_tree()
            .overlapping_tree_entries(shape, layer, &[], &self.lookup(), &mut v);
        for e in v {
            let it = e.object.item().and_then(|i| self.items.get(i)).cloned();
            out.push((e, it));
        }
    }

    /// Splits a trace at a point, returns the two pieces if the point is inside a segment.
    pub fn split_trace_at_point(&mut self, id: ItemId, point: &Point) -> Option<(Option<ItemId>, Option<ItemId>)> {
        let n = self.trace(id).polyline.line_count();
        for i in 0..n.saturating_sub(2) {
            let seg = LineSegment::from_polyline(&self.trace(id).polyline, i + 1);
            if seg.contains(point) {
                let dir = seg.get_line().direction().turn_45_degree(2);
                let split_line = Line::from_point_direction(point, &Direction::Int(dir));
                if let Some(r) = self.split_trace_at_line(id, i + 1, &split_line) {
                    return Some(r);
                }
            }
        }
        None
    }

    /// `PolylineTrace.split(lineIndex, newEndLine)`.
    fn split_trace_at_line(&mut self, id: ItemId, line_index: usize, end_line: &Line) -> Option<(Option<ItemId>, Option<ItemId>)> {
        let item = self.items.get(id)?;
        if !item.on_the_board || self.is_deletion_forbidden(item) {
            return None;
        }
        let (p0, p1) = self.trace(id).polyline.split(line_index, end_line)?;
        if self.split_inside_drill_pad_prohibited(id, line_index, end_line) {
            return None;
        }
        let item = self.item(id).clone();
        let t = item.as_trace().unwrap().clone();
        self.remove_item(id);
        let a = self.insert_trace_without_cleaning(p0, t.layer, t.half_width, &item.net_numbers, item.clearance_class, item.fixed_state);
        let b = self.insert_trace_without_cleaning(p1, t.layer, t.half_width, &item.net_numbers, item.clearance_class, item.fixed_state);
        Some((a, b))
    }

    fn split_inside_drill_pad_prohibited(&self, id: ItemId, line_index: usize, line: &Line) -> bool {
        let this = self.item(id);
        let t = this.as_trace().unwrap();
        let intersection = t.polyline.line(line_index).intersection(line);
        let mut pad_found = false;
        for oid in self.pick_items(&intersection, t.layer) {
            let o = self.item(oid);
            if !o.shares_net(this) {
                continue;
            }
            match &o.kind {
                ItemKind::Pin(p) => {
                    if p.drill.center == intersection {
                        return false;
                    }
                    pad_found = true;
                }
                ItemKind::Trace(ot) => {
                    if (oid != id && *ot.first_corner_ref() == intersection) || *ot.last_corner_ref() == intersection {
                        return false;
                    }
                }
                _ => {}
            }
        }
        pad_found
    }

    /// Splits and combines the trace (`PolylineTrace.normalize`).
    pub fn normalize_trace(&mut self, id: ItemId, clip: Option<&IntOctagon>) -> bool {
        self.normalize_trace_depth(id, clip, 0)
    }

    fn normalize_trace_depth(&mut self, id: ItemId, clip: Option<&IntOctagon>, depth: i32) -> bool {
        if depth > MAX_NORMALIZATION_DEPTH {
            return false;
        }
        if !self.items.contains(id) {
            return false;
        }
        let pieces = self.split_trace(id, clip);
        let mut result = pieces.len() != 1;
        for p in pieces {
            if !self.items.get(p).map(|i| i.on_the_board).unwrap_or(false) {
                continue;
            }
            let combined = self.combine_trace(p);
            if !self.items.get(p).map(|i| i.on_the_board).unwrap_or(false) {
                if combined {
                    result = true;
                }
                continue;
            }
            let t = self.trace(p);
            if t.corner_count() == 2 && t.first_corner() == t.last_corner() {
                if !self.is_deletion_forbidden(self.item(p)) {
                    self.remove_item(p);
                    result = true;
                }
            } else if combined {
                self.normalize_trace_depth(p, clip, depth + 1);
                result = true;
            }
        }
        result
    }

    /// Changes the polyline of a trace, reusing unchanged search tree entries
    /// (`PolylineTrace.change`), then normalizes the trace.
    pub fn change_trace(&mut self, id: ItemId, new_polyline: Polyline) {
        if !self.item(id).on_the_board {
            self.item_mut(id).as_trace_mut().unwrap().polyline = new_polyline;
            return;
        }
        self.additional_update_after_change(id);
        let old_lines: Vec<Line> = self.trace(id).polyline.lines().to_vec();
        let new_lines = new_polyline.lines();
        let last_index = new_lines.len().min(old_lines.len());
        let mut first_diff = last_index;
        for i in 0..last_index {
            if !lines_identical(&new_lines[i], &old_lines[i]) {
                first_diff = i;
                break;
            }
        }
        if first_diff == last_index {
            return;
        }
        let mut last_diff: i64 = -1;
        for i in 1..=last_index {
            if !lines_identical(&new_lines[new_lines.len() - i], &old_lines[old_lines.len() - i]) {
                last_diff = (new_lines.len() - i) as i64;
                break;
            }
        }
        if last_diff < 0 {
            return;
        }
        let keep_at_start = (first_diff as i64 - 2).max(0) as usize;
        let keep_at_end = (new_lines.len() as i64 - last_diff - 3).max(0) as usize;
        let item = self.item(id);
        let (layer, hw, cl) = (self.trace(id).layer, self.trace(id).half_width, item.clearance_class);
        for ti in 0..self.search_trees.trees().len() {
            let tree = &self.search_trees.trees()[ti];
            let comp_hw = hw + tree.clearance_compensation_value(&self.rules, cl, layer);
            let shapes = tree.offset_shapes(
                &new_polyline,
                comp_hw,
                keep_at_start as i32,
                new_polyline.line_count() as i32 - 1 - keep_at_end as i32,
            );
            self.search_trees.trees_mut()[ti].change_entries(TreeObject::Item(id), layer, shapes, keep_at_start, keep_at_end);
        }
        self.item_mut(id).as_trace_mut().unwrap().polyline = new_polyline;
        let clip = self.changed_area.as_ref().map(|c| c.get_area(layer));
        self.normalize_trace(id, clip.as_ref());
    }

    /// Splits the traces of a net at `location` (`splitTraces`).
    pub fn split_traces(&mut self, location: &Point, layer: i32, net_no: i32) -> bool {
        let picked = self.pick_traces(location, layer);
        let loc_shape = TileShape::from_point(location).bounding_octagon();
        let mut split = false;
        for id in picked {
            if !self.items.contains(id) {
                continue;
            }
            if self.item(id).contains_net(net_no) {
                let pieces = self.split_trace(id, Some(&loc_shape));
                if pieces.len() != 1 {
                    split = true;
                }
            }
        }
        split
    }

    /// Combines the traces of a net (all nets if `net_no < 0`).
    pub fn combine_traces(&mut self, net_no: i32) -> bool {
        let mut result = false;
        loop {
            let mut changed = false;
            for id in self.items.ids() {
                let Some(item) = self.items.get(id) else { continue };
                if (net_no < 0 || item.contains_net(net_no)) && item.is_trace() && item.on_the_board && self.combine_trace(id) {
                    changed = true;
                    result = true;
                    break;
                }
            }
            if !changed {
                break;
            }
        }
        result
    }

    /// Normalizes all traces of a net (`normalizeTraces`).
    pub fn normalize_traces(&mut self, net_no: i32) -> bool {
        if self.normalize_suppressed_nets.contains(&net_no) {
            return false;
        }
        let mut result = false;
        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > MAX_NORMALIZE_ITERATIONS {
                log::warn!(
                    "BasicBoard.normalizeTraces: reached {MAX_NORMALIZE_ITERATIONS} iterations for net {net_no} — stopping to prevent hang."
                );
                self.normalize_suppressed_nets.insert(net_no);
                break;
            }
            let mut changed = false;
            let net_traces: Vec<ItemId> = self
                .items
                .iter()
                .filter(|i| i.contains_net(net_no) && i.is_trace() && i.on_the_board)
                .map(|i| i.id)
                .collect();
            for id in net_traces {
                if !self.items.get(id).map(|i| i.on_the_board).unwrap_or(false) {
                    continue;
                }
                if self.normalize_trace(id, None) {
                    changed = true;
                    result = true;
                } else if self.items.contains(id) && !self.item(id).is_user_fixed() && self.remove_if_cycle(id) {
                    changed = true;
                    result = true;
                }
            }
            if !changed {
                break;
            }
        }
        result
    }

    /// Normalizes the traces of all nets (`normalizeAllTraces`).
    pub fn normalize_all_traces(&mut self) -> bool {
        let mut nets: BTreeSet<i32> = BTreeSet::new();
        let mut nets_order: Vec<i32> = Vec::new();
        for item in self.items.iter() {
            if item.is_trace() && item.on_the_board {
                for n in &item.net_numbers {
                    if nets.insert(*n) {
                        nets_order.push(*n);
                    }
                }
            }
        }
        let mut result = false;
        for net_no in nets_order {
            let mut iterations = 0;
            loop {
                iterations += 1;
                if iterations > MAX_NORMALIZE_ITERATIONS {
                    log::warn!("BasicBoard.normalize_all_traces: reached {MAX_NORMALIZE_ITERATIONS} iterations for net {net_no}");
                    break;
                }
                let mut changed = false;
                let net_traces: Vec<ItemId> = self
                    .items
                    .iter()
                    .filter(|i| i.contains_net(net_no) && i.is_trace() && i.on_the_board)
                    .map(|i| i.id)
                    .collect();
                for id in net_traces {
                    if !self.items.get(id).map(|i| i.on_the_board).unwrap_or(false) {
                        continue;
                    }
                    if self.normalize_trace(id, None) {
                        changed = true;
                        result = true;
                    } else if self.items.contains(id) && !self.item(id).is_user_fixed() && self.remove_if_cycle(id) {
                        changed = true;
                        result = true;
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        result
    }

    /// Removes all traces and vias.
    pub fn delete_all_tracks_and_vias(&mut self) {
        for id in self.items.ids() {
            let item = self.item(id);
            if item.is_trace() || item.is_via() {
                self.remove_from_trees(id);
                self.items.remove(id);
            }
        }
    }

    pub fn set_trace_fixed(&mut self, id: ItemId, fixed: FixedState) {
        self.item_mut(id).fixed_state = fixed;
    }
}

/// Identity of lines in the original is reference identity (`Line::same_object`).
#[inline]
fn lines_identical(a: &Line, b: &Line) -> bool {
    a.same_object(b)
}

trait OctagonBox {
    fn bounding_octagon_as_box(&self) -> IntOctagon;
}

impl OctagonBox for LineSegment {
    /// The original intersects the clip octagon with the bounding box of the segment.
    fn bounding_octagon_as_box(&self) -> IntOctagon {
        self.bounding_box().to_int_octagon()
    }
}
