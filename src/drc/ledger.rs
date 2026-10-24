//! Port of `drc.NetRoutingLedger`: the incrementally maintained count of incomplete
//! (unrouted) connections per net.
//!
//! In the original the board owns the ledger and notifies it on every item insertion
//! and removal (`noteInserted`/`noteRemoved`). The Rust board has no such hooks, so the
//! ledger lives outside of the board and has two modes:
//!
//! * **notified** (`note_inserted`/`note_removed` are called by the owner of the board):
//!   identical to the original;
//! * **auto sync** (default): when the insertion/removal counter `board.items.version`
//!   changed since the last query, the per-net item lists are rebuilt from the board and
//!   every net whose item set differs from the recorded one is marked dirty. This
//!   reproduces the effect of the notifications (a net is recounted when an item of it
//!   was inserted or removed).
//!
//! A ledger must only be used with one board (and its successive states). After a
//! board is replaced by a snapshot, call [`NetRoutingLedger::invalidate`] (the original
//! rebuilds the transient ledger after deserialization).

use super::net_incompletes::NetIncompletes;
use crate::board::{Board, Item, ItemId};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default)]
pub struct NetRoutingLedger {
    built: bool,
    items_by_net: Vec<Vec<ItemId>>,
    incomplete_by_net: Vec<usize>,
    incomplete_total: usize,
    maximum_connections: usize,
    dirty_nets: BTreeSet<i32>,
    /// `board.items.version` at the last build/sync.
    synced_version: u64,
    /// Whether the ledger synchronizes itself from `board.items.version`.
    auto_sync: bool,
}

impl NetRoutingLedger {
    /// A ledger in auto sync mode.
    pub fn new() -> NetRoutingLedger {
        NetRoutingLedger {
            auto_sync: true,
            ..Default::default()
        }
    }

    /// A ledger which relies on `note_inserted`/`note_removed` (like the original).
    pub fn new_notified() -> NetRoutingLedger {
        NetRoutingLedger {
            auto_sync: false,
            ..Default::default()
        }
    }

    pub fn invalidate(&mut self) {
        self.built = false;
        self.dirty_nets.clear();
        self.items_by_net = Vec::new();
        self.incomplete_by_net = Vec::new();
        self.incomplete_total = 0;
        self.maximum_connections = 0;
    }

    /// Records an insertion. Ignored until the ledger has been built.
    pub fn note_inserted(&mut self, item: &Item) {
        if !self.built || !item.is_connectable_type() {
            return;
        }
        self.add_item_to_lists(item);
        self.mark_nets(item);
    }

    /// Records a removal. Ignored until the ledger has been built.
    pub fn note_removed(&mut self, item: &Item) {
        if !self.built || !item.is_connectable_type() {
            return;
        }
        for &net_no in &item.net_numbers {
            if net_no >= 1 && (net_no as usize) < self.items_by_net.len() {
                let list = &mut self.items_by_net[net_no as usize];
                if let Some(p) = list.iter().position(|i| *i == item.id) {
                    list.remove(p);
                }
                self.dirty_nets.insert(net_no);
            }
        }
    }

    pub fn incomplete_count(&mut self, board: &Board) -> usize {
        self.ensure_built(board);
        self.sync(board);
        self.flush_dirty(board);
        self.incomplete_total
    }

    pub fn maximum_connections(&mut self, board: &Board) -> usize {
        self.ensure_built(board);
        // The original does not flush here; in auto sync mode the dirty nets are
        // collected, but (like the original) only flushed by the count queries.
        self.sync(board);
        self.maximum_connections
    }

    pub fn incomplete_net_numbers(&mut self, board: &Board) -> BTreeSet<i32> {
        self.ensure_built(board);
        self.sync(board);
        self.flush_dirty(board);
        let mut result = BTreeSet::new();
        for net_no in 1..self.incomplete_by_net.len() {
            if self.incomplete_by_net[net_no] > 0 {
                result.insert(net_no as i32);
            }
        }
        result
    }

