//! Axis parallel integer boxes.

use super::basics::{java_round, Side, CRIT_INT};
use super::{FloatPoint, IntDirection, IntOctagon, IntPoint, Line, Simplex, Vector};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntBox {
    pub ll: IntPoint,
    pub ur: IntPoint,
}

impl fmt::Display for IntBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IntBox(ll={}, ur={})", self.ll, self.ur)
    }
}

impl IntBox {
    pub const EMPTY: IntBox = IntBox {
        ll: IntPoint::new(CRIT_INT, CRIT_INT),
        ur: IntPoint::new(-CRIT_INT, -CRIT_INT),
    };

    #[inline]
    pub const fn new(llx: i32, lly: i32, urx: i32, ury: i32) -> IntBox {
        IntBox {
            ll: IntPoint::new(llx, lly),
            ur: IntPoint::new(urx, ury),
        }
    }

    #[inline]
    pub const fn from_points(ll: IntPoint, ur: IntPoint) -> IntBox {
        IntBox { ll, ur }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ll.x > self.ur.x || self.ll.y > self.ur.y
    }

    #[inline]
    pub fn width(&self) -> i32 {
        self.ur.x - self.ll.x
    }

    #[inline]
    pub fn height(&self) -> i32 {
        self.ur.y - self.ll.y
    }

    pub fn max_width(&self) -> f64 {
        self.width().max(self.height()) as f64
    }

    pub fn min_width(&self) -> f64 {
        self.width().min(self.height()) as f64
    }

    pub fn area(&self) -> f64 {
        (self.ur.x.wrapping_sub(self.ll.x) as f64) * (self.ur.y.wrapping_sub(self.ll.y) as f64)
    }

    pub fn circumference(&self) -> f64 {
        2.0 * ((self.ur.x - self.ll.x) + (self.ur.y - self.ll.y)) as f64
    }

    pub fn corner(&self, no: usize) -> IntPoint {
        match no {
            0 => self.ll,
            1 => IntPoint::new(self.ur.x, self.ll.y),
            2 => self.ur,
            3 => IntPoint::new(self.ll.x, self.ur.y),
            _ => panic!("IntBox.corner: no out of range"),
        }
    }

    pub fn dimension(&self) -> i32 {
        if self.is_empty() {
            return -1;
        }
        if self.ll == self.ur {
            return 0;
        }
        if self.ur.x == self.ll.x || self.ll.y == self.ur.y {
            return 1;
        }
        2
    }

    pub fn contains_inside(&self, p: &IntPoint) -> bool {
        p.x > self.ll.x && p.x < self.ur.x && p.y > self.ll.y && p.y < self.ur.y
    }

    pub fn contains_point(&self, p: &IntPoint) -> bool {
        p.x >= self.ll.x && p.y >= self.ll.y && p.x <= self.ur.x && p.y <= self.ur.y
    }

    pub fn nearest_point(&self, from: &FloatPoint) -> FloatPoint {
        let x = if from.x <= self.ll.x as f64 {
            self.ll.x as f64
        } else if from.x >= self.ur.x as f64 {
            self.ur.x as f64
        } else {
            from.x
        };
        let y = if from.y <= self.ll.y as f64 {
            self.ll.y as f64
        } else if from.y >= self.ur.y as f64 {
            self.ur.y as f64
        } else {
            from.y
        };
        FloatPoint::new(x, y)
    }

