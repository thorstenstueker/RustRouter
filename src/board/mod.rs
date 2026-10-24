//! The routing board (port of `BasicBoard` and `RoutingBoard`).

pub mod changed_area;
pub mod checks;
pub mod connectivity;
pub mod item;
pub mod items_factory;
pub mod search_tree;
pub mod structure;
pub mod pin_ops;
pub mod routing_ops;
pub mod snapshot;
pub mod trace_ops;
pub mod trace_pin;

pub use changed_area::ChangedArea;
pub use item::{Item, ItemId, ItemKind, NetNumbers, ObstacleKind};
pub use search_tree::{ItemLookup, RoomId, ShapeSearchTree, TreeEntry, TreeEnv, TreeKind, TreeObject};
pub use structure::{
    Communication, Component, Components, CoordinateTransform, FixedState, IdGenerator, Layer,
    LayerStructure, SpecctraParserInfo, Unit,
};

use crate::geometry::{IntBox, Point, TileShape};
use crate::library::BoardLibrary;
use crate::rules::{AngleRestriction, BoardRules, Nets};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Storage of the board items, indexed by id.
#[derive(Clone, Debug, Default)]
pub struct ItemStore {
    items: Vec<Option<Box<Item>>>,
    count: usize,
    /// Connectable items by net number.
    net_index: FxHashMap<i32, BTreeSet<ItemId>>,
    outline: Option<ItemId>,
    pin_count: usize,
    edge_pin_nets: OnceLock<FxHashSet<i32>>,
    #[allow(dead_code)]
    layer_count: i32,
    /// Revision counter, incremented on each insertion or removal.
    pub version: u64,
    /// Counter incremented on each change of the content.
    content_version: u64,
}

impl ItemStore {
    #[inline]
    pub fn get(&self, id: ItemId) -> Option<&Item> {
        self.items.get(id.0 as usize).and_then(|o| o.as_deref())
    }

    /// Mutable access to an item. Every mutable access counts as a content change
    /// (like `saveForUndo` in the original) and invalidates caches depending on the content.
    #[inline]
    pub fn get_mut(&mut self, id: ItemId) -> Option<&mut Item> {
        self.content_version += 1;
        self.items.get_mut(id.0 as usize).and_then(|o| o.as_deref_mut())
    }

    #[inline]
    pub fn content_version(&self) -> u64 {
        self.content_version
    }

    pub fn contains(&self, id: ItemId) -> bool {
        self.get(id).is_some()
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// All item ids in descending order (the iteration order of the original).
    pub fn ids(&self) -> Vec<ItemId> {
        let mut r = Vec::with_capacity(self.count);
        for i in (0..self.items.len()).rev() {
            if self.items[i].is_some() {
                r.push(ItemId(i as i32));
            }
        }
        r
    }

    /// Iterates the items in descending id order.
    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items.iter().rev().filter_map(|o| o.as_deref())
    }

    pub fn outline_id(&self) -> Option<ItemId> {
        self.outline
    }

    pub fn pin_count(&self) -> usize {
        self.pin_count
    }

    fn insert(&mut self, item: Item) {
        let idx = item.id.0 as usize;
        if self.items.len() <= idx {
            self.items.resize_with(idx + 1, || None);
        }
        if item.is_connectable_type() {
            for n in &item.net_numbers {
                self.net_index.entry(*n).or_default().insert(item.id);
            }
        }
        if item.is_pin() {
            self.pin_count += 1;
            self.invalidate_edge_pin_nets();
        }
        if item.is_board_outline() {
            self.outline = Some(item.id);
            self.invalidate_edge_pin_nets();
        }
        if self.items[idx].is_none() {
            self.count += 1;
        }
        self.items[idx] = Some(Box::new(item));
        self.version += 1;
        self.content_version += 1;
    }

    pub(crate) fn remove(&mut self, id: ItemId) -> Option<Item> {
        let idx = id.0 as usize;
        let item = self.items.get_mut(idx)?.take()?;
        self.count -= 1;
        if item.is_connectable_type() {
            for n in &item.net_numbers {
                if let Some(s) = self.net_index.get_mut(n) {
                    s.remove(&id);
                }
            }
        }
        if item.is_pin() {
            self.pin_count -= 1;
            self.invalidate_edge_pin_nets();
        }
        if self.outline == Some(id) {
            self.outline = None;
            self.invalidate_edge_pin_nets();
        }
        self.version += 1;
        self.content_version += 1;
        Some(*item)
    }

