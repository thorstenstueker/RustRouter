//! The autorouter (port of `app.freerouting.autoroute`).

pub mod complete_shape;
pub mod connection;
pub mod control;
pub mod engine;
pub mod insert;
pub mod locate;
pub mod maze;
pub mod neighbours;
pub mod pipeline;

pub use control::AutorouteControl;
pub use engine::AutorouteEngine;

use crate::board::connectivity::StopConnectionOption;
use crate::board::{Board, ItemId};
use crate::datastructures::{StopFlag, TimeLimit};
use crate::settings::RouterSettings;
use std::collections::{BTreeSet, HashMap};

/// `AutorouteAttemptState`
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AutorouteAttemptState {
    Unknown,
    Skipped,
    NoUnconnectedNets,
    ConnectedToPlane,
    AlreadyConnected,
    NoConnections,
    Routed,
    Failed,
    InsertError,
}

/// `AutorouteAttemptResult`
#[derive(Clone, Debug)]
pub struct AutorouteAttemptResult {
    pub state: AutorouteAttemptState,
    pub details: String,
}

impl AutorouteAttemptResult {
    pub fn new(state: AutorouteAttemptState) -> Self {
        AutorouteAttemptResult {
            state,
            details: String::new(),
        }
    }

    pub fn with(state: AutorouteAttemptState, details: impl Into<String>) -> Self {
        AutorouteAttemptResult {
            state,
            details: details.into(),
        }
    }
}

/// Invalidates the parts of the autoroute database overlapping a changed item
/// (`RoutingBoard.additionalUpdateAfterChange`).
pub fn invalidate_for_item(board: &mut Board, id: ItemId) {
    if let Some(mut eng) = board.autoroute_db.take() {
        eng.update_after_item_change(board, id);
        board.autoroute_db = Some(eng);
    }
}

fn describe(set: &BTreeSet<ItemId>) -> String {
    set.iter().map(|i| format!("#{}", i.0)).collect::<Vec<_>>().join(", ")
}

impl Board {
    /// `initAutoroute`: creates (or reuses) the autoroute engine for a connection.
    pub fn init_autoroute(
        &mut self,
        net_no: i32,
        trace_clearance_class: i32,
        stop: Option<StopFlag>,
        time_limit: Option<TimeLimit>,
        retain_database: bool,
    ) {
        let reuse = match &self.autoroute_db {
            Some(e) => retain_database && e.compensated_clearance_class(self) == trace_clearance_class,
            None => false,
        };
        if !reuse {
            let eng = AutorouteEngine::new(self, trace_clearance_class, retain_database);
            self.autoroute_db = Some(Box::new(eng));
        }
        let mut eng = self.autoroute_db.take().unwrap();
        eng.init_connection(self, net_no, stop, time_limit);
        self.autoroute_db = Some(eng);
    }

    /// `finishAutoroute`
    pub fn finish_autoroute(&mut self) {
        if let Some(mut eng) = self.autoroute_db.take() {
            eng.clear(self);
        }
    }

    /// `clearTransientAutorouteState`
    pub fn clear_transient_autoroute_state(&mut self) {
        self.finish_autoroute();
        self.changed_area = None;
        self.shove_failing_obstacle = None;
        self.shove_failing_layer = -1;
    }

    /// `AutorouteEngine.autorouteConnection` on the engine created by `init_autoroute`.
    pub fn autoroute_connection(
        &mut self,
        start_set: &BTreeSet<ItemId>,
        dest_set: &BTreeSet<ItemId>,
        ctrl: &AutorouteControl,
        ripped_items: &mut BTreeSet<ItemId>,
        ripup_costs: Option<&mut HashMap<ItemId, i32>>,
    ) -> AutorouteAttemptResult {
        let Some(mut eng) = self.autoroute_db.take() else {
            return AutorouteAttemptResult::with(AutorouteAttemptState::Failed, "autoroute engine not initialized");
        };
        let search_result = match maze::MazeSearch::get_instance(&mut eng, self, ctrl, start_set, dest_set) {
            None => {
                self.end_connection_search(&mut eng);
                self.autoroute_db = Some(eng);
                return AutorouteAttemptResult::with(
                    AutorouteAttemptState::Failed,
                    format!(
                        "Failed to route connection between {} and {}, because the maze search algorithm could not be created.",
                        describe(start_set),
                        describe(dest_set)
                    ),
                );
            }
            Some(mut search) => search.find_connection(&mut eng, self),
        };
        let located = search_result.and_then(|r| locate::locate(&mut eng, self, ctrl, &r, ripped_items, ripup_costs));
        self.end_connection_search(&mut eng);
        self.autoroute_db = Some(eng);
        if search_result.is_none() {
            return AutorouteAttemptResult::with(
                AutorouteAttemptState::Failed,
                format!(
                    "Failed to route connection between {} and {}, because no connection was found between their nets.",
                    describe(start_set),
                    describe(dest_set)
                ),
            );
        }
        let Some(found) = located else {
            return AutorouteAttemptResult::with(
                AutorouteAttemptState::Failed,
                format!("Failed to route connection between {} and {}.", describe(start_set), describe(dest_set)),
            );
        };
        if !ctrl.layer_active[found.start_layer as usize] || !ctrl.layer_active[found.target_layer as usize] {
            return AutorouteAttemptResult::with(
                AutorouteAttemptState::Failed,
                format!(
                    "Failed to route connection between {} and {}, because some of their layers are disabled.",
                    describe(start_set),
                    describe(dest_set)
                ),
            );
        }
        let start_item = found.start_item.and_then(|i| self.try_item(i).cloned());
        let target_item = found.target_item.and_then(|i| self.try_item(i).cloned());
        rip_connections(self, ripped_items, ctrl.remove_unconnected_vias);
        if !insert::insert_found_connection(self, ctrl, &found, start_item, target_item) {
            return AutorouteAttemptResult::with(
                AutorouteAttemptState::Failed,
                format!(
                    "Failed to route connection between {} and {}, because the new connection could not be inserted.",
                    describe(start_set),
                    describe(dest_set)
                ),
            );
        }
        AutorouteAttemptResult::new(AutorouteAttemptState::Routed)
    }

