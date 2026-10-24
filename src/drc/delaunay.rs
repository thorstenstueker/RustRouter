//! Port of `datastructures.PlanarDelaunayTriangulation` (incremental randomized
//! Delaunay triangulation with a history DAG for point location).
//!
//! The Java object graph (corners, edges, triangles referencing each other) is
//! represented by index arenas. Identity comparisons of the original (`==` on
//! `Corner`, `Edge`, `Triangle`) become index comparisons. Edge ids are assigned
//! exactly as in the original (including the unused edges created in
//! `splitAtInnerPoint`), because the result edges are ordered by id.

use crate::geometry::basics::JavaRandom;
use crate::geometry::{IntPoint, Point, Side, CRIT_INT};
use std::collections::BTreeMap;

const SEED: i64 = 99;

/// An edge of the triangulation result. `start_object`/`end_object` are the indices
/// of the input objects (in the order passed to [`PlanarDelaunayTriangulation::new`]).
#[derive(Clone, Debug)]
pub struct ResultEdge {
    pub start_point: Point,
    pub start_object: usize,
    pub end_point: Point,
    pub end_object: usize,
}

#[derive(Clone, Debug)]
struct Corner {
    /// `None` for the corners of the bounding triangle.
    object: Option<usize>,
    coor: Point,
}

#[derive(Clone, Debug)]
struct Edge {
    start: usize,
    end: usize,
    id: i32,
    left: Option<usize>,
    right: Option<usize>,
}

#[derive(Clone, Debug)]
struct Triangle {
    edges: [usize; 3],
    first_parent: Option<usize>,
    children: Vec<usize>,
    is_on_left: Option<[bool; 3]>,
}

pub struct PlanarDelaunayTriangulation {
    corners: Vec<Corner>,
    edges: Vec<Edge>,
    triangles: Vec<Triangle>,
    anchor: Option<usize>,
    degenerate_edges: Vec<usize>,
    last_edge_id: i32,
}

impl PlanarDelaunayTriangulation {
    /// Triangulates the corners of the given objects. `objects[i]` are the
    /// triangulation corners of object `i` (`Storable.getTriangulationCorners`).
    pub fn new(objects: &[Vec<Point>]) -> PlanarDelaunayTriangulation {
        let mut t = PlanarDelaunayTriangulation {
            corners: Vec::new(),
            edges: Vec::new(),
            triangles: Vec::new(),
            anchor: None,
            degenerate_edges: Vec::new(),
            last_edge_id: 0,
        };
        let mut corner_list: Vec<usize> = Vec::new();
        for (oi, corners) in objects.iter().enumerate() {
            for c in corners {
                t.corners.push(Corner { object: Some(oi), coor: c.clone() });
                corner_list.push(t.corners.len() - 1);
            }
        }
        // Collections.shuffle(list, new Random(seed))
        let mut rnd = JavaRandom::new(SEED);
        let mut i = corner_list.len();
        while i > 1 {
            let j = rnd.next_int(i as i32) as usize;
            corner_list.swap(i - 1, j);
            i -= 1;
        }
        let bc = CRIT_INT;
        let b0 = t.add_corner(None, Point::Int(IntPoint { x: bc, y: 0 }));
        let b1 = t.add_corner(None, Point::Int(IntPoint { x: 0, y: bc }));
        let b2 = t.add_corner(None, Point::Int(IntPoint { x: -bc, y: -bc }));
        let bounding = [b0, b1, b2];
        let mut edge_lines = [0usize; 3];
        for i in 0..2 {
            edge_lines[i] = t.new_edge(bounding[i], bounding[i + 1]);
        }
        edge_lines[2] = t.new_edge(bounding[2], bounding[0]);
        let start_triangle = t.new_triangle(edge_lines, None);
        for e in edge_lines {
            t.edges[e].left = Some(start_triangle);
        }
        t.graph_insert(start_triangle, None);
        for c in corner_list {
            if let Some(tri) = t.position_locate(c) {
                t.split(tri, c);
            }
        }
        t
    }