    /// Must be called before the net numbers of an item are changed.
    fn unindex_nets(&mut self, id: ItemId) {
        if let Some(item) = self.get(id) {
            if item.is_connectable_type() {
                let nets: Vec<i32> = item.net_numbers.to_vec();
                for n in nets {
                    if let Some(s) = self.net_index.get_mut(&n) {
                        s.remove(&id);
                    }
                }
            }
        }
    }

    /// Must be called after the net numbers of an item are changed.
    fn index_nets(&mut self, id: ItemId) {
        let (nets, is_connectable, is_pin) = match self.get(id) {
            Some(item) => (item.net_numbers.to_vec(), item.is_connectable_type(), item.is_pin()),
            None => return,
        };
        if is_connectable {
            for n in nets {
                self.net_index.entry(n).or_default().insert(id);
            }
        }
        if is_pin {
            self.invalidate_edge_pin_nets();
        }
        self.version += 1;
    }

    pub fn invalidate_edge_pin_nets(&mut self) {
        self.edge_pin_nets = OnceLock::new();
    }

    /// Connectable items of a net in descending id order.
    pub fn connectable_items(&self, net_no: i32) -> Vec<ItemId> {
        match self.net_index.get(&net_no) {
            Some(s) => s.iter().copied().collect(),
            None => Vec::new(),
        }
    }

    pub fn connectable_item_count(&self, net_no: i32) -> usize {
        self.net_index.get(&net_no).map(|s| s.len()).unwrap_or(0)
    }

    /// Nets of pins at the edge of or outside the board outline.
    fn edge_pin_nets(&self) -> &FxHashSet<i32> {
        self.edge_pin_nets.get_or_init(|| {
            let mut set = FxHashSet::default();
            let Some(outline) = self.outline.and_then(|o| self.get(o)).and_then(|o| o.as_board_outline()) else {
                return set;
            };
            for item in self.iter() {
                let Some(pin) = item.as_pin() else { continue };
                let center = &pin.drill.center;
                let mut edge = false;
                if !outline.contains(center) {
                    edge = true;
                } else {
                    for layer in pin.drill.first_layer..=pin.drill.last_layer {
                        if let Some(crate::geometry::Shape::Tile(t)) =
                            pin.drill.get_shape((layer - pin.drill.first_layer) as usize)
                        {
                            for c in 0..t.border_line_count() {
                                if !outline.contains(&t.corner(c)) {
                                    edge = true;
                                    break;
                                }
                            }
                        }
                        if edge {
                            break;
                        }
                    }
                }
                if edge {
                    for n in &item.net_numbers {
                        set.insert(*n);
                    }
                }
            }
            set
        })
    }

    /// `BoardOutline.isTraceObstacle(netNumber)`.
    pub fn outline_is_trace_obstacle(&self, outline: &Item, net_no: i32) -> bool {
        let o = outline.as_board_outline().unwrap();
        if o.edge_pin_nets_cross_outline && net_no > 0 && self.edge_pin_nets().contains(&net_no) {
            return false;
        }
        true
    }

    /// `BoardOutline.blocksNets(netNumbers)`.
    pub fn outline_blocks_nets(&self, outline: &Item, nets: &[i32]) -> bool {
        if nets.is_empty() {
            return true;
        }
        nets.iter().any(|n| self.outline_is_trace_obstacle(outline, *n))
    }

    fn outline_blocks(&self, nets: &[i32]) -> bool {
        match self.outline.and_then(|o| self.get(o)) {
            Some(o) => self.outline_blocks_nets(o, nets),
            None => true,
        }
    }

    /// The name of the package pin of a pin item.
    fn same_logical_pad(&self, a: &Item, b: &Item, components: Option<&Components>, library: Option<&BoardLibrary>) -> bool {
        let (Some(components), Some(library)) = (components, library) else {
            return false;
        };
        let (Some(pa), Some(pb)) = (a.as_pin(), b.as_pin()) else {
            return false;
        };
        let Some(comp) = components.try_get(a.component_id) else {
            return false;
        };
        let pkg = library.packages.get(comp.get_package());
        let (Some(n1), Some(n2)) = (pkg.get_pin(pa.pin_index as usize), pkg.get_pin(pb.pin_index as usize)) else {
            return false;
        };
        let b1 = item::Pin::get_base_pin_name(&n1.name);
        let b2 = item::Pin::get_base_pin_name(&n2.name);
        !b1.is_empty() && b1 == b2
    }

