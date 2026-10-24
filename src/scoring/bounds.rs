//! Port of `core.scoring.BoardStatisticsBoundsCalculator`: lower bounds of trace length,
//! via count and bend count used by the V2 optimizer score.
//!
//! The original caches the result per board object (`WeakHashMap`); the bounds only
//! depend on pins, conduction areas and rules, which do not change while routing, so
//! the result is calculated on each call here.

use super::BoardStatisticsBounds;
use crate::board::{Board, ItemId, ItemKind, Unit};
use std::collections::{BTreeMap, BTreeSet};

struct Terminal {
    x: f64,
    y: f64,
    signal_layers: BTreeSet<i32>,
}

pub fn calculate_bounds(board: &Board) -> BoardStatisticsBounds {
    let mut min_trace_length = 0.0f64;
    let mut min_via_count = 0i32;
    let mut min_bend_count = 0i32;
    let resolution = if board.communication.resolution > 0 { board.communication.resolution } else { 1 } as f64;
    let board_unit_to_mm = Unit::scale(1.0, board.communication.unit, Unit::Mm) / resolution;
    for net_no in 1..=board.rules.nets.max_net_number() {
        if board.rules.nets.get(net_no).is_none() {
            continue;
        }
        let terminals = get_terminals(board, net_no);
        if terminals.len() < 2 {
            continue;
        }
        let (length, bends) = calculate_mst(&terminals);
        min_trace_length += length * board_unit_to_mm;
        min_bend_count += bends;
        min_via_count += calculate_minimum_via_count(board, net_no, &terminals);
    }
    BoardStatisticsBounds {
        min_trace_length_mm: Some(min_trace_length as f32),
        min_via_count: Some(min_via_count),
        min_bend_count: Some(min_bend_count),
    }
}

fn get_terminals(board: &Board, net_no: i32) -> Vec<Terminal> {
    let mut result = Vec::new();
    for id in crate::drc::net_incompletes::net_terminal_items(board, net_no) {
        if let Some(t) = to_terminal(board, id) {
            if !t.signal_layers.is_empty() {
                result.push(t);
            }
        }
    }
    result
}

fn to_terminal(board: &Board, id: ItemId) -> Option<Terminal> {
    let item = board.item(id);
    let mut signal_layers = BTreeSet::new();
    match &item.kind {
        ItemKind::Pin(pin) => {
            let center = pin.drill.center.to_float();
            for layer in pin.drill.first_layer..=pin.drill.last_layer {
                if pin.drill.get_shape((layer - pin.drill.first_layer) as usize).is_some() {
                    add_signal_layer(board, layer, &mut signal_layers);
                }
            }
            Some(Terminal {
                x: center.x,
                y: center.y,
                signal_layers,
            })
        }
        ItemKind::Obstacle(o) if item.is_conduction_area() => {
            let center = o.area.get_border().centre_of_gravity();
            add_signal_layer(board, o.layer, &mut signal_layers);
            Some(Terminal {
                x: center.x,
                y: center.y,
                signal_layers,
            })
        }
        _ => None,
    }
}

fn add_signal_layer(board: &Board, layer: i32, signal_layers: &mut BTreeSet<i32>) {
    if layer >= 0
        && (layer as usize) < board.layer_structure.layers.len()
        && board.layer_structure.layers[layer as usize].is_signal
    {
        signal_layers.insert(layer);
    }
}