    fn add_corner(&mut self, object: Option<usize>, coor: Point) -> usize {
        self.corners.push(Corner { object, coor });
        self.corners.len() - 1
    }

    fn new_edge(&mut self, start: usize, end: usize) -> usize {
        self.last_edge_id += 1;
        self.edges.push(Edge {
            start,
            end,
            id: self.last_edge_id,
            left: None,
            right: None,
        });
        self.edges.len() - 1
    }

    fn new_triangle(&mut self, edges: [usize; 3], first_parent: Option<usize>) -> usize {
        self.triangles.push(Triangle {
            edges,
            first_parent,
            children: Vec::new(),
            is_on_left: None,
        });
        self.triangles.len() - 1
    }

    /// The edges of the triangulation (`getEdgeLines`).
    pub fn get_edge_lines(&self) -> Vec<ResultEdge> {
        let mut result = Vec::new();
        for &e in &self.degenerate_edges {
            result.push(self.result_edge(e));
        }
        if let Some(anchor) = self.anchor {
            let mut leaf_edges: BTreeMap<i32, usize> = BTreeMap::new();
            self.get_leaf_edges(anchor, &mut leaf_edges);
            for (_, e) in leaf_edges {
                result.push(self.result_edge(e));
            }
        }
        result
    }

    fn result_edge(&self, e: usize) -> ResultEdge {
        let edge = &self.edges[e];
        let s = &self.corners[edge.start];
        let en = &self.corners[edge.end];
        ResultEdge {
            start_point: s.coor.clone(),
            start_object: s.object.unwrap_or(usize::MAX),
            end_point: en.coor.clone(),
            end_object: en.object.unwrap_or(usize::MAX),
        }
    }

    fn side_of(&self, corner: usize, p1: usize, p2: usize) -> Side {
        self.corners[corner].coor.side_of(&self.corners[p1].coor, &self.corners[p2].coor)
    }

    fn split(&mut self, triangle: usize, corner: usize) -> bool {
        let mut containing_edge: Option<usize> = None;
        for i in 0..3 {
            let ce = self.triangles[triangle].edges[i];
            let e = &self.edges[ce];
            let side = if e.left == Some(triangle) {
                self.side_of(corner, e.start, e.end)
            } else {
                self.side_of(corner, e.end, e.start)
            };
            if side == Side::OnTheRight {
                log::warn!("PlanarDelaunayTriangulation.split: corner is outside");
                return false;
            } else if side == Side::Collinear {
                if let Some(ce0) = containing_edge {
                    let Some(common) = self.common_corner(ce, ce0) else {
                        log::warn!("PlanarDelaunayTriangulation.split: common corner expected");
                        return false;
                    };
                    if self.corners[corner].object == self.corners[common].object {
                        return false;
                    }
                    let de = self.new_edge(corner, common);
                    self.degenerate_edges.push(de);
                    return true;
                }
                containing_edge = Some(ce);
            }
        }
        match containing_edge {
            None => {
                let Some(new_triangles) = self.split_at_inner_point(triangle, corner) else {
                    return false;
                };
                for nt in new_triangles {
                    self.graph_insert(nt, Some(triangle));
                }
                let edges = self.triangles[triangle].edges;
                for e in edges {
                    self.legalize_edge(corner, e);
                }
            }
            Some(containing_edge) => {
                let Some(neighbour) = self.other_neighbour(containing_edge, triangle) else {
                    return false;
                };
                let Some(nt) = self.split_at_border_point(triangle, corner, neighbour) else {
                    return false;
                };
                self.graph_insert(nt[0], Some(triangle));
                self.graph_insert(nt[1], Some(triangle));
                self.graph_insert(nt[2], Some(neighbour));
                self.graph_insert(nt[3], Some(neighbour));
                let edges = self.triangles[triangle].edges;
                for e in edges {
                    if e != containing_edge {
                        self.legalize_edge(corner, e);
                    }
                }
                let edges = self.triangles[neighbour].edges;
                for e in edges {
                    if e != containing_edge {
                        self.legalize_edge(corner, e);
                    }
                }
            }
        }
        true
    }