    /// `a.isObstacle(b)` with full board context.
    pub fn is_obstacle_ctx(&self, a: &Item, b: &Item, components: Option<&Components>, library: Option<&BoardLibrary>) -> bool {
        if let (ItemKind::Pin(_), ItemKind::Pin(_)) = (&a.kind, &b.kind) {
            if a.id != b.id
                && !b.shares_net(a)
                && a.component_id > 0
                && a.component_id == b.component_id
                && a.net_count() == 0
                && b.net_count() == 0
                && self.same_logical_pad(a, b, components, library)
            {
                return false;
            }
        }
        a.is_obstacle_for(b, |nets| self.outline_blocks(nets))
    }
}

/// `ItemLookup` without component context (logical pad detection disabled).
impl ItemLookup for ItemStore {
    #[inline]
    fn item(&self, id: ItemId) -> &Item {
        self.get(id).expect("item in search tree must exist")
    }

    #[inline]
    fn is_obstacle_net(&self, id: ItemId, net_no: i32) -> bool {
        let item = self.item(id);
        item.is_obstacle_net_basic(net_no)
    }

    #[inline]
    fn is_trace_obstacle(&self, id: ItemId, net_no: i32) -> bool {
        let item = self.item(id);
        if item.is_board_outline() {
            return self.outline_is_trace_obstacle(item, net_no);
        }
        item.is_trace_obstacle_basic(net_no)
    }

    #[inline]
    fn item_is_obstacle(&self, a: &Item, b: &Item) -> bool {
        self.is_obstacle_ctx(a, b, None, None)
    }
}

/// Item lookup with component context.
pub struct BoardLookup<'a> {
    pub items: &'a ItemStore,
    pub components: &'a Components,
    pub library: &'a BoardLibrary,
}

impl ItemLookup for BoardLookup<'_> {
    #[inline]
    fn item(&self, id: ItemId) -> &Item {
        self.items.item(id)
    }

    #[inline]
    fn is_obstacle_net(&self, id: ItemId, net_no: i32) -> bool {
        self.items.is_obstacle_net(id, net_no)
    }

    #[inline]
    fn is_trace_obstacle(&self, id: ItemId, net_no: i32) -> bool {
        self.items.is_trace_obstacle(id, net_no)
    }

    #[inline]
    fn item_is_obstacle(&self, a: &Item, b: &Item) -> bool {
        self.items.is_obstacle_ctx(a, b, Some(self.components), Some(self.library))
    }
}

/// The search trees of a board: the default tree, the uncompensated tree for
/// design rule checks and the autoroute trees with clearance compensation.
#[derive(Clone, Debug)]
pub struct SearchTreeManager {
    trees: Vec<ShapeSearchTree>,
    default_tree: usize,
    clearance_compensation_used: bool,
}

impl Default for SearchTreeManager {
    fn default() -> Self {
        SearchTreeManager {
            trees: vec![ShapeSearchTree::new(TreeKind::Default, 0)],
            default_tree: 0,
            clearance_compensation_used: false,
        }
    }
}

/// Index of a search tree in the manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TreeId(pub usize);

impl SearchTreeManager {
    #[inline]
    pub fn default_tree_id(&self) -> TreeId {
        TreeId(self.default_tree)
    }

    #[inline]
    pub fn get(&self, id: TreeId) -> &ShapeSearchTree {
        &self.trees[id.0]
    }

    #[inline]
    pub fn get_mut(&mut self, id: TreeId) -> &mut ShapeSearchTree {
        &mut self.trees[id.0]
    }

    #[inline]
    pub fn default_tree(&self) -> &ShapeSearchTree {
        &self.trees[self.default_tree]
    }

    pub fn is_clearance_compensation_used(&self) -> bool {
        self.clearance_compensation_used
    }

    pub fn trees(&self) -> &[ShapeSearchTree] {
        &self.trees
    }

    pub fn trees_mut(&mut self) -> &mut [ShapeSearchTree] {
        &mut self.trees
    }

    pub fn find(&self, kind_default_only: bool, clearance_class: i32) -> Option<TreeId> {
        self.trees
            .iter()
            .position(|t| {
                t.compensated_clearance_class == clearance_class && (!kind_default_only || t.kind == TreeKind::Default)
            })
            .map(TreeId)
    }
}