/// Prim's minimum spanning tree with manhattan distances. Returns (length, bend count).
fn calculate_mst(terminals: &[Terminal]) -> (f64, i32) {
    let n = terminals.len();
    let mut used = vec![false; n];
    let mut distances = vec![f64::INFINITY; n];
    let mut parents = vec![-1i64; n];
    distances[0] = 0.0;
    let mut length = 0.0;
    let mut bend_count = 0;
    for _ in 0..n {
        let mut current: i64 = -1;
        for index in 0..n {
            if !used[index] && (current < 0 || distances[index] < distances[current as usize]) {
                current = index as i64;
            }
        }
        if current < 0 {
            break;
        }
        let current = current as usize;
        used[current] = true;
        if parents[current] >= 0 {
            let from = &terminals[parents[current] as usize];
            let to = &terminals[current];
            length += distances[current];
            if from.x != to.x && from.y != to.y {
                bend_count += 1;
            }
        }
        for index in 0..n {
            if !used[index] {
                let d = manhattan_distance(&terminals[current], &terminals[index]);
                if d < distances[index] {
                    distances[index] = d;
                    parents[index] = current as i64;
                }
            }
        }
    }
    (length, bend_count)
}

fn manhattan_distance(a: &Terminal, b: &Terminal) -> f64 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}

/// Union find over layers (`LayerGroups`). The original uses `HashMap<Integer,Integer>`,
/// whose iteration order for small layer numbers is ascending, like the `BTreeMap` here.
#[derive(Default)]
struct LayerGroups {
    parent: BTreeMap<i32, i32>,
}

impl LayerGroups {
    fn add_layers(&mut self, layers: &BTreeSet<i32>) {
        let mut first: Option<i32> = None;
        for &layer in layers {
            self.parent.entry(layer).or_insert(layer);
            match first {
                None => first = Some(layer),
                Some(f) => self.union(f, layer),
            }
        }
    }

    fn roots(&mut self) -> BTreeSet<i32> {
        let keys: Vec<i32> = self.parent.keys().copied().collect();
        keys.into_iter().map(|l| self.find(l)).collect()
    }

    fn count(&mut self) -> usize {
        self.roots().len()
    }

    fn groups_covered_by(&mut self, first_layer: i32, last_layer: i32) -> BTreeSet<i32> {
        let keys: Vec<i32> = self.parent.keys().copied().collect();
        let mut result = BTreeSet::new();
        for layer in keys {
            if layer >= first_layer && layer <= last_layer {
                result.insert(self.find(layer));
            }
        }
        result
    }

    fn find(&mut self, layer: i32) -> i32 {
        let mut p = self.parent[&layer];
        if p != layer {
            p = self.find(p);
            self.parent.insert(layer, p);
        }
        p
    }

    fn union(&mut self, a: i32, b: i32) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(rb, ra);
        }
    }
}

fn calculate_minimum_via_count(board: &Board, net_no: i32, terminals: &[Terminal]) -> i32 {
    let mut groups = LayerGroups::default();
    for t in terminals {
        groups.add_layers(&t.signal_layers);
    }
    if groups.count() <= 1 {
        return 0;
    }
    let Some(net_class) = board.rules.net_class_of(net_no) else { return 0 };
    let Some(rule_id) = net_class.get_via_rule() else { return 0 };
    let via_rule = board.rules.via_rule(rule_id);
    let mut remaining = groups.roots();
    let mut spans: Vec<(i32, i32)> = Vec::new();
    for index in 0..via_rule.via_count() {
        let info = board.rules.via_infos.get(via_rule.get_via(index));
        let padstack = board.library.padstacks.get(info.padstack);
        let (f, l) = (padstack.from_layer(), padstack.to_layer());
        spans.push((f.min(l), f.max(l)));
    }
    let mut via_count = 0;
    while !remaining.is_empty() {
        let mut best: Option<(i32, i32)> = None;
        let mut best_covered: BTreeSet<i32> = BTreeSet::new();
        for &(f, l) in &spans {
            let covered: BTreeSet<i32> = groups.groups_covered_by(f, l).intersection(&remaining).copied().collect();
            if covered.len() > best_covered.len() {
                best = Some((f, l));
                best_covered = covered;
            }
        }
        if best.is_none() || best_covered.is_empty() {
            break;
        }
        for g in &best_covered {
            remaining.remove(g);
        }
        via_count += 1;
    }
    via_count
}