    fn legalize_edge(&mut self, corner: usize, edge: usize) -> bool {
        if self.is_legal(edge) {
            return false;
        }
        let (Some(left), Some(right)) = (self.edges[edge].left, self.edges[edge].right) else {
            return false;
        };
        let triangle_to_change = if self.opposite_corner(left, edge) == Some(corner) {
            right
        } else if self.opposite_corner(right, edge) == Some(corner) {
            left
        } else {
            log::warn!("PlanarDelaunayTriangulation.legalize_edge: edge lines inconsistent");
            return false;
        };
        let Some(flipped) = self.flip(edge) else {
            return true;
        };
        let fl = self.edges[flipped].left.unwrap();
        let fr = self.edges[flipped].right.unwrap();
        self.graph_insert(fl, Some(left));
        self.graph_insert(fr, Some(left));
        self.graph_insert(fl, Some(right));
        self.graph_insert(fr, Some(right));
        let edges = self.triangles[triangle_to_change].edges;
        for e in edges {
            if e != edge {
                self.legalize_edge(corner, e);
            }
        }
        true
    }

    // ------------------------------------------------------------ triangle graph

    fn graph_insert(&mut self, triangle: usize, parent: Option<usize>) {
        self.initialize_is_on_left(triangle);
        match parent {
            None => self.anchor = Some(triangle),
            Some(p) => self.triangles[p].children.push(triangle),
        }
    }

    fn position_locate(&self, corner: usize) -> Option<usize> {
        let anchor = self.anchor?;
        if self.triangles[anchor].children.is_empty() {
            return Some(anchor);
        }
        for &child in &self.triangles[anchor].children {
            if let Some(r) = self.position_locate_reku(corner, child) {
                return Some(r);
            }
        }
        log::warn!("TriangleGraph.position_locate: containing triangle not found");
        None
    }

    fn position_locate_reku(&self, corner: usize, triangle: usize) -> Option<usize> {
        if !self.triangle_contains(triangle, corner) {
            return None;
        }
        if self.triangles[triangle].children.is_empty() {
            return Some(triangle);
        }
        for &child in &self.triangles[triangle].children {
            if let Some(r) = self.position_locate_reku(corner, child) {
                return Some(r);
            }
        }
        log::warn!("TriangleGraph.position_locate_reku: containing triangle not found");
        None
    }

    // ------------------------------------------------------------ edge methods

    fn common_corner(&self, this: usize, other: usize) -> Option<usize> {
        let t = &self.edges[this];
        let o = &self.edges[other];
        if o.start == t.start || o.end == t.start {
            Some(t.start)
        } else if o.start == t.end || o.end == t.end {
            Some(t.end)
        } else {
            None
        }
    }

    fn other_neighbour(&self, edge: usize, triangle: usize) -> Option<usize> {
        let e = &self.edges[edge];
        if Some(triangle) == e.left {
            e.right
        } else if Some(triangle) == e.right {
            e.left
        } else {
            log::warn!("Edge.other_neighbour: inconsistent neighbour triangle");
            None
        }
    }

    fn is_legal(&self, edge: usize) -> bool {
        let e = &self.edges[edge];
        let (Some(left), Some(right)) = (e.left, e.right) else {
            return true;
        };
        let (Some(lo), Some(ro)) = (self.opposite_corner(left, edge), self.opposite_corner(right, edge)) else {
            return true;
        };
        let inside = self.corners[ro].coor.to_float().inside_circle(
            &self.corners[e.start].coor.to_float(),
            &self.corners[lo].coor.to_float(),
            &self.corners[e.end].coor.to_float(),
        );
        !inside
    }

