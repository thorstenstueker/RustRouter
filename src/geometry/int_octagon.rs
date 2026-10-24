//! Convex integer octagons with horizontal, vertical and diagonal border lines.

use super::basics::{java_round, Side, CRIT_INT, SQRT2};
use super::{FloatPoint, IntBox, IntPoint, Line, Simplex, Vector};
use std::fmt;

/// An octagon given by its 8 border line values:
/// `lx <= x <= rx`, `ly <= y <= uy`, `ulx <= x - y <= lrx`, `llx <= x + y <= urx`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntOctagon {
    pub lx: i32,
    pub ly: i32,
    pub rx: i32,
    pub uy: i32,
    pub ulx: i32,
    pub lrx: i32,
    pub llx: i32,
    pub urx: i32,
}

impl fmt::Display for IntOctagon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IntOctagon(lx={}, ly={}, rx={}, uy={}, ulx={}, lrx={}, llx={}, urx={})",
            self.lx, self.ly, self.rx, self.uy, self.ulx, self.lrx, self.llx, self.urx
        )
    }
}

/// The 8 directions in steps of 45 degree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FortyfiveDegreeDirection {
    Right,
    Right45,
    Up,
    Up45,
    Left,
    Left45,
    Down,
    Down45,
}

impl FortyfiveDegreeDirection {
    pub const ALL: [FortyfiveDegreeDirection; 8] = [
        FortyfiveDegreeDirection::Right,
        FortyfiveDegreeDirection::Right45,
        FortyfiveDegreeDirection::Up,
        FortyfiveDegreeDirection::Up45,
        FortyfiveDegreeDirection::Left,
        FortyfiveDegreeDirection::Left45,
        FortyfiveDegreeDirection::Down,
        FortyfiveDegreeDirection::Down45,
    ];

    pub fn get_direction(self) -> super::IntDirection {
        use super::IntDirection as D;
        match self {
            FortyfiveDegreeDirection::Right => D::RIGHT,
            FortyfiveDegreeDirection::Right45 => D::RIGHT45,
            FortyfiveDegreeDirection::Up => D::UP,
            FortyfiveDegreeDirection::Up45 => D::UP45,
            FortyfiveDegreeDirection::Left => D::LEFT,
            FortyfiveDegreeDirection::Left45 => D::LEFT45,
            FortyfiveDegreeDirection::Down => D::DOWN,
            FortyfiveDegreeDirection::Down45 => D::DOWN45,
        }
    }
}

impl IntOctagon {
    pub const EMPTY: IntOctagon = IntOctagon {
        lx: CRIT_INT,
        ly: CRIT_INT,
        rx: -CRIT_INT,
        uy: -CRIT_INT,
        ulx: CRIT_INT,
        lrx: -CRIT_INT,
        llx: CRIT_INT,
        urx: -CRIT_INT,
    };

    /// Parameter order as in the original: `lx, ly, rx, uy, ulx, lrx, llx, urx`.
    #[allow(clippy::too_many_arguments)]
    #[inline]
    pub const fn new(
        lx: i32,
        ly: i32,
        rx: i32,
        uy: i32,
        ulx: i32,
        lrx: i32,
        llx: i32,
        urx: i32,
    ) -> IntOctagon {
        IntOctagon {
            lx,
            ly,
            rx,
            uy,
            ulx,
            lrx,
            llx,
            urx,
        }
    }