/// The routing board.
#[derive(Clone, Debug)]
pub struct Board {
    pub items: ItemStore,
    pub search_trees: SearchTreeManager,
    pub components: Components,
    pub rules: BoardRules,
    pub library: BoardLibrary,
    pub layer_structure: LayerStructure,
    pub communication: Communication,
    pub bounding_box: IntBox,
    pub changed_area: Option<ChangedArea>,
    max_trace_half_width: i32,
    min_trace_half_width: i32,
    pub pre_existing_clearance_violations_count: usize,
    pub normalize_suppressed_nets: FxHashSet<i32>,
    /// State of the autoroute database maintained while routing.
    pub autoroute_db: Option<Box<crate::autoroute::AutorouteEngine>>,
    pub shove_failing_obstacle: Option<ItemId>,
    pub shove_failing_layer: i32,
    pub shove_failing_obstacle_writes: u64,
    pub shove_failing_layer_writes: u64,
    pub shove_cache: crate::optimize::shove::ShoveCheckCache,
    /// `RoutingBoard.failureLog`
    pub failure_log: snapshot::RoutingFailureLog,
    /// `BasicBoard.clearanceBaseline` (transient: not copied by snapshots)
    pub clearance_baseline: Option<std::sync::Arc<crate::autoroute::pipeline::stats::Baseline>>,
    /// Items removed since the start of the current connection: the original keeps working
    /// with removed item objects held in local sets, so their data must stay available.
    pub removed_items: FxHashMap<ItemId, Item>,
}

impl Board {
    pub fn new(
        bounding_box: IntBox,
        layer_structure: LayerStructure,
        outline_shapes: Vec<crate::geometry::Shape>,
        outline_cl_class: i32,
        rules: BoardRules,
        communication: Communication,
    ) -> Board {
        let padstacks = crate::library::Padstacks::new(layer_structure.count());
        let mut board = Board {
            items: ItemStore {
                layer_count: layer_structure.count() as i32,
                ..Default::default()
            },
            search_trees: SearchTreeManager::default(),
            components: Components::default(),
            rules,
            library: BoardLibrary::new(padstacks, Default::default()),
            layer_structure,
            communication,
            bounding_box,
            changed_area: None,
            max_trace_half_width: 1000,
            min_trace_half_width: 10000,
            pre_existing_clearance_violations_count: 0,
            normalize_suppressed_nets: FxHashSet::default(),
            autoroute_db: None,
            shove_failing_obstacle: None,
            shove_failing_layer: -1,
            shove_failing_obstacle_writes: 0,
            shove_failing_layer_writes: 0,
            shove_cache: Default::default(),
            failure_log: Default::default(),
            clearance_baseline: None,
            removed_items: FxHashMap::default(),
        };
        board.insert_outline(outline_shapes, outline_cl_class);
        board
    }

    #[inline]
    pub fn layer_count(&self) -> i32 {
        self.layer_structure.count() as i32
    }

    #[inline]
    pub fn item(&self, id: ItemId) -> &Item {
        self.items.get(id).expect("item exists")
    }

    #[inline]
    pub fn try_item(&self, id: ItemId) -> Option<&Item> {
        self.items.get(id)
    }

    /// An item on the board or removed since the start of the current connection.
    #[inline]
    pub fn any_item(&self, id: ItemId) -> Option<&Item> {
        self.items.get(id).or_else(|| self.removed_items.get(&id))
    }

    /// Forgets the removed items (at the start of a new connection).
    pub fn clear_removed_items(&mut self) {
        self.removed_items.clear();
    }

    #[inline]
    pub fn item_mut(&mut self, id: ItemId) -> &mut Item {
        self.items.get_mut(id).expect("item exists")
    }

