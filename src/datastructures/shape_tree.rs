//! Binary search tree of shapes with minimal area increase on insertion
//! (port of `ShapeTree` / `MinAreaTree`). Nodes live in an arena; leaves are addressed by
//! stable indices.

use crate::geometry::RegularTileShape;

pub type NodeIdx = u32;
pub const NONE: NodeIdx = u32::MAX;

#[derive(Clone, Debug)]
enum NodeKind {
    Inner { first: NodeIdx, second: NodeIdx },
    Leaf,
    Free,
}

/// Leaf data is kept in a separate array (indexed by node) so the nodes stay small for the
/// tree traversals.
#[derive(Clone, Debug)]
struct Node {
    bounds: RegularTileShape,
    parent: NodeIdx,
    kind: NodeKind,
}

#[derive(Clone, Debug)]
pub struct MinAreaTree<L> {
    nodes: Vec<Node>,
    data: Vec<Option<L>>,
    free: Vec<NodeIdx>,
    root: NodeIdx,
    leaf_count: usize,
    /// Debug tag identifying the tree in the tree operation log.
    pub tag: i32,
}

/// Debug: log tree insertions/removals (compared against the instrumented Java original).
pub static TREEOPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn rr_b(r: &RegularTileShape) -> String {
    let o = r.to_octagon();
    format!("{{{},{},{},{},{},{},{},{}}}", o.lx, o.ly, o.rx, o.uy, o.ulx, o.lrx, o.llx, o.urx)
}

impl<L> Default for MinAreaTree<L> {
    fn default() -> Self {
        MinAreaTree {
            nodes: Vec::new(),
            data: Vec::new(),
            free: Vec::new(),
            root: NONE,
            leaf_count: 0,
            tag: -1,
        }
    }
}