    fn flip(&mut self, edge: usize) -> Option<usize> {
        let left = self.edges[edge].left?;
        let right = self.edges[edge].right?;
        let rc = self.opposite_corner(right, edge)?;
        let lc = self.opposite_corner(left, edge)?;
        let flipped = self.new_edge(rc, lc);
        let first_parent = left;
        let mut left_index = -1i32;
        let mut right_index = -1i32;
        for i in 0..3 {
            if self.triangles[left].edges[i] == edge {
                left_index = i as i32;
            }
            if self.triangles[right].edges[i] == edge {
                right_index = i as i32;
            }
        }
        if left_index < 0 || right_index < 0 {
            log::warn!("Edge.flip: edge line inconsistent");
            return None;
        }
        let li = left_index as usize;
        let ri = right_index as usize;
        let left_prev = self.triangles[left].edges[(li + 2) % 3];
        let left_next = self.triangles[left].edges[(li + 1) % 3];
        let right_prev = self.triangles[right].edges[(ri + 2) % 3];
        let right_next = self.triangles[right].edges[(ri + 1) % 3];

        let new_left = self.new_triangle([flipped, left_prev, right_next], Some(first_parent));
        self.edges[flipped].left = Some(new_left);
        if self.edges[left_prev].left == Some(left) {
            self.edges[left_prev].left = Some(new_left);
        } else {
            self.edges[left_prev].right = Some(new_left);
        }
        if self.edges[right_next].left == Some(right) {
            self.edges[right_next].left = Some(new_left);
        } else {
            self.edges[right_next].right = Some(new_left);
        }

        let new_right = self.new_triangle([flipped, right_prev, left_next], Some(first_parent));
        self.edges[flipped].right = Some(new_right);
        if self.edges[right_prev].left == Some(right) {
            self.edges[right_prev].left = Some(new_right);
        } else {
            self.edges[right_prev].right = Some(new_right);
        }
        if self.edges[left_next].left == Some(left) {
            self.edges[left_next].left = Some(new_right);
        } else {
            self.edges[left_next].right = Some(new_right);
        }
        Some(flipped)
    }

    // ------------------------------------------------------------ triangle methods

    fn get_corner(&self, triangle: usize, no: usize) -> Option<usize> {
        let e = &self.edges[self.triangles[triangle].edges[no]];
        if e.left == Some(triangle) {
            Some(e.start)
        } else if e.right == Some(triangle) {
            Some(e.end)
        } else {
            log::warn!("Triangle.get_corner: inconsistent edge lines");
            None
        }
    }

    fn opposite_corner(&self, triangle: usize, edge: usize) -> Option<usize> {
        let t = &self.triangles[triangle];
        let Some(no) = t.edges.iter().position(|e| *e == edge) else {
            log::warn!("Triangle.opposite_corner: edgeLine not found");
            return None;
        };
        let next = &self.edges[t.edges[(no + 1) % 3]];
        if next.left == Some(triangle) {
            Some(next.end)
        } else {
            Some(next.start)
        }
    }

    fn triangle_contains(&self, triangle: usize, corner: usize) -> bool {
        let t = &self.triangles[triangle];
        let Some(on_left) = t.is_on_left else {
            log::warn!("Triangle.contains: array isOnTheLeftOfEdgeLine not initialized");
            return false;
        };
        for i in 0..3 {
            let e = &self.edges[t.edges[i]];
            let side = self.side_of(corner, e.start, e.end);
            if on_left[i] {
                if side == Side::OnTheRight {
                    return false;
                }
            } else if side == Side::OnTheLeft {
                return false;
            }
        }
        true
    }