    /// Creates a normalized octagon.
    #[allow(clippy::too_many_arguments)]
    pub fn get_instance(
        lx: i32,
        ly: i32,
        rx: i32,
        uy: i32,
        ulx: i32,
        lrx: i32,
        llx: i32,
        urx: i32,
    ) -> IntOctagon {
        IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx).normalize()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        *self == IntOctagon::EMPTY
            || self.lx > self.rx
            || self.ly > self.uy
            || self.llx > self.urx
            || self.ulx > self.lrx
    }

    pub fn bounding_box(&self) -> IntBox {
        IntBox::new(self.lx, self.ly, self.rx, self.uy)
    }

    pub fn dimension(&self) -> i32 {
        if self.is_empty() {
            return -1;
        }
        if self.rx > self.lx && self.uy > self.ly && self.lrx > self.ulx && self.urx > self.llx {
            2
        } else if self.rx == self.lx && self.uy == self.ly {
            0
        } else {
            1
        }
    }

    pub fn corner(&self, no: usize) -> IntPoint {
        match no {
            0 => IntPoint::new(self.llx - self.ly, self.ly),
            1 => IntPoint::new(self.lrx + self.ly, self.ly),
            2 => IntPoint::new(self.rx, self.rx - self.lrx),
            3 => IntPoint::new(self.rx, self.urx - self.rx),
            4 => IntPoint::new(self.urx - self.uy, self.uy),
            5 => IntPoint::new(self.ulx + self.uy, self.uy),
            6 => IntPoint::new(self.lx, self.lx - self.ulx),
            7 => IntPoint::new(self.lx, self.llx - self.lx),
            _ => panic!("IntOctagon.corner: no out of range"),
        }
    }

    pub fn get_id(&self) -> i32 {
        let mut r = self.lx;
        for v in [self.rx, self.ly, self.uy, self.llx, self.urx, self.ulx, self.lrx] {
            r = r.wrapping_mul(31).wrapping_add(v);
        }
        r
    }

    pub fn corner_y(&self, no: usize) -> i32 {
        match no {
            0 | 1 => self.ly,
            2 => self.rx - self.lrx,
            3 => self.urx - self.rx,
            4 | 5 => self.uy,
            6 => self.lx - self.ulx,
            7 => self.llx - self.lx,
            _ => panic!("IntOctagon.corner: no out of range"),
        }
    }

    pub fn corner_x(&self, no: usize) -> i32 {
        match no {
            0 => self.llx - self.ly,
            1 => self.lrx + self.ly,
            2 | 3 => self.rx,
            4 => self.urx - self.uy,
            5 => self.ulx + self.uy,
            6 | 7 => self.lx,
            _ => panic!("IntOctagon.corner: no out of range"),
        }
    }

    pub fn area(&self) -> f64 {
        // Like Java, the inner differences are computed in (wrapping) int arithmetic.
        let (lx, ly, rx, uy) = (self.lx, self.ly, self.rx, self.uy);
        let (ulx, lrx, llx, urx) = (self.ulx, self.lrx, self.llx, self.urx);
        let d = |v: i32| v as f64;
        let mut r = d(llx.wrapping_sub(ly)) * d(ly.wrapping_sub(llx).wrapping_add(lx));
        r += d(lrx.wrapping_add(ly)) * d(rx.wrapping_sub(lrx).wrapping_sub(ly));
        r += d(rx) * d(urx.wrapping_sub(rx.wrapping_mul(2)).wrapping_sub(ly).wrapping_add(uy).wrapping_add(lrx));
        r += d(urx.wrapping_sub(uy)) * d(uy.wrapping_sub(urx).wrapping_add(rx));
        r += d(ulx.wrapping_add(uy)) * d(lx.wrapping_sub(ulx).wrapping_sub(uy));
        r += d(lx) * d(llx.wrapping_sub(lx.wrapping_mul(2)).wrapping_sub(uy).wrapping_add(ly).wrapping_add(ulx));
        0.5 * r.abs()
    }

    pub fn border_line(&self, no: usize) -> Line {
        match no {
            0 => Line::from_coords(0, self.ly, 1, self.ly),
            1 => Line::from_coords(self.lrx, 0, self.lrx + 1, 1),
            2 => Line::from_coords(self.rx, 0, self.rx, 1),
            3 => Line::from_coords(self.urx, 0, self.urx - 1, 1),
            4 => Line::from_coords(0, self.uy, -1, self.uy),
            5 => Line::from_coords(self.ulx, 0, self.ulx - 1, -1),
            6 => Line::from_coords(self.lx, 0, self.lx, -1),
            7 => Line::from_coords(self.llx, 0, self.llx + 1, -1),
            _ => panic!("IntOctagon.borderLine: no out of range"),
        }
    }

    pub fn translate_by(&self, v: &Vector) -> IntOctagon {
        if v.is_zero() {
            return *self;
        }
        let r = v.to_int();
        IntOctagon::new(
            self.lx + r.x,
            self.ly + r.y,
            self.rx + r.x,
            self.uy + r.y,
            self.ulx + r.x - r.y,
            self.lrx + r.x - r.y,
            self.llx + r.x + r.y,
            self.urx + r.x + r.y,
        )
    }

    pub fn max_width(&self) -> f64 {
        let w1 = (self.rx - self.lx).max(self.uy - self.ly) as f64;
        let w2 = (self.urx - self.llx).max(self.lrx - self.ulx) as f64;
        w1.max(w2 / SQRT2)
    }

    pub fn min_width(&self) -> f64 {
        let w1 = (self.rx - self.lx).min(self.uy - self.ly) as f64;
        let w2 = (self.urx - self.llx).min(self.lrx - self.ulx) as f64;
        w1.min(w2 / SQRT2)
    }

    pub fn offset(&self, distance: f64) -> IntOctagon {
        let width = java_round(distance);
        if width == 0 {
            return *self;
        }
        let dia = java_round(SQRT2 * distance);
        IntOctagon::new(
            self.lx - width,
            self.ly - width,
            self.rx + width,
            self.uy + width,
            self.ulx - dia,
            self.lrx + dia,
            self.llx - dia,
            self.urx + dia,
        )
        .normalize()
    }

    pub fn enlarge(&self, offset: f64) -> IntOctagon {
        self.offset(offset)
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        if (self.lx as f64) > p.x || (self.ly as f64) > p.y || (self.rx as f64) < p.x || (self.uy as f64) < p.y {
            return false;
        }
        let t1 = p.x - p.y;
        let t2 = p.x + p.y;
        self.ulx as f64 <= t1 && self.lrx as f64 >= t1 && self.llx as f64 <= t2 && self.urx as f64 >= t2
    }

    pub fn contains_point(&self, p: &IntPoint) -> bool {
        if self.lx > p.x || self.ly > p.y || self.rx < p.x || self.uy < p.y {
            return false;
        }
        let t1 = p.x - p.y;
        let t2 = p.x + p.y;
        self.ulx <= t1 && self.lrx >= t1 && self.llx <= t2 && self.urx >= t2
    }

    pub fn union(&self, o: &IntOctagon) -> IntOctagon {
        IntOctagon::new(
            self.lx.min(o.lx),
            self.ly.min(o.ly),
            self.rx.max(o.rx),
            self.uy.max(o.uy),
            self.ulx.min(o.ulx),
            self.lrx.max(o.lrx),
            self.llx.min(o.llx),
            self.urx.max(o.urx),
        )
    }

    pub fn intersection(&self, o: &IntOctagon) -> IntOctagon {
        IntOctagon::new(
            self.lx.max(o.lx),
            self.ly.max(o.ly),
            self.rx.min(o.rx),
            self.uy.min(o.uy),
            self.ulx.max(o.ulx),
            self.lrx.min(o.lrx),
            self.llx.max(o.llx),
            self.urx.min(o.urx),
        )
        .normalize()
    }

    /// Changes the border lines, so that each of them touches the octagon.
    pub fn normalize(&self) -> IntOctagon {
        if self.lx > self.rx || self.ly > self.uy || self.llx > self.urx || self.ulx > self.lrx {
            return IntOctagon::EMPTY;
        }
        let mut lx = self.lx;
        let mut rx = self.rx;
        let mut ly = self.ly;
        let mut uy = self.uy;
        let mut llx = self.llx;
        let mut ulx = self.ulx;
        let mut lrx = self.lrx;
        let mut urx = self.urx;
        if lx < llx - uy {
            lx = llx - uy;
        }
        if lx < ulx + ly {
            lx = ulx + ly;
        }
        if rx > urx - ly {
            rx = urx - ly;
        }
        if rx > lrx + uy {
            rx = lrx + uy;
        }
        if ly < lx - lrx {
            ly = lx - lrx;
        }
        if ly < llx - rx {
            ly = llx - rx;
        }
        if uy > urx - lx {
            uy = urx - lx;
        }
        if uy > rx - ulx {
            uy = rx - ulx;
        }
        if llx - lx < ly {
            llx = lx + ly;
        }
        if rx - lrx < ly {
            lrx = rx - ly;
        }
        if urx - rx > uy {
            urx = uy + rx;
        }
        if lx - ulx > uy {
            ulx = lx - uy;
        }
        // ceil/floor of x / 2 with integer shifts (exact, avoids libm calls in this hot path).
        let half_ceil = |x: i32| (x >> 1) + (x & 1);
        let diag_upper_y = half_ceil(urx.wrapping_sub(ulx));
        if uy > diag_upper_y {
            uy = diag_upper_y;
        }
        let diag_lower_y = llx.wrapping_sub(lrx) >> 1;
        if ly < diag_lower_y {
            ly = diag_lower_y;
        }
        let diag_right_x = half_ceil(urx.wrapping_add(lrx));
        if rx > diag_right_x {
            rx = diag_right_x;
        }
        let diag_left_x = llx.wrapping_add(ulx) >> 1;
        if lx < diag_left_x {
            lx = diag_left_x;
        }
        if lx > rx || ly > uy || llx > urx || ulx > lrx {
            return IntOctagon::EMPTY;
        }
        IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx)
    }

    pub fn is_normalized(&self) -> bool {
        *self == self.normalize()
    }

    pub fn to_simplex(&self) -> Simplex {
        if self.is_empty() {
            return Simplex::empty();
        }
        let lines: Vec<Line> = (0..8).map(|i| self.border_line(i)).collect();
        Simplex::new(lines).remove_redundant_lines()
    }

    pub fn side_of_border_line(&self, x: i32, y: i32, border_line_no: usize) -> Side {
        let tmp = match border_line_no {
            0 => self.ly - y,
            1 => x - y - self.lrx,
            2 => x - self.rx,
            3 => x + y - self.urx,
            4 => y - self.uy,
            5 => self.ulx + y - x,
            6 => self.lx - x,
            7 => self.llx - x - y,
            _ => {
                log::warn!("IntOctagon.sideOfBorderLine: borderLineNo out of range");
                0
            }
        };
        if tmp < 0 {
            Side::OnTheLeft
        } else if tmp > 0 {
            Side::OnTheRight
        } else {
            Side::Collinear
        }
    }

    pub fn is_contained_in_box(&self, b: &IntBox) -> bool {
        self.lx >= b.ll.x && self.ly >= b.ll.y && self.rx <= b.ur.x && self.uy <= b.ur.y
    }

    pub fn is_contained_in(&self, o: &IntOctagon) -> bool {
        self.lx >= o.lx
            && self.ly >= o.ly
            && self.rx <= o.rx
            && self.uy <= o.uy
            && self.llx >= o.llx
            && self.ulx >= o.ulx
            && self.lrx <= o.lrx
            && self.urx <= o.urx
    }

    /// Returns true, if `other` is contained in this octagon.
    pub fn contains_octagon(&self, other: &IntOctagon) -> bool {
        other.is_contained_in(self)
    }

    #[inline]
    pub fn intersects(&self, o: &IntOctagon) -> bool {
        if o.lx.max(self.lx) > o.rx.min(self.rx) {
            return false;
        }
        if o.ly.max(self.ly) > o.uy.min(self.uy) {
            return false;
        }
        if o.llx.max(self.llx) > o.urx.min(self.urx) {
            return false;
        }
        o.ulx.max(self.ulx) <= o.lrx.min(self.lrx)
    }

    pub fn overlaps(&self, o: &IntOctagon) -> bool {
        if o.lx.max(self.lx) >= o.rx.min(self.rx) {
            return false;
        }
        if o.ly.max(self.ly) >= o.uy.min(self.uy) {
            return false;
        }
        if o.llx.max(self.llx) >= o.urx.min(self.urx) {
            return false;
        }
        o.ulx.max(self.ulx) < o.lrx.min(self.lrx)
    }

    pub fn left_x_value(&self, y: i32) -> i32 {
        self.lx.max(self.ulx + y).max(self.llx - y)
    }

    pub fn right_x_value(&self, y: i32) -> i32 {
        self.rx.min(self.urx - y).min(self.lrx + y)
    }

    pub fn lower_y_value(&self, x: i32) -> i32 {
        self.ly.max(self.llx - x).max(x - self.lrx)
    }

    pub fn upper_y_value(&self, x: i32) -> i32 {
        self.uy.min(x - self.ulx).min(self.urx - x)
    }

    pub fn compare(&self, o: &IntOctagon, edge_index: usize) -> Side {
        let side = |a: i32, b: i32| {
            if a > b {
                Side::OnTheLeft
            } else if a < b {
                Side::OnTheRight
            } else {
                Side::Collinear
            }
        };
        match edge_index {
            0 => side(self.ly, o.ly),
            1 => side(o.lrx, self.lrx),
            2 => side(o.rx, self.rx),
            3 => side(o.urx, self.urx),
            4 => side(o.uy, self.uy),
            5 => side(self.ulx, o.ulx),
            6 => side(self.lx, o.lx),
            7 => side(self.llx, o.llx),
            _ => panic!("IntOctagon.compare: edgeIndex out of range"),
        }
    }

    pub fn border_point(&self, p: &IntPoint, dir: FortyfiveDegreeDirection) -> IntPoint {
        use FortyfiveDegreeDirection as F;
        let (x, y);
        match dir {
            F::Right => {
                x = self.rx.min(self.urx - p.y).min(self.lrx + p.y);
                y = p.y;
            }
            F::Left => {
                x = self.lx.max(self.ulx + p.y).max(self.llx - p.y);
                y = p.y;
            }
            F::Up => {
                x = p.x;
                y = self.uy.min(p.x - self.ulx).min(self.urx - p.x);
            }
            F::Down => {
                x = p.x;
                y = self.ly.max(self.llx - p.x).max(p.x - self.lrx);
            }
            F::Right45 => {
                let rx = (0.5 * (p.x - p.y + self.urx) as f64).ceil() as i32;
                x = rx.min(self.rx).min(p.x - p.y + self.uy);
                y = p.y - p.x + x;
            }
            F::Up45 => {
                let rx = (0.5 * (p.x + p.y + self.ulx) as f64).floor() as i32;
                x = rx.max(self.lx).max(p.x + p.y - self.uy);
                y = p.y + p.x - x;
            }
            F::Left45 => {
                let rx = (0.5 * (p.x - p.y + self.llx) as f64).floor() as i32;
                x = rx.max(self.lx).max(p.x - p.y + self.ly);
                y = p.y - p.x + x;
            }
            F::Down45 => {
                let rx = (0.5 * (p.x + p.y + self.lrx) as f64).ceil() as i32;
                x = rx.min(self.rx).min(p.x + p.y - self.ly);
                y = p.y + p.x - x;
            }
        }
        IntPoint::new(x, y)
    }

    pub fn nearest_border_projections(&self, p: &IntPoint, max_result_points: usize) -> Vec<IntPoint> {
        if !self.contains_point(p) || max_result_points == 0 {
            return Vec::new();
        }
        let n = max_result_points.min(8);
        let mut result: Vec<Option<IntPoint>> = vec![None; n];
        let mut min_dist = vec![f64::MAX; n];
        let inside = p.to_float();
        for dir in FortyfiveDegreeDirection::ALL {
            let bp = self.border_point(p, dir);
            let d = inside.distance_square(&bp.to_float());
            for i in 0..n {
                if d < min_dist[i] {
                    for k in (i + 1..n).rev() {
                        min_dist[k] = min_dist[k - 1];
                        result[k] = result[k - 1];
                    }
                    min_dist[i] = d;
                    result[i] = Some(bp);
                    break;
                }
            }
        }
        result.into_iter().flatten().collect()
    }

    pub fn border_line_side_of(&self, p: &FloatPoint, line_index: usize, tol: f64) -> Side {
        match line_index {
            0 => {
                if p.y > self.ly as f64 + tol {
                    Side::OnTheRight
                } else if p.y < self.ly as f64 - tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            2 => {
                if p.x < self.rx as f64 - tol {
                    Side::OnTheRight
                } else if p.x > self.rx as f64 + tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            4 => {
                if p.y < self.uy as f64 - tol {
                    Side::OnTheRight
                } else if p.y > self.uy as f64 + tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            6 => {
                if p.x > self.lx as f64 + tol {
                    Side::OnTheRight
                } else if p.x < self.lx as f64 - tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            1 => {
                let t = p.y - p.x + self.lrx as f64;
                if t > tol {
                    Side::OnTheRight
                } else if t < -tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            3 => {
                let t = p.x + p.y - self.urx as f64;
                if t < -tol {
                    Side::OnTheRight
                } else if t > tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            5 => {
                let t = p.y - p.x + self.ulx as f64;
                if t < -tol {
                    Side::OnTheRight
                } else if t > tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            7 => {
                let t = p.x + p.y - self.llx as f64;
                if t > tol {
                    Side::OnTheRight
                } else if t < -tol {
                    Side::OnTheLeft
                } else {
                    Side::Collinear
                }
            }
            _ => Side::Collinear,
        }
    }

    pub fn is_int_box(&self) -> bool {
        self.llx == self.lx + self.ly
            && self.lrx == self.rx - self.ly
            && self.urx == self.rx + self.uy
            && self.ulx == self.lx - self.uy
    }

    /// Cuts this octagon out of the box `d`.
    pub fn cutout_from_box(&self, d: &IntBox) -> Vec<IntOctagon> {
        let c = self.intersection(&d.to_int_octagon());
        if self.is_empty() || c.dimension() < self.dimension() {
            return vec![d.to_int_octagon()];
        }
        let mut boxes = [
            IntBox::new(d.ll.x, c.llx - c.lx, c.lx, c.lx - c.ulx),
            IntBox::new(c.rx, c.rx - c.lrx, d.ur.x, c.urx - c.rx),
            IntBox::new(c.llx - c.ly, d.ll.y, c.lrx + c.ly, c.ly),
            IntBox::new(c.ulx + c.uy, c.uy, c.urx - c.uy, d.ur.y),
        ];
        let ci = CRIT_INT;
        let mut octs = [
            IntOctagon::new(d.ll.x, boxes[0].ur.y, boxes[3].ll.x, d.ur.y, -ci, c.ulx, -ci, ci).normalize(),
            IntOctagon::new(d.ll.x, d.ll.y, boxes[2].ll.x, boxes[0].ll.y, -ci, ci, -ci, c.llx).normalize(),
            IntOctagon::new(boxes[2].ur.x, d.ll.y, d.ur.x, boxes[1].ll.y, c.lrx, ci, -ci, ci).normalize(),
            IntOctagon::new(boxes[3].ur.x, boxes[1].ur.y, d.ur.x, d.ur.y, -ci, ci, c.urx, ci).normalize(),
        ];
        let b = boxes[0];
        let o = octs[0];
        if b.ur.x - b.ll.x > o.uy - o.ly {
            boxes[0] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, o.uy);
            octs[0] = IntOctagon::new(b.ur.x, o.ly, o.rx, o.uy, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let b = boxes[3];
        let o = octs[0];
        if b.ur.y - b.ll.y > o.rx - o.lx {
            boxes[3] = IntBox::new(o.lx, b.ll.y, b.ur.x, b.ur.y);
            octs[0] = IntOctagon::new(o.lx, o.ly, o.rx, b.ll.y, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let b = boxes[3];
        let o = octs[3];
        if b.ur.y - b.ll.y > o.rx - o.lx {
            boxes[3] = IntBox::new(b.ll.x, b.ll.y, o.rx, b.ur.y);
            octs[3] = IntOctagon::new(o.lx, o.ly, o.rx, o.uy, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let b = boxes[1];
        let o = octs[3];
        if b.ur.x - b.ll.x > o.uy - o.ly {
            boxes[1] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, o.uy);
            octs[3] = IntOctagon::new(o.lx, o.ly, b.ll.x, o.uy, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let b = boxes[1];
        let o = octs[2];
        if b.ur.x - b.ll.x > o.uy - o.ly {
            boxes[1] = IntBox::new(b.ll.x, o.ly, b.ur.x, b.ur.y);
            octs[2] = IntOctagon::new(o.lx, o.ly, b.ll.x, o.uy, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let b = boxes[2];
        let o = octs[2];
        if b.ur.y - b.ll.y > o.rx - o.lx {
            boxes[2] = IntBox::new(b.ll.x, b.ll.y, o.rx, b.ur.y);
            octs[2] = IntOctagon::new(o.lx, b.ur.y, o.rx, o.uy, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let b = boxes[2];
        let o = octs[1];
        if b.ur.y - b.ll.y > o.rx - o.lx {
            boxes[2] = IntBox::new(o.lx, b.ll.y, b.ur.x, b.ur.y);
            octs[1] = IntOctagon::new(o.lx, b.ur.y, o.rx, o.uy, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let b = boxes[0];
        let o = octs[1];
        if b.ur.x - b.ll.x > o.uy - o.ly {
            boxes[0] = IntBox::new(b.ll.x, o.ly, b.ur.x, b.ur.y);
            octs[1] = IntOctagon::new(b.ur.x, o.ly, o.rx, o.uy, o.ulx, o.lrx, o.llx, o.urx).normalize();
        }
        let mut result: Vec<IntOctagon> = boxes.iter().map(|b| b.to_int_octagon()).collect();
        result.extend_from_slice(&octs);
        result
    }

    /// Cuts this octagon out of the octagon `d`.
    pub fn cutout_from_octagon(&self, d: &IntOctagon) -> Vec<IntOctagon> {
        let c = self.intersection(d);
        if self.is_empty() || c.dimension() < self.dimension() {
            return vec![*d];
        }
        let mut r = [IntOctagon::EMPTY; 8];
        let mut tmp = c.llx - c.lx;
        r[0] = IntOctagon::new(d.lx, tmp, c.lx, c.lx - c.ulx, d.ulx, d.lrx, d.llx, d.urx);
        let mut tmp2 = c.llx - c.ly;
        r[1] = IntOctagon::new(d.lx, d.ly, tmp2, tmp, d.ulx, d.lrx, d.llx, c.llx);
        tmp = c.lrx + c.ly;
        r[2] = IntOctagon::new(tmp2, d.ly, tmp, c.ly, d.ulx, d.lrx, d.llx, d.urx);
        tmp2 = c.rx - c.lrx;
        r[3] = IntOctagon::new(tmp, d.ly, d.rx, tmp2, c.lrx, d.lrx, d.llx, d.urx);
        tmp = c.urx - c.rx;
        r[4] = IntOctagon::new(c.rx, tmp2, d.rx, tmp, d.ulx, d.lrx, d.llx, d.urx);
        tmp2 = c.urx - c.uy;
        r[5] = IntOctagon::new(tmp2, tmp, d.rx, d.uy, d.ulx, d.lrx, c.urx, d.urx);
        tmp = c.ulx + c.uy;
        r[6] = IntOctagon::new(tmp, c.uy, tmp2, d.uy, d.ulx, d.lrx, d.llx, d.urx);
        tmp2 = c.lx - c.ulx;
        r[7] = IntOctagon::new(d.lx, tmp2, tmp, d.uy, d.ulx, c.ulx, d.llx, d.urx);
        for o in r.iter_mut() {
            *o = o.normalize();
        }

        let c1 = r[0];
        let c2 = r[7];
        if !(c1.is_empty() || c2.is_empty())
            && c1.rx - c1.left_x_value(c1.uy) > c2.upper_y_value(c1.rx) - c2.ly
        {
            let n1 = IntOctagon::new(c1.lx.min(c2.lx), c1.ly, c1.rx, c2.uy, c2.ulx, c1.lrx, c1.llx, c2.urx);
            let n2 = IntOctagon::new(n1.rx, c2.ly, c2.rx, c2.uy, c2.ulx, c2.lrx, c2.llx, c2.urx);
            r[0] = n1.normalize();
            r[7] = n2.normalize();
        }
        let c1 = r[7];
        let c2 = r[6];
        if !(c1.is_empty() || c2.is_empty())
            && c2.upper_y_value(c1.rx) - c2.ly > c1.rx - c1.left_x_value(c2.ly)
        {
            let n2 = IntOctagon::new(c1.lx, c2.ly, c2.rx, c2.uy.max(c1.uy), c1.ulx, c2.lrx, c1.llx, c2.urx);
            let n1 = IntOctagon::new(c1.lx, c1.ly, c1.rx, n2.ly, c1.ulx, c1.lrx, c1.llx, c1.urx);
            r[7] = n1.normalize();
            r[6] = n2.normalize();
        }
        let c1 = r[6];
        let c2 = r[5];
        if !(c1.is_empty() || c2.is_empty())
            && c2.upper_y_value(c1.rx) - c1.ly > c2.right_x_value(c1.ly) - c2.lx
        {
            let n1 = IntOctagon::new(c1.lx, c1.ly, c2.rx, c2.uy.max(c1.uy), c1.ulx, c2.lrx, c1.llx, c2.urx);
            let n2 = IntOctagon::new(c2.lx, c2.ly, c2.rx, n1.ly, c2.ulx, c2.lrx, c2.llx, c2.urx);
            r[6] = n1.normalize();
            r[5] = n2.normalize();
        }
        let c1 = r[5];
        let c2 = r[4];
        if !(c1.is_empty() || c2.is_empty())
            && c2.right_x_value(c2.uy) - c2.lx > c1.upper_y_value(c2.lx) - c2.uy
        {
            let n2 = IntOctagon::new(c2.lx, c2.ly, c2.rx.max(c1.rx), c1.uy, c1.ulx, c2.lrx, c2.llx, c1.urx);
            let n1 = IntOctagon::new(c1.lx, c1.ly, n2.lx, c1.uy, c1.ulx, c1.lrx, c1.llx, c1.urx);
            r[5] = n1.normalize();
            r[4] = n2.normalize();
        }
        let c1 = r[4];
        let c2 = r[3];
        if !(c1.is_empty() || c2.is_empty())
            && c1.right_x_value(c1.ly) - c1.lx > c1.ly - c2.lower_y_value(c1.lx)
        {
            let n1 = IntOctagon::new(c1.lx, c2.ly, c2.rx.max(c1.rx), c1.uy, c1.ulx, c2.lrx, c2.llx, c1.urx);
            let n2 = IntOctagon::new(c2.lx, c2.ly, n1.lx, c2.uy, c2.ulx, c2.lrx, c2.llx, c2.urx);
            r[4] = n1.normalize();
            r[3] = n2.normalize();
        }
        let c1 = r[3];
        let c2 = r[2];
        if !(c1.is_empty() || c2.is_empty())
            && c2.uy - c2.lower_y_value(c2.rx) > c1.right_x_value(c2.uy) - c2.rx
        {
            let n2 = IntOctagon::new(c2.lx, c1.ly.min(c2.ly), c1.rx, c2.uy, c2.ulx, c1.lrx, c2.llx, c1.urx);
            let n1 = IntOctagon::new(c1.lx, n2.uy, c1.rx, c1.uy, c1.ulx, c1.lrx, c1.llx, c1.urx);
            r[3] = n1.normalize();
            r[2] = n2.normalize();
        }
        let c1 = r[2];
        let c2 = r[1];
        if !(c1.is_empty() || c2.is_empty())
            && c1.uy - c1.lower_y_value(c1.lx) > c1.lx - c2.left_x_value(c1.uy)
        {
            let n1 = IntOctagon::new(c2.lx, c1.ly.min(c2.ly), c1.rx, c1.uy, c2.ulx, c1.lrx, c2.llx, c1.urx);
            let n2 = IntOctagon::new(c2.lx, n1.uy, c2.rx, c2.uy, c2.ulx, c2.lrx, c2.llx, c2.urx);
            r[2] = n1.normalize();
            r[1] = n2.normalize();
        }
        let c1 = r[1];
        let c2 = r[0];
        if !(c1.is_empty() || c2.is_empty())
            && c2.rx - c2.left_x_value(c2.ly) > c2.ly - c1.lower_y_value(c2.rx)
        {
            let n2 = IntOctagon::new(c2.lx.min(c1.lx), c1.ly, c2.rx, c2.uy, c2.ulx, c1.lrx, c1.llx, c2.urx);
            let n1 = IntOctagon::new(n2.rx, c1.ly, c1.rx, c1.uy, c1.ulx, c1.lrx, c1.llx, c1.urx);
            r[1] = n1.normalize();
            r[0] = n2.normalize();
        }
        r.to_vec()
    }
}