    #[inline]
    pub fn lookup(&self) -> BoardLookup<'_> {
        BoardLookup {
            items: &self.items,
            components: &self.components,
            library: &self.library,
        }
    }

    pub fn tree_env(&self) -> TreeEnv<'_> {
        TreeEnv {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            layer_count: self.layer_count(),
            bounding_box: self.bounding_box,
            host_cad_exists: self.communication.host_cad_exists(),
            mil_resolution: self.communication.get_resolution(Unit::Mil),
        }
    }

    pub fn get_max_trace_half_width(&self) -> i32 {
        self.max_trace_half_width
    }

    pub fn get_min_trace_half_width(&self) -> i32 {
        self.min_trace_half_width
    }

    pub fn note_trace_half_width(&mut self, hw: i32) {
        self.max_trace_half_width = self.max_trace_half_width.max(hw);
        self.min_trace_half_width = self.min_trace_half_width.min(hw);
    }

    pub fn new_item_id(&mut self) -> ItemId {
        ItemId(self.communication.id_generator.new_id())
    }

    // ---------------------------------------------------------------- shapes of items

    /// The tile shape of an item in the default tree.
    pub fn tile_shape(&self, id: ItemId, index: usize) -> Option<TileShape> {
        self.tree_shape(self.search_trees.default_tree_id(), id, index)
    }

    /// The shape of an item in a search tree. For items not in the tree the shape is calculated.
    pub fn tree_shape(&self, tree: TreeId, id: ItemId, index: usize) -> Option<TileShape> {
        let t = self.search_trees.get(tree);
        let obj = TreeObject::Item(id);
        if t.contains_object(&obj) {
            return t.object_shape(&obj, index).cloned();
        }
        let item = self.any_item(id)?;
        let (shapes, _) = t.calculate_tree_shapes(item, &self.tree_env());
        shapes.into_iter().nth(index).flatten()
    }

    pub fn tree_shape_count(&self, tree: TreeId, id: ItemId) -> usize {
        let t = self.search_trees.get(tree);
        let obj = TreeObject::Item(id);
        if t.contains_object(&obj) {
            return t.object_shape_count(&obj);
        }
        match self.items.get(id) {
            Some(item) => self.calc_shapes_detached(tree, item).0.len(),
            None => 0,
        }
    }

    /// Shapes of an item not stored in the board.
    pub fn calc_shapes_detached(&self, tree: TreeId, item: &Item) -> (Vec<Option<TileShape>>, Vec<i32>) {
        self.search_trees.get(tree).calculate_tree_shapes(item, &self.tree_env())
    }

    pub fn tile_shape_count(&self, id: ItemId) -> usize {
        self.tree_shape_count(self.search_trees.default_tree_id(), id)
    }

    /// `shapeLayer(index)` of an item.
    pub fn shape_layer(&self, id: ItemId, index: usize) -> i32 {
        let item = self.item(id);
        match &item.kind {
            ItemKind::BoardOutline(_) => {
                let n = self.tile_shape_count(id);
                item.shape_layer_with(index, self.layer_count(), n)
            }
            _ => item.shape_layer_with(index, self.layer_count(), 0),
        }
    }

    // ---------------------------------------------------------------- insertion and removal

    pub(crate) fn insert_into_trees(&mut self, id: ItemId) {
        let Board {
            items,
            search_trees,
            rules,
            library,
            layer_structure,
            bounding_box,
            communication,
            ..
        } = self;
        let env = TreeEnv {
            rules,
            padstacks: &library.padstacks,
            layer_count: layer_structure.count() as i32,
            bounding_box: *bounding_box,
            host_cad_exists: communication.host_cad_exists(),
            mil_resolution: communication.get_resolution(Unit::Mil),
        };
        let item = items.get(id).unwrap();
        for tree in search_trees.trees.iter_mut() {
            let (shapes, layers) = tree.calculate_tree_shapes(item, &env);
            tree.insert_shapes(TreeObject::Item(id), shapes, &layers);
        }
        items.get_mut(id).unwrap().on_the_board = true;
    }

    pub(crate) fn remove_from_trees(&mut self, id: ItemId) {
        let obj = TreeObject::Item(id);
        for tree in self.search_trees.trees.iter_mut() {
            tree.remove_object(&obj);
        }
        if let Some(item) = self.items.get_mut(id) {
            item.on_the_board = false;
        }
    }

    /// Inserts an item into the item store only (the tree entries are handled by the caller).
    pub(crate) fn items_insert_raw(&mut self, item: Item) {
        self.items.insert(item);
    }

    /// Reinserts an item into the search trees after its geometry changed.
    pub fn reinsert_into_trees(&mut self, id: ItemId) {
        self.remove_from_trees(id);
        self.insert_into_trees(id);
    }

    /// Inserts an item into the board (`insertItem` of the original).
    pub fn insert_item(&mut self, mut item: Item) -> ItemId {
        if item.clearance_class < 0 || item.clearance_class as usize >= self.rules.clearance_matrix.get_class_count() {
            log::warn!("LayeredBoard.insert_item: clearanceClass no out of range");
            item.clearance_class = 0;
        }
        let id = item.id;
        if snapshot::evlog::enabled() {
            snapshot::evlog::event("INS", &item);
        }
        self.items.insert(item);
        self.insert_into_trees(id);
        self.additional_update_after_change(id);
        id
    }

    /// Removes an item from the board (`removeItem` of the original).
    /// Returns false, if the deletion is forbidden.
    pub fn remove_item(&mut self, id: ItemId) -> bool {
        self.remove_item_impl(id, false)
    }

    pub fn detach_item_for_move(&mut self, id: ItemId) -> bool {
        self.remove_item_impl(id, true)
    }

    fn remove_item_impl(&mut self, id: ItemId, for_move: bool) -> bool {
        let Some(item) = self.items.get(id) else {
            return false;
        };
        if self.is_deletion_forbidden(item) && !(for_move && item.component_id > 0) {
            return false;
        }
        self.additional_update_after_change(id);
        if snapshot::evlog::enabled() {
            snapshot::evlog::event("DEL", self.items.get(id).unwrap());
        }
        self.remove_from_trees(id);
        if let Some(it) = self.items.remove(id) {
            self.removed_items.insert(id, it);
        }
        true
    }

    pub fn remove_items(&mut self, ids: &[ItemId]) -> bool {
        let mut result = true;
        for id in ids {
            let Some(item) = self.items.get(*id) else { continue };
            if self.is_deletion_forbidden(item) || item.is_user_fixed() {
                result = false;
            } else {
                self.remove_item(*id);
            }
        }
        result
    }

    pub fn is_deletion_forbidden(&self, item: &Item) -> bool {
        if item.component_id > 0 || item.is_user_fixed() {
            return true;
        }
        if let ItemKind::Obstacle(o) = &item.kind {
            if matches!(o.kind, ObstacleKind::Conduction { .. }) {
                return !self.layer_structure.layers[o.layer as usize].is_signal;
            }
        }
        false
    }

    /// Changes the net numbers of an item keeping the net index consistent.
    pub fn set_item_nets(&mut self, id: ItemId, nets: NetNumbers) {
        self.items.unindex_nets(id);
        self.item_mut(id).net_numbers = nets;
        self.items.index_nets(id);
    }

    pub fn assign_net_no(&mut self, id: ItemId, net_no: i32) {
        if !Nets::is_normal_net_number(net_no) {
            return;
        }
        if net_no > self.rules.nets.max_net_number() {
            log::warn!("Item.assign_net_no: netNumber to big");
            return;
        }
        let mut nets = self.item(id).net_numbers.clone();
        if nets.is_empty() {
            nets.push(net_no);
        } else {
            if nets.len() > 1 {
                log::warn!("Item.assign_net_no: unexpected netCount > 1");
            }
            nets[0] = net_no;
        }
        self.set_item_nets(id, nets);
    }

    pub fn remove_item_from_net(&mut self, id: ItemId, net_no: i32) -> bool {
        let mut nets = self.item(id).net_numbers.clone();
        let mut found = None;
        for (i, n) in nets.iter().enumerate() {
            if *n == net_no {
                found = Some(i);
            }
        }
        let Some(i) = found else { return false };
        nets.remove(i);
        self.set_item_nets(id, nets);
        true
    }

    /// Changes the fixed state of an item.
    pub fn set_fixed_state(&mut self, id: ItemId, state: FixedState) {
        self.item_mut(id).fixed_state = state;
    }

    /// Hook called when an item changes (`additionalUpdateAfterChange` of `RoutingBoard`):
    /// invalidates the overlapping parts of the autoroute database.
    pub fn additional_update_after_change(&mut self, id: ItemId) {
        if self.autoroute_db.as_ref().map(|d| d.maintain_database).unwrap_or(false) {
            crate::autoroute::invalidate_for_item(self, id);
        }
    }

    // ---------------------------------------------------------------- queries

    /// Objects overlapping a shape in the default tree.
    pub fn overlapping_objects(&self, shape: &TileShape, layer: i32) -> Vec<TreeObject> {
        self.search_trees.default_tree().overlapping_objects(shape, layer, &self.lookup())
    }

    /// Items overlapping a shape in the default tree (descending id).
    pub fn overlapping_items(&self, shape: &TileShape, layer: i32) -> Vec<ItemId> {
        self.search_trees.default_tree().overlapping_items(shape, layer, &self.lookup())
    }

    pub fn overlapping_items_with_clearance(
        &self,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
    ) -> Vec<ItemId> {
        self.search_trees.default_tree().overlapping_items_with_clearance(
            shape,
            layer,
            ignore_nets,
            clearance_class,
            &self.rules,
            self.layer_count(),
            &self.bounding_box,
            &self.lookup(),
        )
    }

    /// Tree entries overlapping with clearance in a tree.
    #[allow(clippy::too_many_arguments)]
    pub fn overlapping_tree_entries_with_clearance(
        &self,
        tree: TreeId,
        shape: &TileShape,
        layer: i32,
        ignore_nets: &[i32],
        clearance_class: i32,
        obstacles_of: Option<&Item>,
        result: &mut Vec<TreeEntry>,
    ) {
        self.search_trees.get(tree).overlapping_tree_entries_with_clearance(
            shape,
            layer,
            ignore_nets,
            clearance_class,
            obstacles_of,
            &self.rules,
            self.layer_count(),
            &self.bounding_box,
            &self.lookup(),
            result,
        );
    }

    /// Items at `location` on `layer` (`pickItems` with optional filter applied by the caller).
    pub fn pick_items(&self, location: &Point, layer: i32) -> Vec<ItemId> {
        let shape = TileShape::from_point(location);
        self.overlapping_items(&shape, layer)
    }

    pub fn pick_traces(&self, location: &Point, layer: i32) -> Vec<ItemId> {
        self.pick_items(location, layer)
            .into_iter()
            .filter(|id| self.item(*id).is_trace())
            .collect()
    }

    pub fn contains_point(&self, p: &Point) -> bool {
        p.is_contained_in(&self.bounding_box)
    }

    pub fn clearance_value(&self, class1: i32, class2: i32, layer: i32) -> i32 {
        self.rules.clearance_matrix.get_value(class1, class2, layer, true)
    }

    /// `isShoveFixed` including the net class check for traces.
    pub fn is_shove_fixed(&self, item: &Item) -> bool {
        if item.is_shove_fixed_state() {
            return true;
        }
        if item.is_trace() {
            for n in &item.net_numbers {
                if Nets::is_normal_net_number(*n) {
                    if let Some(c) = self.rules.net_class_of(*n) {
                        if c.is_shove_fixed() {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// `isTraceObstacle(netNumber)` with board context.
    pub fn is_trace_obstacle(&self, id: ItemId, net_no: i32) -> bool {
        self.items.is_trace_obstacle(id, net_no)
    }

    /// `a.isObstacle(b)` with board context.
    pub fn is_obstacle(&self, a: &Item, b: &Item) -> bool {
        self.items.is_obstacle_ctx(a, b, Some(&self.components), Some(&self.library))
    }

    pub fn has_ignored_nets(&self, item: &Item) -> bool {
        item.net_numbers.iter().any(|n| {
            self.rules
                .net_class_of(*n)
                .map(|c| c.is_ignored_by_autorouter)
                .unwrap_or(false)
        })
    }

    pub fn get_outline(&self) -> Option<&Item> {
        self.items.outline_id().map(|id| self.item(id))
    }

    pub fn get_pins(&self) -> Vec<ItemId> {
        self.items.iter().filter(|i| i.is_pin()).map(|i| i.id).collect()
    }

    pub fn get_vias(&self) -> Vec<ItemId> {
        self.items.iter().filter(|i| i.is_via()).map(|i| i.id).collect()
    }

    pub fn get_traces(&self) -> Vec<ItemId> {
        self.items.iter().filter(|i| i.is_trace()).map(|i| i.id).collect()
    }

    pub fn get_via_count(&self) -> usize {
        self.items.iter().filter(|i| i.is_via()).count()
    }

    pub fn get_pin_count(&self) -> usize {
        self.items.pin_count()
    }

    pub fn cumulative_trace_length(&self) -> f64 {
        self.items
            .iter()
            .filter_map(|i| i.as_trace())
            .map(|t| t.get_length())
            .sum()
    }

    pub fn get_connectable_items(&self, net_no: i32) -> Vec<ItemId> {
        self.items.connectable_items(net_no)
    }

    pub fn connectable_item_count(&self, net_no: i32) -> usize {
        self.items.connectable_item_count(net_no)
    }

    pub fn get_component_items(&self, component_id: i32) -> Vec<ItemId> {
        self.items
            .iter()
            .filter(|i| i.component_id == component_id)
            .map(|i| i.id)
            .collect()
    }

    pub fn get_pin(&self, component_id: i32, pin_index: i32) -> Option<ItemId> {
        self.items
            .iter()
            .find(|i| i.component_id == component_id && i.as_pin().map(|p| p.pin_index == pin_index).unwrap_or(false))
            .map(|i| i.id)
    }

    pub fn get_non_45_degree_trace_count(&self) -> usize {
        self.items
            .iter()
            .filter_map(|i| i.as_trace())
            .filter(|t| !t.polyline.is_multiple_of_45_degree())
            .count()
    }

    pub fn trace_angle_restriction(&self) -> AngleRestriction {
        self.rules.get_trace_angle_restriction()
    }

    /// Reinserts all items into the search trees (`reinsertTreeItems`).
    pub fn reinsert_tree_items(&mut self) {
        let ids = self.items.ids();
        for id in &ids {
            self.remove_from_trees(*id);
        }
        for id in &ids {
            self.insert_into_trees(*id);
        }
    }

    /// Returns the autoroute tree for a clearance class, creating it if necessary.
    pub fn get_autoroute_tree(&mut self, clearance_class: i32) -> TreeId {
        if let Some(t) = self
            .search_trees
            .trees
            .iter()
            .position(|t| t.compensated_clearance_class == clearance_class)
        {
            return TreeId(t);
        }
        let kind = match self.rules.get_trace_angle_restriction() {
            AngleRestriction::NinetyDegree => TreeKind::Degree90,
            AngleRestriction::FortyfiveDegree => TreeKind::Degree45,
            AngleRestriction::None => TreeKind::Default,
        };
        self.add_tree(kind, clearance_class)
    }

    /// The uncompensated tree used for design rule checks.
    pub fn get_uncompensated_tree(&mut self) -> TreeId {
        if let Some(t) = self.search_trees.find(true, 0) {
            return t;
        }
        self.add_tree(TreeKind::Default, 0)
    }

    fn add_tree(&mut self, kind: TreeKind, clearance_class: i32) -> TreeId {
        let mut tree = ShapeSearchTree::new(kind, clearance_class);
        let env = self.tree_env();
        for item in self.items.iter() {
            let (shapes, layers) = tree.calculate_tree_shapes(item, &env);
            tree.insert_shapes(TreeObject::Item(item.id), shapes, &layers);
        }
        self.search_trees.trees.push(tree);
        TreeId(self.search_trees.trees.len() - 1)
    }

    pub fn set_clearance_compensation_used(&mut self, value: bool) {
        if self.search_trees.clearance_compensation_used == value {
            return;
        }
        self.search_trees.clearance_compensation_used = value;
        let cl = if value { 1 } else { 0 };
        self.search_trees.trees.clear();
        self.search_trees.default_tree = 0;
        self.search_trees.trees.push(ShapeSearchTree::new(TreeKind::Default, cl));
        let ids = self.items.ids();
        for id in ids {
            self.insert_into_trees(id);
        }
    }

    /// Removes the compensated trees except the default tree (`resetCompensatedTrees`).
    pub fn reset_compensated_trees(&mut self) {
        let d = self.search_trees.trees.swap_remove(self.search_trees.default_tree);
        self.search_trees.trees.clear();
        self.search_trees.trees.push(d);
        self.search_trees.default_tree = 0;
    }

    /// Removes the autoroute trees, which became invalid after a clearance value change.
    pub fn clearance_value_changed(&mut self) {
        let dcl = self.search_trees.default_tree().compensated_clearance_class;
        let d = self.search_trees.default_tree;
        let mut keep = Vec::new();
        for (i, t) in self.search_trees.trees.drain(..).enumerate() {
            if t.compensated_clearance_class == dcl || i == d {
                keep.push(t);
            }
        }
        self.search_trees.trees = keep;
        self.search_trees.default_tree = 0;
        if self.search_trees.clearance_compensation_used {
            self.reinsert_tree_items();
        }
    }

    pub fn expand_bounding_box_to_include_all_items(&mut self) {
        let mut bounds = self.bounding_box;
        let mut changed = false;
        for item in self.items.iter() {
            let b = item.bounding_box();
            if !b.is_empty() && !b.is_contained_in(&bounds) {
                bounds = bounds.union(&b);
                changed = true;
            }
        }
        if changed {
            self.bounding_box = bounds.offset(1000.0);
            self.items.invalidate_edge_pin_nets();
        }
    }

    pub fn shared_layer(&self, a: &Item, b: &Item) -> bool {
        a.shares_layer(b, self.layer_count())
    }
}