    fn get_leaf_edges(&self, triangle: usize, result: &mut BTreeMap<i32, usize>) {
        let t = &self.triangles[triangle];
        if t.children.is_empty() {
            for &e in &t.edges {
                let edge = &self.edges[e];
                if self.corners[edge.start].object.is_some() && self.corners[edge.end].object.is_some() {
                    result.insert(edge.id, e);
                }
            }
        } else {
            for &child in &t.children {
                if self.triangles[child].first_parent == Some(triangle) {
                    self.get_leaf_edges(child, result);
                }
            }
        }
    }

    fn split_at_inner_point(&mut self, this: usize, corner: usize) -> Option<[usize; 3]> {
        let c0 = self.get_corner(this, 0)?;
        let c1 = self.get_corner(this, 1)?;
        let c2 = self.get_corner(this, 2)?;
        // The original creates three edges here, which are never used (but consume ids).
        for c in [c0, c1, c2] {
            self.new_edge(c, corner);
        }
        let this_edges = self.triangles[this].edges;
        let e01 = self.new_edge(c1, corner);
        let e02 = self.new_edge(corner, c0);
        let t0 = self.new_triangle([this_edges[0], e01, e02], Some(this));
        let e11 = self.new_edge(c2, corner);
        let t1 = self.new_triangle([this_edges[1], e11, e01], Some(this));
        let t2 = self.new_triangle([this_edges[2], e02, e11], Some(this));
        let nt = [t0, t1, t2];
        for &t in &nt {
            let e = self.triangles[t].edges[0];
            if self.edges[e].left == Some(this) {
                self.edges[e].left = Some(t);
            } else {
                self.edges[e].right = Some(t);
            }
        }
        let e = self.triangles[t0].edges[1];
        self.edges[e].left = Some(t0);
        self.edges[e].right = Some(t1);
        let e = self.triangles[t1].edges[1];
        self.edges[e].left = Some(t1);
        self.edges[e].right = Some(t2);
        let e = self.triangles[t2].edges[1];
        self.edges[e].left = Some(t0);
        self.edges[e].right = Some(t2);
        Some(nt)
    }