    pub fn nearest_border_projections(&self, p: &IntPoint, max_result_points: usize) -> Vec<IntPoint> {
        if max_result_points == 0 {
            return Vec::new();
        }
        let max_result_points = max_result_points.min(2);
        let lower_h = p.x - self.ll.x;
        let upper_h = self.ur.x - p.x;
        let lower_v = p.y - self.ll.y;
        let upper_v = self.ur.y - p.y;
        let mut min_diff;
        let mut second_min_diff;
        let mut npx;
        let mut npy = p.y;
        let mut snpx;
        let mut snpy = p.y;
        if lower_h <= upper_h {
            min_diff = lower_h;
            second_min_diff = upper_h;
            npx = self.ll.x;
            snpx = self.ur.x;
        } else {
            min_diff = upper_h;
            second_min_diff = lower_h;
            npx = self.ur.x;
            snpx = self.ll.x;
        }
        if lower_v < min_diff {
            second_min_diff = min_diff;
            min_diff = lower_v;
            snpx = npx;
            snpy = npy;
            npx = p.x;
            npy = self.ll.y;
        } else if lower_v < second_min_diff {
            second_min_diff = lower_v;
            snpx = p.x;
            snpy = self.ll.y;
        }
        if upper_v < min_diff {
            snpx = npx;
            snpy = npy;
            npx = p.x;
            npy = self.ur.y;
        } else if upper_v < second_min_diff {
            snpx = p.x;
            snpy = self.ur.y;
        }
        let mut result = vec![IntPoint::new(npx, npy)];
        if max_result_points > 1 {
            result.push(IntPoint::new(snpx, snpy));
        }
        result
    }

    pub fn distance(&self, from: &FloatPoint) -> f64 {
        from.distance(&self.nearest_point(from))
    }

    pub fn weighted_distance(&self, other: &IntBox, hw: f64, vw: f64) -> f64 {
        let max_ll_x = self.ll.x.max(other.ll.x) as f64;
        let max_ll_y = self.ll.y.max(other.ll.y) as f64;
        let min_ur_x = self.ur.x.min(other.ur.x) as f64;
        let min_ur_y = self.ur.y.min(other.ur.y) as f64;
        if min_ur_x >= max_ll_x {
            (vw * (max_ll_y - min_ur_y)).max(0.0)
        } else if min_ur_y >= max_ll_y {
            (hw * (max_ll_x - min_ur_x)).max(0.0)
        } else {
            let dx = (max_ll_x - min_ur_x) * hw;
            let dy = (max_ll_y - min_ur_y) * vw;
            (dx * dx + dy * dy).sqrt()
        }
    }

    pub fn get_id(&self) -> i32 {
        31i32.wrapping_mul(self.ll.get_id()).wrapping_add(self.ur.get_id())
    }

    pub fn union(&self, o: &IntBox) -> IntBox {
        IntBox::new(
            self.ll.x.min(o.ll.x),
            self.ll.y.min(o.ll.y),
            self.ur.x.max(o.ur.x),
            self.ur.y.max(o.ur.y),
        )
    }

    pub fn intersection(&self, o: &IntBox) -> IntBox {
        if o.ll.x > self.ur.x || o.ll.y > self.ur.y || self.ll.x > o.ur.x || self.ll.y > o.ur.y {
            return IntBox::EMPTY;
        }
        IntBox::new(
            self.ll.x.max(o.ll.x),
            self.ll.y.max(o.ll.y),
            self.ur.x.min(o.ur.x),
            self.ur.y.min(o.ur.y),
        )
    }

    #[inline]
    pub fn intersects(&self, o: &IntBox) -> bool {
        if o.ll.x > self.ur.x || o.ll.y > self.ur.y || self.ll.x > o.ur.x {
            return false;
        }
        self.ll.y <= o.ur.y
    }

    pub fn overlaps(&self, o: &IntBox) -> bool {
        if o.ll.x >= self.ur.x || o.ll.y >= self.ur.y || self.ll.x >= o.ur.x {
            return false;
        }
        self.ll.y < o.ur.y
    }

    /// Returns true, if `other` is contained in this box.
    pub fn contains_box(&self, other: &IntBox) -> bool {
        other.is_contained_in(self)
    }

    pub fn enlarge(&self, offset: f64) -> IntOctagon {
        self.to_int_octagon().offset(offset)
    }