    /// The end of `autorouteConnection` before the insertion: `clear` or `resetAllDoors`.
    fn end_connection_search(&mut self, eng: &mut AutorouteEngine) {
        if !eng.maintain_database {
            eng.clear(self);
        } else {
            eng.reset_all_doors();
        }
    }

    /// `RoutingBoard.autoroute(item, settings, viaCosts, ...)`
    pub fn autoroute_item(
        &mut self,
        item: ItemId,
        settings: &RouterSettings,
        via_costs: i32,
        stop: Option<StopFlag>,
        time_limit: Option<TimeLimit>,
    ) -> AutorouteAttemptResult {
        self.clear_removed_items();
        let it = self.item(item);
        if !it.is_connectable_type() || it.net_count() == 0 {
            return AutorouteAttemptResult::with(AutorouteAttemptState::NoConnections, format!("The item '#{}' is not connectable.", item.0));
        }
        if it.net_count() > 1 {
            log::warn!("RoutingBoard.autoroute: netCount > 1 not yet implemented");
        }
        let net_no = it.get_net_number(0);
        let mut ctrl = AutorouteControl::with_costs(self, net_no, settings, via_costs, settings.get_trace_costs());
        ctrl.remove_unconnected_vias = false;
        let start_set = self.connected_set(item, net_no, false);
        if let Some(net) = self.rules.nets.get(net_no) {
            if net.contains_plane() {
                for id in &start_set {
                    if self.item(*id).is_conduction_area() {
                        return AutorouteAttemptResult::with(
                            AutorouteAttemptState::ConnectedToPlane,
                            format!("The item '#{}' is connected to a plane.", id.0),
                        );
                    }
                }
            }
        }
        let dest_set = self.unconnected_set(item, net_no);
        if dest_set.is_empty() {
            return AutorouteAttemptResult::with(AutorouteAttemptState::AlreadyConnected, format!("The item '#{}' is already connected.", item.0));
        }
        let mut ripped = BTreeSet::new();
        self.init_autoroute(net_no, ctrl.trace_clearance_class, stop.clone(), time_limit, false);
        let result = self.autoroute_connection(&start_set, &dest_set, &ctrl, &mut ripped, None);
        if result.state == AutorouteAttemptState::Routed {
            self.opt_changed_area(&[net_no], None, settings.trace_pull_tight_accuracy, Some(&ctrl.trace_costs), stop, 1000, None, -1);
        }
        result
    }
}

/// `AutorouteEngine.ripConnections`: removes the connections of the ripped items.
pub fn rip_connections(board: &mut Board, ripped_items: &BTreeSet<ItemId>, remove_unconnected_vias: bool) -> BTreeSet<i32> {
    let mut ripped_connections: BTreeSet<ItemId> = BTreeSet::new();
    let mut changed_nets: BTreeSet<i32> = BTreeSet::new();
    let stop = if remove_unconnected_vias {
        StopConnectionOption::None
    } else {
        StopConnectionOption::FanoutVia
    };
    for id in ripped_items {
        if board.try_item(*id).is_none() {
            continue;
        }
        ripped_connections.extend(board.connection_items(*id, stop));
        let it = board.item(*id);
        for i in 0..it.net_count() {
            changed_nets.insert(it.get_net_number(i));
        }
    }
    let ids: Vec<ItemId> = ripped_connections.into_iter().collect();
    board.remove_items(&ids);
    for n in &changed_nets {
        board.remove_trace_tails(*n, stop);
    }
    changed_nets
}