    fn split_at_border_point(&mut self, this: usize, corner: usize, neighbour: usize) -> Option<[usize; 4]> {
        let mut this_no: i32 = -1;
        let mut neighbour_no: i32 = -1;
        let mut touching: Option<usize> = None;
        let mut other_touching: Option<usize> = None;
        for i in 0..3 {
            let ce = self.triangles[this].edges[i];
            if self.side_of(corner, self.edges[ce].start, self.edges[ce].end) == Side::Collinear {
                this_no = i as i32;
                touching = Some(ce);
            }
            let ce = self.triangles[neighbour].edges[i];
            if self.side_of(corner, self.edges[ce].start, self.edges[ce].end) == Side::Collinear {
                neighbour_no = i as i32;
                other_touching = Some(ce);
            }
        }
        if this_no < 0 || neighbour_no < 0 {
            log::warn!("Triangle.split_at_border_point: touching edge not found");
            return None;
        }
        if touching != other_touching {
            log::warn!("Triangle.split_at_border_point: edges inconsistent");
            return None;
        }
        let touching = touching.unwrap();
        let this_no = this_no as usize;
        let neighbour_no = neighbour_no as usize;
        let (ts, te) = (self.edges[touching].start, self.edges[touching].end);
        let (first_common, second_common) = if Some(this) == self.edges[touching].left {
            let a = self.new_edge(ts, corner);
            let b = self.new_edge(corner, te);
            (a, b)
        } else {
            let a = self.new_edge(te, corner);
            let b = self.new_edge(corner, ts);
            (a, b)
        };
        let prev_edge = self.triangles[this].edges[(this_no + 2) % 3];
        let this_splitting = if Some(this) == self.edges[prev_edge].left {
            let s = self.edges[prev_edge].start;
            self.new_edge(corner, s)
        } else {
            let s = self.edges[prev_edge].end;
            self.new_edge(corner, s)
        };
        let t0 = self.new_triangle([prev_edge, first_common, this_splitting], Some(this));
        if Some(this) == self.edges[prev_edge].left {
            self.edges[prev_edge].left = Some(t0);
        } else {
            self.edges[prev_edge].right = Some(t0);
        }
        self.edges[first_common].left = Some(t0);
        self.edges[this_splitting].left = Some(t0);

        let next_edge = self.triangles[this].edges[(this_no + 1) % 3];
        let t1 = self.new_triangle([this_splitting, second_common, next_edge], Some(this));
        self.edges[this_splitting].right = Some(t1);
        self.edges[second_common].left = Some(t1);
        if Some(this) == self.edges[next_edge].left {
            self.edges[next_edge].left = Some(t1);
        } else {
            self.edges[next_edge].right = Some(t1);
        }

        let neighbour_next = self.triangles[neighbour].edges[(neighbour_no + 1) % 3];
        let neighbour_splitting = if Some(neighbour) == self.edges[neighbour_next].left {
            let s = self.edges[neighbour_next].end;
            self.new_edge(s, corner)
        } else {
            let s = self.edges[neighbour_next].start;
            self.new_edge(s, corner)
        };
        let t2 = self.new_triangle([neighbour_splitting, first_common, neighbour_next], Some(neighbour));
        self.edges[neighbour_splitting].left = Some(t2);
        self.edges[first_common].right = Some(t2);
        if Some(neighbour) == self.edges[neighbour_next].left {
            self.edges[neighbour_next].left = Some(t2);
        } else {
            self.edges[neighbour_next].right = Some(t2);
        }

        let prev_edge = self.triangles[neighbour].edges[(neighbour_no + 2) % 3];
        let t3 = self.new_triangle([prev_edge, second_common, neighbour_splitting], Some(neighbour));
        if Some(neighbour) == self.edges[prev_edge].left {
            self.edges[prev_edge].left = Some(t3);
        } else {
            self.edges[prev_edge].right = Some(t3);
        }
        self.edges[second_common].right = Some(t3);
        self.edges[neighbour_splitting].right = Some(t3);
        Some([t0, t1, t2, t3])
    }

    fn initialize_is_on_left(&mut self, triangle: usize) {
        if self.triangles[triangle].is_on_left.is_some() {
            return;
        }
        let edges = self.triangles[triangle].edges;
        let mut arr = [false; 3];
        for i in 0..3 {
            arr[i] = self.edges[edges[i]].left == Some(triangle);
        }
        self.triangles[triangle].is_on_left = Some(arr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, y: i32) -> Point {
        Point::Int(IntPoint { x, y })
    }

    fn run(set: &str) -> String {
        let objects: Vec<Vec<Point>> = set
            .split(';')
            .map(|q| {
                let v: Vec<i32> = q.split(',').map(|x| x.parse().unwrap()).collect();
                vec![p(v[0], v[1])]
            })
            .collect();
        let t = PlanarDelaunayTriangulation::new(&objects);
        let mut r = String::new();
        for e in t.get_edge_lines() {
            r.push_str(&format!("{}-{} ", e.start_object, e.end_object));
        }
        r
    }

    /// Expected values produced by the Java implementation (freerouting 2.2.4).
    #[test]
    fn matches_java() {
        assert_eq!(run("0,0;1000,0;1000,1000;0,1000"), "1-0 0-3 1-3 1-2 ");
        assert_eq!(run("0,0;1000,0;2000,0"), "2-1 1-0 ");
        assert_eq!(
            run("5,7;300,40;120,900;800,800;400,400;401,401;-50,600;700,-30;250,260;250,260"),
            "9-8 8-1 8-4 1-4 8-6 4-6 6-0 0-1 8-0 3-2 2-6 4-5 5-3 5-2 5-6 1-7 3-7 7-5 7-4 0-7 "
        );
    }
}