    pub fn translate_by(&self, v: &Vector) -> IntBox {
        if v.is_zero() {
            return *self;
        }
        let iv = v.to_int();
        IntBox::from_points(self.ll.translate_by_int(iv), self.ur.translate_by_int(iv))
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> IntBox {
        let p1 = self.ll.turn_90_degree(factor, pole);
        let p2 = self.ur.turn_90_degree(factor, pole);
        IntBox::new(p1.x.min(p2.x), p1.y.min(p2.y), p1.x.max(p2.x), p1.y.max(p2.y))
    }

    pub fn border_line(&self, no: usize) -> Line {
        match no {
            0 => Line::from_coords(0, self.ll.y, 1, self.ll.y),
            1 => Line::from_coords(self.ur.x, 0, self.ur.x, 1),
            2 => Line::from_coords(0, self.ur.y, -1, self.ur.y),
            3 => Line::from_coords(self.ll.x, 0, self.ll.x, -1),
            _ => panic!("IntBox.borderLine: no out of range"),
        }
    }

    pub fn offset(&self, dist: f64) -> IntBox {
        if dist == 0.0 || self.is_empty() {
            return *self;
        }
        let d = java_round(dist);
        IntBox::new(self.ll.x - d, self.ll.y - d, self.ur.x + d, self.ur.y + d)
    }

    pub fn horizontal_offset(&self, dist: f64) -> IntBox {
        if dist == 0.0 || self.is_empty() {
            return *self;
        }
        let d = java_round(dist);
        IntBox::new(self.ll.x - d, self.ll.y, self.ur.x + d, self.ur.y)
    }

    pub fn vertical_offset(&self, dist: f64) -> IntBox {
        if dist == 0.0 || self.is_empty() {
            return *self;
        }
        let d = java_round(dist);
        IntBox::new(self.ll.x, self.ll.y - d, self.ur.x, self.ur.y + d)
    }

    pub fn shrink(&self, width: i32) -> IntBox {
        let (llx, urx) = if 2 * width <= self.ur.x - self.ll.x {
            (self.ll.x + width, self.ur.x - width)
        } else {
            let m = (self.ll.x + self.ur.x) / 2;
            (m, m)
        };
        let (lly, ury) = if 2 * width <= self.ur.y - self.ll.y {
            (self.ll.y + width, self.ur.y - width)
        } else {
            let m = (self.ll.y + self.ur.y) / 2;
            (m, m)
        };
        IntBox::new(llx, lly, urx, ury)
    }

    pub fn compare(&self, other: &IntBox, edge_index: usize) -> Side {
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
            0 => side(self.ll.y, other.ll.y),
            1 => side(other.ur.x, self.ur.x),
            2 => side(other.ur.y, self.ur.y),
            3 => side(self.ll.x, other.ll.x),
            _ => panic!("IntBox.compare: edgeIndex out of range"),
        }
    }

    pub fn to_int_octagon(&self) -> IntOctagon {
        IntOctagon::new(
            self.ll.x,
            self.ll.y,
            self.ur.x,
            self.ur.y,
            self.ll.x - self.ur.y,
            self.ur.x - self.ll.y,
            self.ll.x + self.ll.y,
            self.ur.x + self.ur.y,
        )
    }

    pub fn to_simplex(&self) -> Simplex {
        if self.is_empty() {
            return Simplex::empty();
        }
        Simplex::new(vec![
            Line::from_point_dir(self.ll, &IntDirection::RIGHT),
            Line::from_point_dir(self.ur, &IntDirection::UP),
            Line::from_point_dir(self.ur, &IntDirection::LEFT),
            Line::from_point_dir(self.ll, &IntDirection::DOWN),
        ])
    }

    pub fn is_contained_in(&self, other: &IntBox) -> bool {
        if self.is_empty() {
            return true;
        }
        self.ll.x >= other.ll.x
            && self.ll.y >= other.ll.y
            && self.ur.x <= other.ur.x
            && self.ur.y <= other.ur.y
    }

