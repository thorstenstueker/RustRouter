//! Port of the headless parts of `drc.DesignRulesChecker` and `drc.UnconnectedItems`.

use super::clearance::{ClearanceChecker, ClearanceViolation};
use super::net_incompletes::{AirLine, NetIncompletes};
use crate::board::{Board, ItemId};
use std::collections::{BTreeMap, BTreeSet};

/// A pair of items which should be connected, or a dangling item (`second_item == None`).
#[derive(Clone, Debug)]
pub struct UnconnectedItems {
    pub first_item: ItemId,
    pub second_item: Option<ItemId>,
    pub all_items: Vec<ItemId>,
    /// "unconnectedItems", "track_dangling" or "via_dangling".
    pub kind: &'static str,
}

pub struct DesignRulesChecker<'a> {
    board: &'a Board,
    pub max_connections: usize,
    net_incompletes: Option<Vec<NetIncompletes>>,
}

impl<'a> DesignRulesChecker<'a> {
    pub fn new(board: &'a Board) -> DesignRulesChecker<'a> {
        DesignRulesChecker {
            board,
            max_connections: 0,
            net_incompletes: None,
        }
    }

    pub fn get_all_clearance_violations(&self) -> Vec<ClearanceViolation> {
        ClearanceChecker::new(self.board).all_clearance_violations()
    }

    pub fn get_all_unconnected_items(&self) -> Vec<UnconnectedItems> {
        let board = self.board;
        let mut result: Vec<UnconnectedItems> = Vec::new();
        let mut items_by_net: BTreeMap<i32, Vec<ItemId>> = BTreeMap::new();
        for item in board.items.iter() {
            if item.is_connectable_type() && item.net_count() > 0 {
                items_by_net.entry(item.get_net_number(0)).or_default().push(item.id);
            }
        }
        for (net_no, net_items) in &items_by_net {
            if net_items.len() <= 1 {
                continue;
            }
            let net_set: BTreeSet<ItemId> = net_items.iter().copied().collect();
            let mut connected_sets: Vec<BTreeSet<ItemId>> = Vec::new();
            let mut processed: BTreeSet<ItemId> = BTreeSet::new();
            for id in net_items {
                if processed.contains(id) {
                    continue;
                }
                let set: BTreeSet<ItemId> = board
                    .connected_set(*id, *net_no, false)
                    .into_iter()
                    .filter(|i| net_set.contains(i))
                    .collect();
                if !set.is_empty() {
                    processed.extend(set.iter().copied());
                    connected_sets.push(set);
                }
            }
            if connected_sets.len() >= 2 {
                let i1 = self.find_representative_item(&connected_sets[0]);
                let i2 = self.find_representative_item(&connected_sets[1]);
                if let (Some(i1), Some(i2)) = (i1, i2) {
                    let mut all: Vec<ItemId> = connected_sets[0].iter().copied().collect();
                    all.extend(connected_sets[1].iter().copied());
                    result.push(UnconnectedItems {
                        first_item: i1,
                        second_item: Some(i2),
                        all_items: all,
                        kind: "unconnectedItems",
                    });
                }
            }
        }
        for item in board.items.iter() {
            if item.is_trace()
                && (board.trace_start_contacts(item.id).is_empty() || board.trace_end_contacts(item.id).is_empty())
                && !result.iter().any(|u| u.first_item == item.id)
            {
                result.push(UnconnectedItems {
                    first_item: item.id,
                    second_item: None,
                    all_items: vec![item.id],
                    kind: "track_dangling",
                });
            }
        }
        for item in board.items.iter() {
            if item.is_via() && board.is_tail(item.id) {
                result.push(UnconnectedItems {
                    first_item: item.id,
                    second_item: None,
                    all_items: vec![item.id],
                    kind: "via_dangling",
                });
            }
        }
        result
    }

    fn find_representative_item(&self, set: &BTreeSet<ItemId>) -> Option<ItemId> {
        if let Some(i) = set.iter().find(|i| self.board.item(**i).is_pin()) {
            return Some(*i);
        }
        if let Some(i) = set.iter().find(|i| self.board.item(**i).is_trace()) {
            return Some(*i);
        }
        set.iter().next().copied()
    }

    pub fn calculate_all_incompletes(&mut self) {
        let board = self.board;
        let max_net_no = board.rules.nets.max_net_number().max(0) as usize;
        let mut lists: Vec<Vec<ItemId>> = vec![Vec::new(); max_net_no];
        for item in board.items.iter() {
            if item.is_connectable_type() {
                for &n in &item.net_numbers {
                    if n >= 1 && n as usize <= max_net_no {
                        lists[n as usize - 1].push(item.id);
                    }
                }
            }
        }
        self.max_connections = lists
            .iter()
            .filter(|l| !l.is_empty())
            .map(|l| {
                let endpoints = l
                    .iter()
                    .filter(|id| {
                        let i = board.item(**id);
                        i.is_pin() || i.is_conduction_area()
                    })
                    .count();
                endpoints.saturating_sub(1)
            })
            .sum();
        let mut result = Vec::with_capacity(max_net_no);
        for (i, l) in lists.iter().enumerate() {
            result.push(NetIncompletes::new(i as i32 + 1, l, board));
        }
        self.net_incompletes = Some(result);
    }

    fn ensure(&mut self) -> &mut Vec<NetIncompletes> {
        if self.net_incompletes.is_none() {
            self.calculate_all_incompletes();
        }
        self.net_incompletes.as_mut().unwrap()
    }

    pub fn recalculate_net_incompletes(&mut self, net_no: i32) {
        if self.net_incompletes.is_none() {
            self.calculate_all_incompletes();
            return;
        }
        let board = self.board;
        let list = self.net_incompletes.as_mut().unwrap();
        if net_no >= 1 && net_no as usize <= list.len() {
            let items = board.get_connectable_items(net_no);
            list[net_no as usize - 1] = NetIncompletes::new(net_no, &items, board);
        }
    }

    pub fn recalculate_net_incompletes_with_items(&mut self, net_no: i32, items: &[ItemId]) {
        let board = self.board;
        let list = self.ensure();
        if net_no >= 1 && net_no as usize <= list.len() {
            list[net_no as usize - 1] = NetIncompletes::new(net_no, items, board);
        }
    }

    pub fn get_incomplete_count(&mut self) -> usize {
        self.ensure().iter().map(|n| n.count()).sum()
    }

    pub fn incomplete_net_numbers(&mut self) -> BTreeSet<i32> {
        self.ensure()
            .iter()
            .enumerate()
            .filter(|(_, n)| n.count() > 0)
            .map(|(i, _)| i as i32 + 1)
            .collect()
    }

    pub fn get_incomplete_count_of_net(&mut self, net_no: i32) -> usize {
        let list = self.ensure();
        if net_no <= 0 || net_no as usize > list.len() {
            return 0;
        }
        list[net_no as usize - 1].count()
    }

    pub fn get_length_violation_count(&mut self) -> usize {
        self.ensure().iter().filter(|n| n.get_length_violation() != 0.0).count()
    }

    pub fn get_length_violation(&mut self, net_no: i32) -> f64 {
        let list = self.ensure();
        if net_no <= 0 || net_no as usize > list.len() {
            return 0.0;
        }
        list[net_no as usize - 1].get_length_violation()
    }

    pub fn recalculate_length_violations(&mut self) -> bool {
        if self.net_incompletes.is_none() {
            self.calculate_all_incompletes();
            return true;
        }
        let board = self.board;
        let mut result = false;
        for n in self.net_incompletes.as_mut().unwrap().iter_mut() {
            if n.calc_length_violation(board) {
                result = true;
            }
        }
        result
    }

    pub fn get_all_airlines(&mut self) -> Vec<AirLine> {
        self.ensure().iter().flat_map(|n| n.incompletes.iter().cloned()).collect()
    }

    pub fn get_net_incompletes(&mut self, net_no: i32) -> Option<&NetIncompletes> {
        let list = self.ensure();
        if net_no <= 0 || net_no as usize > list.len() {
            return None;
        }
        Some(&list[net_no as usize - 1])
    }
}