impl<L> MinAreaTree<L> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.leaf_count
    }

    #[inline]
    pub fn root(&self) -> NodeIdx {
        self.root
    }

    fn alloc(&mut self, node: Node, data: Option<L>) -> NodeIdx {
        if let Some(i) = self.free.pop() {
            self.nodes[i as usize] = node;
            self.data[i as usize] = data;
            i
        } else {
            self.nodes.push(node);
            self.data.push(data);
            (self.nodes.len() - 1) as NodeIdx
        }
    }

    #[inline]
    pub fn leaf(&self, idx: NodeIdx) -> &L {
        self.data[idx as usize].as_ref().expect("MinAreaTree: leaf expected")
    }

    #[inline]
    pub fn leaf_mut(&mut self, idx: NodeIdx) -> &mut L {
        self.data[idx as usize].as_mut().expect("MinAreaTree: leaf expected")
    }

    #[inline]
    pub fn is_leaf(&self, idx: NodeIdx) -> bool {
        matches!(self.nodes[idx as usize].kind, NodeKind::Leaf)
    }

    #[inline]
    pub fn bounds(&self, idx: NodeIdx) -> &RegularTileShape {
        &self.nodes[idx as usize].bounds
    }

    /// The children of an inner node, None for leaves.
    #[inline]
    pub fn children(&self, idx: NodeIdx) -> Option<(NodeIdx, NodeIdx)> {
        match self.nodes[idx as usize].kind {
            NodeKind::Inner { first, second } => Some((first, second)),
            _ => None,
        }
    }

    /// Inserts a new leaf and returns its index.
    pub fn insert(&mut self, bounds: RegularTileShape, data: L) -> NodeIdx {
        let leaf = self.alloc(
            Node {
                bounds,
                parent: NONE,
                kind: NodeKind::Leaf,
            },
            Some(data),
        );
        self.leaf_count += 1;
        if self.root == NONE {
            self.root = leaf;
            return leaf;
        }
        let to_replace = self.position_locate(leaf);
        if TREEOPS.load(std::sync::atomic::Ordering::Relaxed) {
            crate::board::snapshot::evlog::line(&format!(
                "TI t{} {} -> {}",
                self.tag,
                rr_b(&self.nodes[leaf as usize].bounds),
                rr_b(&self.nodes[to_replace as usize].bounds)
            ));
        }
        let new_bounds = self.nodes[leaf as usize]
            .bounds
            .union(&self.nodes[to_replace as usize].bounds);
        let current_parent = self.nodes[to_replace as usize].parent;
        let new_node = self.alloc(
            Node {
                bounds: new_bounds,
                parent: current_parent,
                kind: NodeKind::Inner {
                    first: to_replace,
                    second: leaf,
                },
            },
            None,
        );
        if current_parent != NONE {
            if let NodeKind::Inner { first, second } = &mut self.nodes[current_parent as usize].kind {
                if *first == to_replace {
                    *first = new_node;
                } else {
                    *second = new_node;
                }
            }
        }
        self.nodes[to_replace as usize].parent = new_node;
        self.nodes[leaf as usize].parent = new_node;
        if self.root == to_replace {
            self.root = new_node;
        }
        leaf
    }

    fn position_locate(&mut self, leaf: NodeIdx) -> NodeIdx {
        let leaf_bounds = self.nodes[leaf as usize].bounds;
        let mut node = self.root;
        loop {
            let (first, second) = match self.nodes[node as usize].kind {
                NodeKind::Inner { first, second } => (first, second),
                _ => return node,
            };
            let nb = leaf_bounds.union(&self.nodes[node as usize].bounds);
            self.nodes[node as usize].bounds = nb;
            let fs = self.nodes[first as usize].bounds;
            let first_increase = leaf_bounds.union(&fs).area() - fs.area();
            let ss = self.nodes[second as usize].bounds;
            let second_increase = leaf_bounds.union(&ss).area() - ss.area();
            node = if first_increase <= second_increase { first } else { second };
        }
    }

    /// Removes a leaf. Returns its data.
    pub fn remove_leaf(&mut self, leaf: NodeIdx) -> Option<L> {
        if leaf == NONE || leaf as usize >= self.nodes.len() || !self.is_leaf(leaf) {
            return None;
        }
        let parent = self.nodes[leaf as usize].parent;
        if parent == NONE && self.root != leaf {
            return None;
        }
        self.leaf_count -= 1;
        if TREEOPS.load(std::sync::atomic::Ordering::Relaxed) {
            crate::board::snapshot::evlog::line(&format!("TR t{} {}", self.tag, rr_b(&self.nodes[leaf as usize].bounds)));
        }
        self.nodes[leaf as usize].kind = NodeKind::Free;
        let data = self.data[leaf as usize].take().expect("leaf data");
        self.nodes[leaf as usize].parent = NONE;
        self.free.push(leaf);
        if parent == NONE {
            self.root = NONE;
            return Some(data);
        }
        let other = match self.nodes[parent as usize].kind {
            NodeKind::Inner { first, second } => {
                if second == leaf {
                    first
                } else if first == leaf {
                    second
                } else {
                    log::warn!("MinAreaTree.remove_leaf: parent inconsistent");
                    return Some(data);
                }
            }
            _ => unreachable!(),
        };
        let grand_parent = self.nodes[parent as usize].parent;
        self.nodes[other as usize].parent = grand_parent;
        if grand_parent == NONE {
            self.root = other;
        } else if let NodeKind::Inner { first, second } = &mut self.nodes[grand_parent as usize].kind {
            if *second == parent {
                *second = other;
            } else if *first == parent {
                *first = other;
            } else {
                log::warn!("MinAreaTree.remove_leaf: grandParent inconsistent");
            }
        }
        self.nodes[parent as usize].kind = NodeKind::Free;
        self.nodes[parent as usize].parent = NONE;
        self.free.push(parent);
        let mut n = grand_parent;
        while n != NONE {
            let (first, second) = match self.nodes[n as usize].kind {
                NodeKind::Inner { first, second } => (first, second),
                _ => break,
            };
            let nb = self.nodes[second as usize]
                .bounds
                .union(&self.nodes[first as usize].bounds);
            if nb.contains(&self.nodes[n as usize].bounds) {
                break;
            }
            self.nodes[n as usize].bounds = nb;
            n = self.nodes[n as usize].parent;
        }
        Some(data)
    }

    /// Collects all leaves whose bounding shape intersects `shape` (unsorted, in stack order).
    pub fn overlaps_unsorted(&self, shape: &RegularTileShape, result: &mut Vec<NodeIdx>) {
        if self.root == NONE {
            return;
        }
        let mut stack: Vec<NodeIdx> = Vec::with_capacity(64);
        stack.push(self.root);
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n as usize];
            if node.bounds.intersects(shape) {
                match node.kind {
                    NodeKind::Leaf => result.push(n),
                    NodeKind::Inner { first, second } => {
                        stack.push(first);
                        stack.push(second);
                    }
                    NodeKind::Free => {}
                }
            }
        }
    }

    /// All leaves in tree order.
    pub fn to_vec(&self) -> Vec<NodeIdx> {
        let mut result = Vec::with_capacity(self.leaf_count);
        if self.root == NONE {
            return result;
        }
        let mut stack = vec![self.root];
        while let Some(n) = stack.pop() {
            match self.nodes[n as usize].kind {
                NodeKind::Leaf => result.push(n),
                NodeKind::Inner { first, second } => {
                    stack.push(second);
                    stack.push(first);
                }
                NodeKind::Free => {}
            }
        }
        result
    }
}
