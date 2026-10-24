//! Board copies and hashes (port of `BoardSnapshotManager` and the Java serialization
//! round trip used for board snapshots).

use super::{Board, ItemId, ItemKind, SearchTreeManager};
use crate::autoroute::AutorouteAttemptState;
use crate::geometry::Point;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};

/// `RoutingFailureLog`: failures per item id (serialized with the board).
#[derive(Clone, Debug, Default)]
pub struct RoutingFailureLog {
    failures: FxHashMap<i32, ItemFailureInfo>,
}

#[derive(Clone, Debug)]
pub struct ItemFailureInfo {
    pub net_no: i32,
    pub failure_count: i32,
    pub last_failure_state: Option<AutorouteAttemptState>,
    pub last_failure_reason: String,
    pub last_attempt_pass: i64,
}

impl RoutingFailureLog {
    pub const FAILURE_THRESHOLD: i32 = 50;

    pub fn record_failure(&mut self, item: ItemId, net_no: i32, pass_no: i32, state: AutorouteAttemptState, reason: &str) {
        let e = self.failures.entry(item.0).or_insert_with(|| ItemFailureInfo {
            net_no,
            failure_count: 0,
            last_failure_state: None,
            last_failure_reason: String::new(),
            last_attempt_pass: 0,
        });
        e.failure_count += 1;
        e.last_attempt_pass = pass_no as i64;
        e.last_failure_state = Some(state);
        e.last_failure_reason = reason.to_string();
    }

    pub fn should_skip(&self, item: ItemId) -> bool {
        self.failures
            .get(&item.0)
            .map(|i| i.failure_count >= Self::FAILURE_THRESHOLD)
            .unwrap_or(false)
    }

    pub fn get_failure_count(&self, item: ItemId) -> i32 {
        self.failures.get(&item.0).map(|i| i.failure_count).unwrap_or(0)
    }

    pub fn clear(&mut self) {
        self.failures.clear();
    }
}

fn hash_point<H: Hasher>(p: &Point, h: &mut H) {
    match p {
        Point::Int(ip) => {
            0u8.hash(h);
            ip.x.hash(h);
            ip.y.hash(h);
        }
        Point::Rational(rp) => {
            1u8.hash(h);
            format!("{rp:?}").hash(h);
        }
    }
}

impl Board {
    /// A copy of the board like `BasicBoard.deserialize(board.serialize(false))`: the transient
    /// data (search trees, autoroute database, changed area, shove state) is not copied and
    /// the default search tree is rebuilt by inserting the items in their iteration order.
    pub fn snapshot_copy(&self) -> Board {
        let mut b = Board {
            items: self.items.clone(),
            search_trees: SearchTreeManager::default(),
            components: self.components.clone(),
            rules: self.rules.clone(),
            library: self.library.clone(),
            layer_structure: self.layer_structure.clone(),
            communication: self.communication.clone(),
            bounding_box: self.bounding_box,
            changed_area: None,
            max_trace_half_width: self.max_trace_half_width,
            min_trace_half_width: self.min_trace_half_width,
            pre_existing_clearance_violations_count: self.pre_existing_clearance_violations_count,
            normalize_suppressed_nets: Default::default(),
            autoroute_db: None,
            shove_failing_obstacle: None,
            shove_failing_layer: -1,
            shove_failing_obstacle_writes: 0,
            shove_failing_layer_writes: 0,
            shove_cache: Default::default(),
            failure_log: self.failure_log.clone(),
            clearance_baseline: None,
            removed_items: Default::default(),
        };
        for id in b.items.ids() {
            b.insert_into_trees(id);
        }
        b
    }

    /// The serialized form of the board (`serialize(false)`): a copy without search trees.
    pub fn serialize(&self) -> Board {
        Board {
            items: self.items.clone(),
            search_trees: SearchTreeManager::default(),
            components: self.components.clone(),
            rules: self.rules.clone(),
            library: self.library.clone(),
            layer_structure: self.layer_structure.clone(),
            communication: self.communication.clone(),
            bounding_box: self.bounding_box,
            changed_area: None,
            max_trace_half_width: self.max_trace_half_width,
            min_trace_half_width: self.min_trace_half_width,
            pre_existing_clearance_violations_count: self.pre_existing_clearance_violations_count,
            normalize_suppressed_nets: Default::default(),
            autoroute_db: None,
            shove_failing_obstacle: None,
            shove_failing_layer: -1,
            shove_failing_obstacle_writes: 0,
            shove_failing_layer_writes: 0,
            shove_cache: Default::default(),
            failure_log: self.failure_log.clone(),
            clearance_baseline: None,
            removed_items: Default::default(),
        }
    }

    /// `BasicBoard.deserialize(serialized)`: rebuilds the default search tree.
    pub fn deserialize(serialized: &Board) -> Board {
        let mut b = serialized.serialize();
        for id in b.items.ids() {
            b.insert_into_trees(id);
        }
        b
    }