    pub fn contains_in_interior(&self, other: &IntBox) -> bool {
        if other.is_empty() {
            return true;
        }
        other.ll.x > self.ll.x
            && other.ll.y > self.ll.y
            && other.ur.x < self.ur.x
            && other.ur.y < self.ur.y
    }

    pub fn nearest_part(&self, from: &IntBox) -> IntBox {
        let llx = if from.ll.x >= self.ll.x {
            from.ll.x
        } else {
            from.ur.x.min(self.ll.x)
        };
        let urx = if from.ur.x <= self.ur.x {
            from.ur.x
        } else {
            from.ll.x.max(self.ur.x)
        };
        let lly = if from.ll.y >= self.ll.y {
            from.ll.y
        } else {
            from.ur.y.min(self.ll.y)
        };
        let ury = if from.ur.y <= self.ur.y {
            from.ur.y
        } else {
            from.ll.y.max(self.ur.y)
        };
        IntBox::new(llx, lly, urx, ury)
    }

    pub fn divide_into_sections(&self, max_section_width: f64) -> Vec<IntBox> {
        if max_section_width <= 0.0 {
            return Vec::new();
        }
        let length = (self.ur.x - self.ll.x) as f64;
        let height = (self.ur.y - self.ll.y) as f64;
        let xcount = (length / max_section_width).ceil() as i32;
        let ycount = (height / max_section_width).ceil() as i32;
        let slx = (length / xcount as f64).ceil() as i32;
        let sly = (height / ycount as f64).ceil() as i32;
        let mut result = Vec::with_capacity((xcount.max(0) * ycount.max(0)) as usize);
        for j in 0..ycount {
            let cly = self.ll.y + j * sly;
            let cuy = if j == ycount - 1 { self.ur.y } else { cly + sly };
            for i in 0..xcount {
                let clx = self.ll.x + i * slx;
                let cux = if i == xcount - 1 { self.ur.x } else { clx + slx };
                result.push(IntBox::new(clx, cly, cux, cuy));
            }
        }
        result
    }

    /// Cuts this box out of `d`.
    pub fn cutout_from_box(&self, d: &IntBox) -> Vec<IntBox> {
        let c = self.intersection(d);
        if self.is_empty() || c.dimension() < self.dimension() {
            return vec![*d];
        }
        let mut r = [
            IntBox::new(d.ll.x, d.ll.y, c.ur.x, c.ll.y),
            IntBox::new(d.ll.x, c.ll.y, c.ll.x, d.ur.y),
            IntBox::new(c.ur.x, d.ll.y, d.ur.x, c.ur.y),
            IntBox::new(c.ll.x, c.ur.y, d.ur.x, d.ur.y),
        ];
        if c.ll.x - d.ll.x > c.ll.y - d.ll.y {
            let b = r[0];
            r[0] = IntBox::new(c.ll.x, b.ll.y, b.ur.x, b.ur.y);
            let b = r[1];
            r[1] = IntBox::new(b.ll.x, d.ll.y, b.ur.x, b.ur.y);
        }
        if d.ur.y - c.ur.y > c.ll.x - d.ll.x {
            let b = r[1];
            r[1] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, c.ur.y);
            let b = r[3];
            r[3] = IntBox::new(d.ll.x, b.ll.y, b.ur.x, b.ur.y);
        }
        if d.ur.x - c.ur.x > d.ur.y - c.ur.y {
            let b = r[2];
            r[2] = IntBox::new(b.ll.x, b.ll.y, b.ur.x, d.ur.y);
            let b = r[3];
            r[3] = IntBox::new(b.ll.x, b.ll.y, c.ur.x, b.ur.y);
        }
        if c.ll.y - d.ll.y > d.ur.x - c.ur.x {
            let b = r[0];
            r[0] = IntBox::new(b.ll.x, b.ll.y, d.ur.x, b.ur.y);
            let b = r[2];
            r[2] = IntBox::new(b.ll.x, c.ll.y, b.ur.x, b.ur.y);
        }
        r.to_vec()
    }
}
