//! Contacts and connectivity of items (the contact methods of `Item`, `Trace`,
//! `DrillItem`, `Pin` and `ConductionArea`).

use super::item::{Item, ItemId, ItemKind, ObstacleKind};
use super::Board;
use crate::geometry::{IntBox, Point, TileShape};
use std::collections::BTreeSet;

/// Where to stop when collecting the connection items of a trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopConnectionOption {
    None,
    FanoutVia,
    Via,
}

const PROTECT_FANOUT_LENGTH: f64 = 400.0;

impl Board {
    /// Contacts of a trace at one of its end points (`Trace.getNormalContacts(point, ignoreNet)`).
    pub fn trace_contacts_at(&self, id: ItemId, point: &Point, ignore_net: bool) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let item = self.item(id);
        let Some(t) = item.as_trace() else { return result };
        if point != t.first_corner_ref() && point != t.last_corner_ref() {
            return result;
        }
        let shape = TileShape::from_point(point);
        let lc = self.layer_count();
        for oid in self.overlapping_items(&shape, t.layer) {
            if oid == id {
                continue;
            }
            let other = self.item(oid);
            if !other.shares_layer(item, lc) || !(ignore_net || other.shares_net(item)) {
                continue;
            }
            match &other.kind {
                ItemKind::Trace(ot) => {
                    if point == ot.first_corner_ref() || point == ot.last_corner_ref() || ot.contains(point) {
                        result.insert(oid);
                    }
                }
                ItemKind::Pin(p) => {
                    if *point == p.drill.center {
                        result.insert(oid);
                    }
                }
                ItemKind::Via(v) => {
                    if *point == v.drill.center {
                        result.insert(oid);
                    }
                }
                ItemKind::Obstacle(o) if matches!(o.kind, ObstacleKind::Conduction { .. }) => {
                    if o.area.contains(point) {
                        result.insert(oid);
                    }
                }
                _ => {}
            }
        }
        result
    }

    pub fn trace_start_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        let p = self.item(id).as_trace().unwrap().first_corner();
        self.trace_contacts_at(id, &p, false)
    }

    pub fn trace_end_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        let p = self.item(id).as_trace().unwrap().last_corner();
        self.trace_contacts_at(id, &p, false)
    }

    /// The normal contacts of an item (`getNormalContacts()`).
    pub fn normal_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        let item = self.any_item(id).expect("item exists");
        match &item.kind {
            ItemKind::Trace(_) => self.trace_normal_contacts(id),
            ItemKind::Via(_) => self.drill_normal_contacts(id),
            ItemKind::Pin(_) => {
                let mut r = self.drill_normal_contacts(id);
                if item.component_id != 0 {
                    for o in self.copper_overlapping_pins(id, |o| o.shares_net(item)) {
                        r.insert(o);
                    }
                }
                r
            }
            ItemKind::Obstacle(o) if matches!(o.kind, ObstacleKind::Conduction { .. }) => {
                self.conduction_area_normal_contacts(id)
            }
            _ => BTreeSet::new(),
        }
    }

    fn trace_normal_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        let item = self.any_item(id).expect("item exists");
        let t = item.as_trace().unwrap();
        let mut result = BTreeSet::new();
        result.extend(self.trace_contacts_at(id, &t.first_corner(), false));
        result.extend(self.trace_contacts_at(id, &t.last_corner(), false));
        let bounds = TileShape::Octagon(t.contact_bounds());
        let lc = self.layer_count();
        for oid in self.overlapping_items(&bounds, t.layer) {
            if oid == id {
                continue;
            }
            let other = self.item(oid);
            if !other.shares_net(item) || !other.shares_layer(item, lc) {
                continue;
            }
            match &other.kind {
                ItemKind::Trace(ot) => {
                    if t.contains(ot.first_corner_ref()) || t.contains(ot.last_corner_ref()) {
                        result.insert(oid);
                    }
                }
                ItemKind::Pin(_) | ItemKind::Via(_) => {
                    let c = &other.as_drill().unwrap().center;
                    if t.contains(c) {
                        result.insert(oid);
                    }
                }
                _ => {}
            }
        }
        result
    }

    fn drill_normal_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        let item = self.any_item(id).expect("item exists");
        let center = &item.as_drill().unwrap().center;
        let shape = TileShape::from_point(center);
        let lc = self.layer_count();
        let mut result = BTreeSet::new();
        for oid in self.overlapping_items(&shape, -1) {
            if oid == id {
                continue;
            }
            let other = self.item(oid);
            if !other.shares_net(item) || !other.shares_layer(item, lc) {
                continue;
            }
            match &other.kind {
                ItemKind::Trace(t) => {
                    if center == t.first_corner_ref() || center == t.last_corner_ref() || t.contains(center) {
                        result.insert(oid);
                    }
                }
                ItemKind::Pin(_) | ItemKind::Via(_) => {
                    if *center == other.as_drill().unwrap().center {
                        result.insert(oid);
                    }
                }
                ItemKind::Obstacle(o) if matches!(o.kind, ObstacleKind::Conduction { .. }) => {
                    if o.area.contains(center) {
                        result.insert(oid);
                    }
                }
                _ => {}
            }
        }
        result
    }

    fn conduction_area_normal_contacts(&self, id: ItemId) -> BTreeSet<ItemId> {
        let item = self.item(id);
        let layer = item.first_layer();
        let lc = self.layer_count();
        let mut result = BTreeSet::new();
        for i in 0..self.tile_shape_count(id) {
            let Some(shape) = self.tile_shape(id, i) else { continue };
            for oid in self.overlapping_items(&shape, layer) {
                if oid == id {
                    continue;
                }
                let other = self.item(oid);
                if !other.shares_net(item) || !other.shares_layer(item, lc) {
                    continue;
                }
                match &other.kind {
                    ItemKind::Trace(t) => {
                        if shape.contains(t.first_corner_ref()) || shape.contains(t.last_corner_ref()) {
                            result.insert(oid);
                        }
                    }
                    ItemKind::Pin(_) | ItemKind::Via(_) => {
                        if shape.contains(&other.as_drill().unwrap().center) {
                            result.insert(oid);
                        }
                    }
                    _ => {}
                }
            }
        }
        result
    }

    /// Pins of other components or the same component whose copper overlaps this pin.
    pub fn copper_overlapping_pins(&self, id: ItemId, accept: impl Fn(&Item) -> bool) -> Vec<ItemId> {
        let item = self.item(id);
        let mut result = Vec::new();
        let Some(drill) = item.as_drill() else { return result };
        let b: IntBox = item.bounding_box();
        let lc = self.layer_count();
        for oid in self.overlapping_items(&TileShape::Box(b), -1) {
            let other = self.item(oid);
            let Some(od) = other.as_pin().map(|p| &p.drill) else { continue };
            if oid == id || !accept(other) || !other.shares_layer(item, lc) || !b.intersects(&other.bounding_box()) {
                continue;
            }
            let mut overlaps = false;
            'outer: for i in 0..drill.tile_shape_count {
                for j in 0..od.tile_shape_count {
                    if let (Some(m), Some(t)) = (drill.get_shape(i), od.get_shape(j)) {
                        if drill.shape_layer(i) == od.shape_layer(j) && m.intersects(t) {
                            overlaps = true;
                            break 'outer;
                        }
                    }
                }
            }
            if overlaps {
                result.push(oid);
            }
        }
        result
    }

    pub fn overlapping_netted_pins(&self, id: ItemId) -> Vec<ItemId> {
        if self.item(id).net_count() != 0 {
            return Vec::new();
        }
        self.copper_overlapping_pins(id, |o| o.net_count() != 0)
    }

    /// All contacts of a connectable item on any layer (`getAllContacts`).
    pub fn all_contacts(&self, id: ItemId, layer: Option<i32>) -> BTreeSet<ItemId> {
        let item = self.item(id);
        let mut result = BTreeSet::new();
        if !item.is_connectable_type() {
            return result;
        }
        for i in 0..self.tile_shape_count(id) {
            let sl = self.shape_layer(id, i);
            if let Some(l) = layer {
                if sl != l {
                    continue;
                }
            }
            let Some(shape) = self.tile_shape(id, i) else { continue };
            for oid in self.overlapping_items(&shape, sl) {
                let other = self.item(oid);
                if oid != id && other.is_connectable_type() && other.shares_net(item) {
                    result.insert(oid);
                }
            }
        }
        result
    }

    /// The contact point of two items in normal contact (`normalContactPoint`).
    pub fn normal_contact_point(&self, a: ItemId, b: ItemId) -> Option<Point> {
        let ia = self.item(a);
        let ib = self.item(b);
        let lc = self.layer_count();
        match (&ia.kind, &ib.kind) {
            (ItemKind::Trace(ta), ItemKind::Trace(tb)) => {
                if ta.layer != tb.layer {
                    return None;
                }
                let p1 = ta.first_corner();
                let p2 = ta.last_corner();
                let op1 = tb.first_corner();
                let op2 = tb.last_corner();
                let mut unique: Option<Point> = None;
                let mut count = 0;
                if tb.contains(&p1) {
                    unique = Some(p1.clone());
                    count += 1;
                }
                if tb.contains(&p2) && unique.as_ref() != Some(&p2) {
                    unique = Some(p2.clone());
                    count += 1;
                }
                if ta.contains(&op1) && unique.as_ref() != Some(&op1) {
                    unique = Some(op1.clone());
                    count += 1;
                }
                if ta.contains(&op2) && unique.as_ref() != Some(&op2) {
                    unique = Some(op2.clone());
                    count += 1;
                }
                if count == 1 {
                    unique
                } else {
                    None
                }
            }
            (ItemKind::Trace(t), ItemKind::Pin(_) | ItemKind::Via(_))
            | (ItemKind::Pin(_) | ItemKind::Via(_), ItemKind::Trace(t)) => {
                let (drill_item, trace_item) = if ia.is_trace() { (ib, ia) } else { (ia, ib) };
                if !drill_item.shares_layer(trace_item, lc) {
                    return None;
                }
                let c = &drill_item.as_drill().unwrap().center;
                if t.contains(c) {
                    Some(c.clone())
                } else {
                    None
                }
            }
            (ItemKind::Pin(_) | ItemKind::Via(_), ItemKind::Pin(_) | ItemKind::Via(_)) => {
                let ca = &ia.as_drill().unwrap().center;
                let cb = &ib.as_drill().unwrap().center;
                if ia.shares_layer(ib, lc) && ca == cb {
                    Some(ca.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// The set of items connected to `id` by normal contacts (`getConnectedSet`).
    pub fn connected_set(&self, id: ItemId, net_no: i32, stop_at_plane: bool) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        if net_no > 0 && !self.item(id).contains_net(net_no) {
            return result;
        }
        result.insert(id);
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            // The original recursion visits contacts in sorted order; the resulting set is
            // the same for an iterative traversal.
            for c in self.normal_contacts(cur) {
                let ci = self.item(c);
                if stop_at_plane && ci.is_conduction_area() && ci.component_id <= 0 {
                    continue;
                }
                if net_no > 0 && !ci.contains_net(net_no) {
                    continue;
                }
                if result.insert(c) {
                    stack.push(c);
                }
            }
        }
        result
    }

    /// Connectable items of the net(s) of `id`, which are not connected to `id`.
    pub fn unconnected_set(&self, id: ItemId, net_no: i32) -> BTreeSet<ItemId> {
        let item = self.item(id);
        let mut result = BTreeSet::new();
        if net_no > 0 && !item.contains_net(net_no) {
            return result;
        }
        if net_no > 0 {
            result.extend(self.get_connectable_items(net_no));
        } else {
            for n in &item.net_numbers {
                result.extend(self.get_connectable_items(*n));
            }
        }
        for c in self.connected_set(id, net_no, false) {
            result.remove(&c);
        }
        result
    }

    /// The connected sets of the items of a net.
    pub fn connected_sets(&self, net_no: i32) -> Vec<BTreeSet<ItemId>> {
        let mut result = Vec::new();
        if net_no <= 0 {
            return result;
        }
        let mut to_handle: BTreeSet<ItemId> = self.get_connectable_items(net_no).into_iter().collect();
        while let Some(first) = to_handle.iter().next().copied() {
            let set = self.connected_set(first, net_no, false);
            for s in &set {
                to_handle.remove(s);
            }
            to_handle.remove(&first);
            result.push(set);
        }
        result
    }

    fn is_cycle_recu(
        &self,
        cur: ItemId,
        visited: &mut BTreeSet<ItemId>,
        search_item: ItemId,
        come_from: ItemId,
        ignore_areas: bool,
    ) -> bool {
        if ignore_areas && self.item(cur).is_conduction_area() {
            return false;
        }
        for c in self.normal_contacts(cur) {
            if c == come_from {
                continue;
            }
            if c == search_item {
                return true;
            }
            if visited.insert(c) && self.is_cycle_recu(c, visited, search_item, cur, ignore_areas) {
                return true;
            }
        }
        false
    }

    pub fn trace_is_overlap(&self, id: ItemId) -> bool {
        let s = self.trace_start_contacts(id);
        let e = self.trace_end_contacts(id);
        !s.is_disjoint(&e)
    }

    pub fn trace_is_cycle(&self, id: ItemId) -> bool {
        if self.trace_is_overlap(id) {
            return true;
        }
        let start = self.trace_start_contacts(id);
        let mut visited = start.clone();
        let item = self.item(id);
        let ignore_areas = item
            .net_numbers
            .first()
            .and_then(|n| self.rules.net_class_of(*n))
            .map(|c| c.get_ignore_cycles_with_areas())
            .unwrap_or(false);
        for c in start {
            if self.is_cycle_recu(c, &mut visited, id, id, ignore_areas) {
                return true;
            }
        }
        false
    }

    pub fn is_tail(&self, id: ItemId) -> bool {
        let item = self.item(id);
        match &item.kind {
            ItemKind::Trace(_) => {
                self.trace_start_contacts(id).is_empty() || self.trace_end_contacts(id).is_empty()
            }
            ItemKind::Via(_) => {
                let contacts = self.normal_contacts(id);
                if contacts.len() <= 1 {
                    return true;
                }
                let mut it = contacts.iter();
                let first = self.item(*it.next().unwrap());
                let lc = self.layer_count();
                let (fl, ll) = (first.first_layer_with(lc), first.last_layer_with(lc));
                for c in it {
                    let ci = self.item(*c);
                    if ci.first_layer_with(lc) != fl || ci.last_layer_with(lc) != ll {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    pub fn is_fanout_via(&self, id: ItemId, ignore: Option<&BTreeSet<ItemId>>) -> bool {
        let lc = self.layer_count();
        for c in self.normal_contacts(id) {
            let ci = self.item(c);
            if ci.is_pin() && ci.first_layer_with(lc) == ci.last_layer_with(lc) && self.normal_contacts(c).len() <= 1 {
                return true;
            }
            if let Some(t) = ci.as_trace() {
                if ignore.map(|s| s.contains(&c)).unwrap_or(false) {
                    continue;
                }
                if t.get_length() >= PROTECT_FANOUT_LENGTH * t.half_width as f64 {
                    continue;
                }
                for tc in self.normal_contacts(c) {
                    let tci = self.item(tc);
                    if tci.is_pin()
                        && tci.first_layer_with(lc) == tci.last_layer_with(lc)
                        && self.normal_contacts(tc).len() <= 1
                    {
                        return true;
                    }
                    if let Some(tt) = tci.as_trace() {
                        if tci.fixed_state == super::FixedState::ShoveFixed && tt.corner_count() == 2 {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// The items of the connection containing `id` up to forks or pins (`getConnectionItems`).
    pub fn connection_items(&self, id: ItemId, stop: StopConnectionOption) -> BTreeSet<ItemId> {
        let contacts = self.normal_contacts(id);
        let mut result = BTreeSet::new();
        let item = self.item(id);
        if item.is_routable() {
            result.insert(id);
        }
        let lc = self.layer_count();
        for start_contact in contacts {
            let Some(prev_point0) = self.normal_contact_point(id, start_contact) else {
                continue;
            };
            let mut prev_contact_point = prev_point0;
            let mut prev_contact_layer = item.first_common_layer(self.item(start_contact), lc);
            if item.is_trace() && self.trace_contacts_at(id, &prev_contact_point, false).len() != 1 {
                continue;
            }
            let mut current = start_contact;
            let mut ring_item = current;
            let mut ring_point = prev_contact_point.clone();
            let mut ring_layer = prev_contact_layer;
            let mut ring_steps: u64 = 0;
            let mut ring_mark: u64 = 1;
            loop {
                ring_steps += 1;
                if ring_steps > 1 && current == ring_item && prev_contact_layer == ring_layer && prev_contact_point == ring_point {
                    break;
                }
                if ring_steps == ring_mark {
                    ring_item = current;
                    ring_point = prev_contact_point.clone();
                    ring_layer = prev_contact_layer;
                    ring_mark *= 2;
                }
                let ci = self.item(current);
                if !ci.is_routable() {
                    break;
                }
                if ci.is_via() {
                    if stop == StopConnectionOption::Via {
                        break;
                    }
                    if stop == StopConnectionOption::FanoutVia && self.is_fanout_via(current, Some(&result)) {
                        break;
                    }
                }
                result.insert(current);
                let mut next_contact: Option<ItemId> = None;
                let mut next_point: Option<Point> = None;
                let mut next_layer = -1;
                let mut fork = false;
                for tmp in self.normal_contacts(current) {
                    let tl = ci.first_common_layer(self.item(tmp), lc);
                    if tl >= 0 {
                        let Some(tp) = self.normal_contact_point(current, tmp) else {
                            fork = true;
                            break;
                        };
                        if prev_contact_layer != tl || prev_contact_point != tp {
                            if next_contact.is_some() {
                                fork = true;
                                break;
                            }
                            next_contact = Some(tmp);
                            next_point = Some(tp);
                            next_layer = tl;
                        }
                    }
                }
                if next_contact.is_none() || fork {
                    break;
                }
                current = next_contact.unwrap();
                prev_contact_point = next_point.unwrap();
                prev_contact_layer = next_layer;
            }
        }
        result
    }

    /// The pins touching the end corners of a trace (`touchingPinsAtEndCorners`).
    pub fn touching_pins_at_end_corners(&self, trace: &Item) -> BTreeSet<ItemId> {
        let mut result = BTreeSet::new();
        let t = trace.as_trace().unwrap();
        let tree = self.search_trees.default_tree();
        let comp_hw = t.half_width + tree.clearance_compensation_value(&self.rules, trace.clearance_class, t.layer);
        for p in [t.first_corner(), t.last_corner()] {
            let oct = p.surrounding_octagon().enlarge(comp_hw as f64);
            for oid in self.overlapping_items_with_clearance(&TileShape::Octagon(oct), t.layer, &[], trace.clearance_class) {
                let o = self.item(oid);
                if o.is_pin() && o.shares_net(trace) {
                    result.insert(oid);
                }
            }
        }
        result
    }

    /// A trace ending at `location` without contacts at that end (`getTraceTail`).
    pub fn get_trace_tail(&self, location: &Point, layer: i32, nets: &[i32]) -> Option<ItemId> {
        let shape = TileShape::from_point(location);
        for oid in self.overlapping_items(&shape, layer) {
            let item = self.item(oid);
            let Some(t) = item.as_trace() else { continue };
            if !item.nets_equal(nets) {
                continue;
            }
            if t.first_corner_ref() == location && self.trace_start_contacts(oid).is_empty() {
                return Some(oid);
            }
            if t.last_corner_ref() == location && self.trace_end_contacts(oid).is_empty() {
                return Some(oid);
            }
        }
        None
    }

    /// Removes a trace and its connection, if it forms a cycle (`removeIfCycle`).
    pub fn remove_if_cycle(&mut self, id: ItemId) -> bool {
        let Some(item) = self.try_item(id) else { return false };
        if !item.on_the_board {
            return false;
        }
        if !self.trace_is_cycle(id) {
            return false;
        }
        let t = item.as_trace().unwrap();
        let layer = t.layer;
        let nets = item.net_numbers.to_vec();
        let ends = [t.first_corner(), t.last_corner()];
        let mut tail_before = [false; 2];
        for i in 0..2 {
            tail_before[i] = self.get_trace_tail(&ends[i], layer, &nets).is_some();
        }
        let conn: Vec<ItemId> = self.connection_items(id, StopConnectionOption::None).into_iter().collect();
        self.remove_items(&conn);
        for i in 0..2 {
            if !tail_before[i] {
                if let Some(tail) = self.get_trace_tail(&ends[i], layer, &nets) {
                    let c: Vec<ItemId> = self.connection_items(tail, StopConnectionOption::None).into_iter().collect();
                    self.remove_items(&c);
                }
            }
        }
        true
    }

    /// Corners for the ratsnest of an item.
    pub fn ratsnest_corners(&self, id: ItemId) -> Vec<Point> {
        let item = self.item(id);
        match &item.kind {
            ItemKind::Pin(_) | ItemKind::Via(_) => vec![item.as_drill().unwrap().center.clone()],
            ItemKind::Trace(t) => {
                let mut r = Vec::new();
                if self.trace_start_contacts(id).is_empty() {
                    r.push(t.first_corner());
                }
                if self.trace_end_contacts(id).is_empty() {
                    r.push(t.last_corner());
                }
                r
            }
            ItemKind::Obstacle(o) if matches!(o.kind, ObstacleKind::Conduction { .. }) => o
                .area
                .corner_approx_arr()
                .iter()
                .map(|c| Point::Int(c.round()))
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn is_connected(&self, id: ItemId) -> bool {
        !self.all_contacts(id, None).is_empty()
    }

    /// The shape of a connectable item used by the autorouter as connection target.
    pub fn trace_connection_shape(&self, tree: super::TreeId, id: ItemId, index: usize) -> Option<TileShape> {
        let item = self.any_item(id)?;
        match &item.kind {
            ItemKind::Trace(t) => {
                if index >= t.tile_shape_count() {
                    log::warn!("PolylineTrace.get_trace_connection_shape index out of range");
                    return None;
                }
                Some(t.connection_shape(index))
            }
            ItemKind::Pin(_) | ItemKind::Via(_) => Some(TileShape::from_point(&item.as_drill().unwrap().center)),
            ItemKind::Obstacle(o) if matches!(o.kind, ObstacleKind::Conduction { .. }) => {
                self.tree_shape(tree, id, index)
            }
            _ => None,
        }
    }
}