    fn collect_lists(board: &Board, max_net: i32) -> Vec<Vec<ItemId>> {
        let mut lists: Vec<Vec<ItemId>> = vec![Vec::new(); max_net as usize + 1];
        for item in board.items.iter() {
            if item.is_connectable_type() {
                for &net_no in &item.net_numbers {
                    if net_no >= 1 && (net_no as usize) < lists.len() {
                        let l = &mut lists[net_no as usize];
                        // items are visited once, so only a repeated net of the same item can
                        // produce a duplicate
                        if l.last() != Some(&item.id) {
                            l.push(item.id);
                        }
                    }
                }
            }
        }
        lists
    }

    fn ensure_built(&mut self, board: &Board) {
        if self.built {
            return;
        }
        let max_net = board.rules.nets.max_net_number().max(0);
        self.items_by_net = Self::collect_lists(board, max_net);
        self.incomplete_by_net = vec![0; max_net as usize + 1];
        self.maximum_connections = self.count_maximum_connections(board);
        self.incomplete_total = 0;
        for net_no in 1..=max_net {
            let count = self.count_net(board, net_no);
            self.incomplete_by_net[net_no as usize] = count;
            self.incomplete_total += count;
        }
        self.dirty_nets.clear();
        self.synced_version = board.items.version;
        self.built = true;
    }

    fn sync(&mut self, board: &Board) {
        if !self.auto_sync || board.items.version == self.synced_version {
            return;
        }
        self.synced_version = board.items.version;
        let max_net = board.rules.nets.max_net_number().max(0);
        if max_net as usize + 1 != self.items_by_net.len() {
            self.invalidate();
            self.ensure_built(board);
            return;
        }
        let fresh = Self::collect_lists(board, max_net);
        for net_no in 1..fresh.len() {
            let old = &self.items_by_net[net_no];
            let new = &fresh[net_no];
            let differs = old.len() != new.len() || {
                let a: BTreeSet<ItemId> = old.iter().copied().collect();
                new.iter().any(|i| !a.contains(i))
            };
            if differs {
                self.dirty_nets.insert(net_no as i32);
            }
        }
        self.items_by_net = fresh;
    }

    fn flush_dirty(&mut self, board: &Board) {
        if self.dirty_nets.is_empty() {
            return;
        }
        let dirty: Vec<i32> = self.dirty_nets.iter().copied().collect();
        for net_no in dirty {
            let count = self.count_net(board, net_no);
            let idx = net_no as usize;
            self.incomplete_total = self.incomplete_total + count - self.incomplete_by_net[idx];
            self.incomplete_by_net[idx] = count;
        }
        self.dirty_nets.clear();
        self.maximum_connections = self.count_maximum_connections(board);
    }

    fn count_net(&self, board: &Board, net_no: i32) -> usize {
        if net_no < 1 || net_no as usize >= self.items_by_net.len() {
            return 0;
        }
        NetIncompletes::new(net_no, &self.items_by_net[net_no as usize], board).count()
    }

    fn count_maximum_connections(&self, board: &Board) -> usize {
        let mut maximum = 0usize;
        for net_no in 1..self.items_by_net.len() {
            let endpoints = self.items_by_net[net_no]
                .iter()
                .filter(|id| board.try_item(**id).map(|i| i.is_pin() || i.is_conduction_area()).unwrap_or(false))
                .count();
            maximum += endpoints.saturating_sub(1);
        }
        maximum
    }

    fn add_item_to_lists(&mut self, item: &Item) {
        for &net_no in &item.net_numbers {
            if net_no >= 1 && (net_no as usize) < self.items_by_net.len() {
                let l = &mut self.items_by_net[net_no as usize];
                if !l.contains(&item.id) {
                    l.push(item.id);
                }
            }
        }
    }

    fn mark_nets(&mut self, item: &Item) {
        for &net_no in &item.net_numbers {
            if net_no >= 1 && (net_no as usize) < self.items_by_net.len() {
                self.dirty_nets.insert(net_no);
            }
        }
    }
}
