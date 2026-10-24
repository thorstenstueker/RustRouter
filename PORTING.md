# RustRouter – porting notes

RustRouter is a faithful Rust port of the Freerouting autorouter (Java sources in
`../freerouting/src/main/java/app/freerouting`). Algorithms and their order of
operations are kept identical, so that routing results stay comparable with the
original. Debug/trace logging of the original (`FRLogger.trace`, net-49/94 debug
blocks, GUI printInfo) is dropped.

## Module mapping

| Java package                         | Rust module                   |
|--------------------------------------|-------------------------------|
| `geometry.planar`                    | `geometry`                    |
| `datastructures` (ShapeTree, …)      | `datastructures`              |
| `rules`                              | `rules`                       |
| `core.library`                       | `library`                     |
| `board.model.items`, `board.trace`   | `board::item`, `board::trace_ops` |
| `board.model.structure`, `board.state` | `board::structure`, `board::changed_area` |
| `board.searchtree`                   | `board::search_tree`          |
| `board.facade` (BasicBoard, RoutingBoard) | `board` (`Board`)        |
| `board.optimize`                     | `optimize`                    |
| `autoroute.*`                        | `autoroute`                   |
| `io.specctra`                        | `io::specctra`                |

## Conventions

* **Geometry types**: `Point` = enum {Int, Rational}; `Vector`, `Direction` likewise.
  `Line` always has two `IntPoint`s (the original casts to `IntPoint` everywhere).
  `TileShape` = enum {Box(IntBox), Octagon(IntOctagon), Simplex}; `Shape` = enum
  {Tile, Circle, Polygon}; `Area` = enum {Shape, PolylineArea}. `Simplex`,
  `Polyline`, `PolygonShape`, `PolylineArea` are `Arc` wrapped (cheap clone) with
  lazily cached corners.
* Java `int` arithmetic: `overflow-checks` are off in all profiles to keep wrap-around
  semantics. `Math.round` → `geometry::basics::java_round`, `Math.rint` → `rint`.
  `java.util.Random` → `basics::JavaRandom` (bit-identical).
* **Items** are stored in `board.items` (`ItemStore`, a `Vec<Option<Box<Item>>>` indexed
  by id). `ItemId` has a *descending* `Ord`, which reproduces the `TreeSet<Item>` and
  `UndoableObjects` iteration order of the original (`compareTo` = `other.id - this.id`).
  Use `BTreeSet<ItemId>` where the original uses `TreeSet<Item>`.
* An item is `Item { id, net_numbers, clearance_class, component_id, fixed_state,
  on_the_board, kind: ItemKind }`. Methods needing board context are `Board` methods
  taking an `ItemId`. Pin/via shapes, centers and layers are precalculated at insertion
  (`Board::calculate_pin_drill`). Obstacle areas store their absolute `area` and
  `convex_pieces`.
* **Search trees** (`ShapeSearchTree`) store `TreeLeaf { object, shape_index, layer, shape }`
  in an arena based `MinAreaTree` (same insertion/removal algorithm as Java, so traversal
  order is identical). `TreeObject` is `Item(ItemId)` or `Room(RoomId, java_id)` (autoroute
  expansion rooms). Query results are sorted like the Java leaves (`compareTo`).
* **No undo**: the autorouter only uses board snapshots (serialize/deserialize in Java).
  `Board::snapshot_copy()` / `deep_copy()` reproduce Java's deserialization: only the
  default search tree is rebuilt (items inserted in iteration order, so the tree shape
  differs from a long-lived board exactly as in Java) and transient fields are dropped.
  `Board::content_hash()` replaces `getHash` (only compared for equality).
* **Removed items still referenced**: Java code keeps using `Item` objects after they were
  removed from the board (stale start/destination sets, split traces). Removed items are
  kept in `board.removed_items` (cleared per routed connection); `Board::any_item` looks
  up both the board and that graveyard.
* **Item ids**: every place where Java creates an item object consumes an id, even when
  the object is discarded (`check_polyline_trace`, degenerate traces). Keep it that way,
  ids decide `TreeSet` order and with that the routing result.
* **Autoroute engine**: rooms, doors, targets and drills live in arenas inside
  `AutorouteEngine` (keys are never reused during an engine's lifetime). Locator float
  points carry an id to emulate Java object identity.
* **Optimizer threads**: each candidate is evaluated on a fresh `deep_copy` (like Java's
  worker boards), rayon processes chunks, results are taken in order, so the result is
  deterministic and independent of the thread count.
* Back references to the board (`item.board`) do not exist; pass `&Board`/`&mut Board`.
* Identity comparisons of Java objects (`==` on Lines, Polylines) are replaced by value
  comparisons or `Polyline::ptr_eq`; documented at the place of use.