    /// `RoutingBoard.deepCopy()`
    pub fn deep_copy(&self) -> Board {
        // The copy has no autoroute engine and no item autoroute infos.
        self.snapshot_copy()
    }

    /// A content hash of the board (`getHash`): equal for boards with equal items.
    pub fn content_hash(&self) -> u64 {
        let mut h = rustc_hash::FxHasher::default();
        for item in self.items.iter() {
            item.id.0.hash(&mut h);
            item.net_numbers.as_slice().hash(&mut h);
            item.clearance_class.hash(&mut h);
            item.component_id.hash(&mut h);
            (item.fixed_state as u8).hash(&mut h);
            match &item.kind {
                ItemKind::Trace(t) => {
                    1u8.hash(&mut h);
                    t.layer.hash(&mut h);
                    t.half_width.hash(&mut h);
                    for l in t.polyline.lines() {
                        l.a.x.hash(&mut h);
                        l.a.y.hash(&mut h);
                        l.b.x.hash(&mut h);
                        l.b.y.hash(&mut h);
                    }
                }
                ItemKind::Via(v) => {
                    2u8.hash(&mut h);
                    hash_point(&v.drill.center, &mut h);
                    v.drill.padstack.0.hash(&mut h);
                    v.attach_allowed.hash(&mut h);
                }
                ItemKind::Pin(p) => {
                    3u8.hash(&mut h);
                    hash_point(&p.drill.center, &mut h);
                    p.pin_index.hash(&mut h);
                }
                ItemKind::Obstacle(_) => 4u8.hash(&mut h),
                ItemKind::ComponentOutline(_) => 5u8.hash(&mut h),
                ItemKind::BoardOutline(_) => 6u8.hash(&mut h),
            }
        }
        h.finish()
    }

    /// `getHash()` as hex string.
    pub fn get_hash(&self) -> String {
        format!("{:016x}", self.content_hash())
    }

    /// `getSmdPins`: pins on a single layer.
    pub fn get_smd_pins(&self) -> Vec<ItemId> {
        let lc = self.layer_count();
        self.items
            .iter()
            .filter(|i| i.is_pin() && i.first_layer() == i.last_layer_with(lc))
            .map(|i| i.id)
            .collect()
    }
}

/// Debug event log of item insertions and removals (enabled with the environment variable
/// `RR_EVLOG=<file>`), in the format of the instrumented Java harness.
pub mod evlog {
    use super::super::{Item, ItemKind};
    use crate::geometry::basics::java_round;
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};

    static LOG: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

    fn log() -> &'static Option<Mutex<std::fs::File>> {
        LOG.get_or_init(|| std::env::var_os("RR_EVLOG").and_then(|f| std::fs::File::create(f).ok()).map(Mutex::new))
    }

    #[inline]
    pub fn enabled() -> bool {
        log().is_some()
    }

    pub fn describe(item: &Item) -> String {
        let mut s = format!("{} {} nets=", item.id.0, java_type_name(item));
        for n in &item.net_numbers {
            s.push_str(&format!("{n},"));
        }
        match &item.kind {
            ItemKind::Trace(t) => {
                s.push_str(&format!(" L{} hw{} ", t.layer, t.half_width));
                for i in 0..t.corner_count() {
                    let c = t.polyline.corner_approx(i);
                    s.push_str(&format!("{},{} ", java_round(c.x), java_round(c.y)));
                }
            }
            ItemKind::Pin(_) | ItemKind::Via(_) => {
                let c = item.as_drill().unwrap().center.to_float();
                s.push_str(&format!(" at {},{}", java_round(c.x), java_round(c.y)));
            }
            _ => {}
        }
        s
    }

    fn java_type_name(item: &Item) -> &'static str {
        match &item.kind {
            ItemKind::Trace(_) => "PolylineTrace",
            ItemKind::Via(_) => "Via",
            ItemKind::Pin(_) => "Pin",
            ItemKind::BoardOutline(_) => "BoardOutline",
            ItemKind::ComponentOutline(_) => "ComponentOutline",
            ItemKind::Obstacle(_) => {
                if item.is_conduction_area() {
                    "ConductionArea"
                } else if item.is_via_obstacle_area() {
                    "ViaObstacleArea"
                } else if item.is_component_obstacle_area() {
                    "ComponentObstacleArea"
                } else {
                    "ObstacleArea"
                }
            }
        }
    }

    pub fn id_log_enabled() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| enabled() && std::env::var_os("RR_IDLOG").is_some())
    }

    pub fn maze_log_enabled() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| enabled() && std::env::var_os("RR_MAZELOG").is_some())
    }

    pub fn line(s: &str) {
        if let Some(m) = log() {
            let _ = m.lock().unwrap().write_all(format!("{s}\n").as_bytes());
        }
    }

    pub fn event(kind: &str, item: &Item) {
        if let Some(m) = log() {
            let line = format!("{} {}\n", kind, describe(item));
            let _ = m.lock().unwrap().write_all(line.as_bytes());
        }
    }
}
