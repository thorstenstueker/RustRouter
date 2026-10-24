# API requests from the drc/scoring port

1. **Scoring versions in settings** (`src/settings.rs`): Java `RouterScoreSettings.version`
   (`RouterScoringVersion`, default `V2_CONTINUOUS`) and `OptimizerScoreSettings.version`
   (`OptimizerScoringVersion`, default `V2_LOWER_BOUND`) are missing. Workaround:
   `scoring::RouterScoringVersion` / `OptimizerScoringVersion` enums;
   `BoardStatistics::get_router_score(&RouterSettings)` / `get_optimizer_score` use the Java
   defaults, `*_with_version(..)` take an explicit version. Suggest adding
   `pub version: crate::scoring::RouterScoringVersion` etc. to the settings structs.

2. **Routing ledger hooks** (`src/board/mod.rs`): Java `BasicBoard` owns a transient
   `NetRoutingLedger` and calls `noteInserted(item)` / `noteRemoved(item)` in
   `insertItem`/`removeItem` and `invalidate()` in `deleteAllTracksAndVias`.
   Workaround: `drc::NetRoutingLedger::new()` (auto sync) compares the per-net item sets
   whenever `board.items.version` changed and recounts the differing nets (same effect as
   the notifications, except that in-place net changes of an item are also detected).
   For an exact replica add `pub routing_ledger: NetRoutingLedger` (created with
   `NetRoutingLedger::new_notified()`) to `Board` and call `note_inserted(&item)` after
   `ItemStore::insert` and `note_removed(&item)` after `ItemStore::remove`; reset it
   (`invalidate()`) on snapshot restore. Callers then use
   `board.routing_ledger.incomplete_count(&board)` (needs a split borrow or
   `std::mem::take`).

3. **Clearance baseline on the board**: Java `BasicBoard.clearanceBaseline`
   (`transient ClearanceBaseline`) is missing. Workaround: pass it via
   `scoring::StatisticsOptions { clearance_baseline: Some(&baseline), .. }`. Suggest
   `pub clearance_baseline: Option<crate::drc::ClearanceBaseline>` on `Board`.

4. **Uncompensated DRC tree without `&mut`**: `Board::get_uncompensated_tree` needs
   `&mut self`. When the board has no uncompensated default tree (clearance compensation
   on, i.e. during autorouting), `drc::ClearanceChecker::new(&board)` builds a temporary
   tree (same content as Java's permanently added tree, but rebuilt on every checker
   construction). Suggest calling `board.get_uncompensated_tree()` once after loading /
   when compensation is switched on, so that `search_trees.find(true, 0)` finds it.

5. **`Item.smallest_clearance`** is not updated by `clearance_violations` (it takes
   `&Board`). `ClearanceViolation::smallest_clearance` computes the value from the
   violations instead. If some code relies on the cached field, a `&mut Board` variant
   would be needed.
