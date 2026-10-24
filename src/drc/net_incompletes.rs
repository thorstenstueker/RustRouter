//! Port of `drc.NetIncompletes` and `drc.AirLine`.

use super::delaunay::PlanarDelaunayTriangulation;
use crate::board::{Board, ItemId};
use crate::geometry::FloatPoint;
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// An unrouted connection (rats nest line) between two items of a net.
#[derive(Clone, Debug)]
pub struct AirLine {
    pub net_no: i32,
    pub from_item: ItemId,
    pub from_corner: FloatPoint,
    pub to_item: ItemId,
    pub to_corner: FloatPoint,
}

impl AirLine {
    /// `compareTo`: compares by net name.
    pub fn compare_to(&self, other: &AirLine, board: &Board) -> Ordering {
        let a = board.rules.nets.get(self.net_no).map(|n| n.name.as_str()).unwrap_or("");
        let b = board.rules.nets.get(other.net_no).map(|n| n.name.as_str()).unwrap_or("");
        a.cmp(b)
    }
}

/// The incomplete connections of a net.
#[derive(Clone, Debug)]
pub struct NetIncompletes {
    pub net_no: i32,
    pub incompletes: Vec<AirLine>,
    draw_marker_radius: f64,
    length_violation: f64,
    connected_group_count: usize,
}

struct NetItem {
    item: ItemId,
    /// Index of the connected set (identity of the `connectedSet` collection in the original).
    group: usize,
}

struct Edge {
    from: usize,
    from_corner: FloatPoint,
    to: usize,
    to_corner: FloatPoint,
    length_square: f64,
}

impl Edge {
    fn compare(&self, other: &Edge) -> Ordering {
        let mut result = self.length_square - other.length_square;
        if result == 0.0 {
            result = self.from_corner.x - other.from_corner.x;
            if result == 0.0 {
                result = self.from_corner.y - other.from_corner.y;
            }
            if result == 0.0 {
                result = self.to_corner.x - other.to_corner.x;
            }
            if result == 0.0 {
                result = self.to_corner.y - other.to_corner.y;
            }
        }
        if result > 0.0 {
            Ordering::Greater
        } else if result < 0.0 {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    }
}

impl NetIncompletes {
    /// Calculates the incompletes of the net `net_no` from its connectable items.
    ///
    /// Deviation: the original puts the items into a `HashSet` with identity hash codes,
    /// so its iteration order (and thereby the choice of the airlines, not their number)
    /// is not reproducible. Here the items are processed in `ItemId` order (descending id).
    pub fn new(net_no: i32, net_items: &[ItemId], board: &Board) -> NetIncompletes {
        let mut result = NetIncompletes {
            net_no,
            incompletes: Vec::new(),
            draw_marker_radius: board.rules.get_min_trace_half_width() as f64 * 2.0,
            length_violation: 0.0,
            connected_group_count: 0,
        };
        let mut filtered: Vec<ItemId> = Vec::new();
        for &id in net_items {
            let item = board.item(id);
            if board.is_tail(id) {
                continue;
            }
            if !item.is_conduction_area() && !item.is_drill_item() && board.normal_contacts(id).is_empty() {
                continue;
            }
            filtered.push(id);
        }
        let mut net_items_grouped = Self::calculate_net_items(net_no, &filtered, board);
        let groups: BTreeSet<usize> = net_items_grouped.iter().map(|n| n.group).collect();
        result.connected_group_count = groups.len();
        if result.connected_group_count <= 1 {
            result.calc_length_violation(board);
            return result;
        }
        let objects: Vec<_> = net_items_grouped.iter().map(|n| board.ratsnest_corners(n.item)).collect();
        let triangulation = PlanarDelaunayTriangulation::new(&objects);
        let mut edges: Vec<Edge> = Vec::new();
        for line in triangulation.get_edge_lines() {
            if line.start_object >= net_items_grouped.len() || line.end_object >= net_items_grouped.len() {
                // degenerate edge to a corner of the bounding triangle (a null object in the original)
                continue;
            }
            let from_corner = line.start_point.to_float();
            let to_corner = line.end_point.to_float();
            edges.push(Edge {
                from: line.start_object,
                length_square: to_corner.distance_square(&from_corner),
                from_corner,
                to: line.end_object,
                to_corner,
            });
        }
        // TreeSet<Edge>: sorted, duplicates (compare == 0) dropped keeping the first.
        edges.sort_by(|a, b| a.compare(b));
        edges.dedup_by(|b, a| a.compare(b) == Ordering::Equal);
        for e in &edges {
            let from_group = net_items_grouped[e.from].group;
            let to_group = net_items_grouped[e.to].group;
            if from_group == to_group {
                continue;
            }
            result.incompletes.push(AirLine {
                net_no,
                from_item: net_items_grouped[e.from].item,
                from_corner: e.from_corner,
                to_item: net_items_grouped[e.to].item,
                to_corner: e.to_corner,
            });
            for n in net_items_grouped.iter_mut() {
                if n.group == from_group {
                    n.group = to_group;
                }
            }
        }
        result.calc_length_violation(board);
        result
    }

