//! Design rule checks (port of `app.freerouting.drc`, headless parts only):
//! incomplete connections per net (`NetRoutingLedger`, `NetIncompletes`, `AirLine`),
//! clearance violations (`Item.clearanceViolations`, `ClearanceViolation`,
//! `ClearanceBaseline`) and the `DesignRulesChecker`.
//!
//! Not ported: zone island detection (needs `java.awt.geom.Area`), KiCad DRC reports,
//! JSON summaries, GUI printing.

pub mod checker;
pub mod clearance;
pub mod delaunay;
pub mod ledger;
pub mod net_incompletes;

pub use checker::{DesignRulesChecker, UnconnectedItems};
pub use clearance::{
    all_clearance_violations, clearance_violation_count, clearance_violations, ClearanceBaseline, ClearanceChecker,
    ClearanceViolation, ClearanceViolationCategory,
};
pub use delaunay::{PlanarDelaunayTriangulation, ResultEdge};
pub use ledger::NetRoutingLedger;
pub use net_incompletes::{AirLine, NetIncompletes};

use crate::board::Board;
use std::collections::BTreeSet;

/// The number of incomplete connections of the board (`routingLedger().incompleteCount()`
/// computed from scratch).
pub fn incomplete_count(board: &Board) -> usize {
    NetRoutingLedger::new().incomplete_count(board)
}

/// The nets with incomplete connections (`routingLedger().incompleteNetNumbers()`).
pub fn incomplete_net_numbers(board: &Board) -> BTreeSet<i32> {
    NetRoutingLedger::new().incomplete_net_numbers(board)
}

/// The maximum number of connections (`routingLedger().maximumConnections()`).
pub fn maximum_connections(board: &Board) -> usize {
    NetRoutingLedger::new().maximum_connections(board)
}

/// The incompletes of one net (`new NetIncompletes(net, board.getConnectableItems(net), board)`).
pub fn net_incompletes(board: &Board, net_no: i32) -> NetIncompletes {
    NetIncompletes::new(net_no, &board.get_connectable_items(net_no), board)
}

/// All airlines of the board (`DesignRulesChecker.getAllAirlines()`).
pub fn all_airlines(board: &Board) -> Vec<AirLine> {
    DesignRulesChecker::new(board).get_all_airlines()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Communication, FixedState, Layer, LayerStructure};
    use crate::geometry::{IntBox, IntPoint, Point, Polyline, Shape, TileShape};
    use crate::rules::{BoardRules, ClearanceMatrix};

    fn p(x: i32, y: i32) -> Point {
        Point::Int(IntPoint { x, y })
    }

    fn test_board() -> Board {
        let ls = LayerStructure::new(vec![Layer::new("F.Cu", true), Layer::new("B.Cu", true)]);
        let mut rules = BoardRules::new(&ls, ClearanceMatrix::get_default_instance(2, 200));
        rules.nets.add("N1", 1, false);
        rules.nets.add("N2", 1, false);
        let mut board = Board::new(
            IntBox::new(-100_000, -100_000, 100_000, 100_000),
            ls,
            Vec::new(),
            1,
            rules,
            Communication::default(),
        );
        let pad = Shape::Tile(TileShape::Box(IntBox::new(-300, -300, 300, 300)));
        board.library.padstacks.add("via", vec![Some(pad.clone()), Some(pad)], None, true, false);
        board
    }

    #[test]
    fn incompletes_and_clearance() {
        let mut board = test_board();
        let ps = crate::library::PadstackId(1);
        board.insert_via(ps, &p(0, 0), &[1], 1, FixedState::Unfixed, true);
        board.insert_via(ps, &p(10_000, 0), &[1], 1, FixedState::Unfixed, true);
        board.insert_via(ps, &p(10_000, 10_000), &[1], 1, FixedState::Unfixed, true);
        let mut ledger = NetRoutingLedger::new();
        // vias are tails (no contacts) and therefore ignored like in the original
        assert_eq!(ledger.incomplete_count(&board), 0);
        board.insert_trace_without_cleaning(
            Polyline::from_points(&[p(0, 0), p(10_000, 0)]),
            0,
            100,
            &[1],
            1,
            FixedState::Unfixed,
        );
        assert_eq!(incomplete_count(&board), 0);
        assert_eq!(all_clearance_violations(&board).len(), 0);
        // a trace of net 2 crossing the net 1 trace
        board.insert_trace_without_cleaning(
            Polyline::from_points(&[p(5_000, -5_000), p(5_000, 5_000)]),
            0,
            100,
            &[2],
            1,
            FixedState::Unfixed,
        );
        let v = all_clearance_violations(&board);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].actual_clearance, 0.0);
        assert_eq!(v[0].expected_clearance, 200.0);
        let stats = crate::scoring::BoardStatistics::new(&board);
        assert_eq!(stats.clearance_violations.total_count, Some(1));
        assert_eq!(stats.traces.total_count, 2);
        assert_eq!(ledger.incomplete_count(&board), 0);
    }

    #[test]
    fn two_connected_groups_give_one_airline() {
        let mut board = test_board();
        let ps = crate::library::PadstackId(1);
        // a via is no tail, if it has contacts on different layers
        for x in [0, 40_000] {
            board.insert_via(ps, &p(x, 0), &[1], 1, FixedState::Unfixed, true);
            board.insert_trace_without_cleaning(Polyline::from_points(&[p(x, 0), p(x, 5_000)]), 0, 100, &[1], 1, FixedState::Unfixed);
            board.insert_trace_without_cleaning(Polyline::from_points(&[p(x, 0), p(x, -5_000)]), 1, 100, &[1], 1, FixedState::Unfixed);
        }
        let mut ledger = NetRoutingLedger::new();
        assert_eq!(ledger.incomplete_count(&board), 1);
        assert_eq!(ledger.incomplete_net_numbers(&board).into_iter().collect::<Vec<_>>(), vec![1]);
        let airlines = all_airlines(&board);
        assert_eq!(airlines.len(), 1);
        let a = &airlines[0];
        assert_eq!((a.from_corner.x.min(a.to_corner.x), a.from_corner.x.max(a.to_corner.x)), (0.0, 40_000.0));
        // connect the groups: the ledger notices the insertion
        board.insert_trace_without_cleaning(Polyline::from_points(&[p(0, 0), p(40_000, 0)]), 0, 100, &[1], 1, FixedState::Unfixed);
        assert_eq!(ledger.incomplete_count(&board), 0);
        assert_eq!(incomplete_count(&board), 0);
        let stats = crate::scoring::BoardStatistics::new(&board);
        assert_eq!(stats.connections.incomplete_count, Some(0));
    }
}