    fn calculate_net_items(net_no: i32, items: &[ItemId], board: &Board) -> Vec<NetItem> {
        let mut result = Vec::new();
        let mut unique: BTreeSet<ItemId> = items.iter().copied().collect();
        let mut group = 0;
        while let Some(start) = unique.iter().next().copied() {
            let connected_set = board.connected_set(start, net_no, false);
            let in_component: Vec<ItemId> = connected_set.iter().copied().filter(|i| unique.contains(i)).collect();
            for i in &in_component {
                result.push(NetItem { item: *i, group });
            }
            for i in &in_component {
                unique.remove(i);
            }
            // Safety against endless loops (the original would loop as well).
            unique.remove(&start);
            group += 1;
        }
        result
    }

    pub fn get_incompletes(&self) -> &[AirLine] {
        &self.incompletes
    }

    pub fn get_marker_radius(&self) -> f64 {
        self.draw_marker_radius
    }

    pub fn count(&self) -> usize {
        self.incompletes.len()
    }

    pub fn get_connected_group_count(&self) -> usize {
        self.connected_group_count
    }

    pub fn get_length_violation(&self) -> f64 {
        self.length_violation
    }

    /// Recalculates the length violation. Returns true, if it changed noticeably.
    pub fn calc_length_violation(&mut self, board: &Board) -> bool {
        let max_length = net_maximum_trace_length(board, self.net_no);
        let min_length = net_minimum_trace_length(board, self.net_no);
        if max_length <= 0.0 && min_length <= 0.0 {
            self.length_violation = 0.0;
            return false;
        }
        let mut new_violation = 0.0;
        let trace_length = net_trace_length(board, self.net_no);
        if max_length > 0.0 && trace_length > max_length {
            new_violation = trace_length - max_length;
        }
        if min_length > 0.0 && trace_length < min_length && self.incompletes.is_empty() {
            if trace_length <= 0.0 && self.connected_group_count <= 1 && net_terminal_item_count(board, self.net_no) < 2 {
                // nothing to measure
            } else {
                new_violation = trace_length - min_length;
            }
        }
        let old = self.length_violation;
        self.length_violation = new_violation;
        (new_violation - old).abs() > 0.1
    }
}

/// `Net.getMinimumTraceLength()`.
pub fn net_minimum_trace_length(board: &Board, net_no: i32) -> f64 {
    let Some(net) = board.rules.nets.get(net_no) else { return 0.0 };
    if let (Some(min), _) = net.explicit_length_constraint() {
        if min > 0.0 {
            return min;
        }
    }
    board.rules.net_class_of(net_no).map(|c| c.get_minimum_trace_length()).unwrap_or(0.0)
}

/// `Net.getMaximumTraceLength()`.
pub fn net_maximum_trace_length(board: &Board, net_no: i32) -> f64 {
    let Some(net) = board.rules.nets.get(net_no) else { return 0.0 };
    if let (_, Some(max)) = net.explicit_length_constraint() {
        if max > 0.0 {
            return max;
        }
    }
    board.rules.net_class_of(net_no).map(|c| c.get_maximum_trace_length()).unwrap_or(0.0)
}

/// `Net.getTraceLength()`: the cumulative length of the traces of a net.
pub fn net_trace_length(board: &Board, net_no: i32) -> f64 {
    let mut sum = 0.0;
    for id in board.get_connectable_items(net_no) {
        if let Some(t) = board.item(id).as_trace() {
            sum += t.get_length();
        }
    }
    sum
}

/// `Net.getTerminalItems()`: the connectable, not routable items of a net (in item order).
pub fn net_terminal_items(board: &Board, net_no: i32) -> Vec<ItemId> {
    board
        .items
        .iter()
        .filter(|i| i.is_connectable_type() && i.contains_net(net_no) && !i.is_routable())
        .map(|i| i.id)
        .collect()
}

fn net_terminal_item_count(board: &Board, net_no: i32) -> usize {
    net_terminal_items(board, net_no).len()
}
