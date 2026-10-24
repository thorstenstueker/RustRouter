//! Floating point approximations of points and lines.

use super::basics::{java_round, rint, Side, CRIT_INT};
use super::{Direction, IntBox, IntOctagon, IntPoint, Line};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct FloatPoint {
    pub x: f64,
    pub y: f64,
}

impl fmt::Display for FloatPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:.4} , {:.4})", self.x, self.y)
    }
}

impl From<IntPoint> for FloatPoint {
    fn from(p: IntPoint) -> FloatPoint {
        p.to_float()
    }
}

impl FloatPoint {
    pub const ZERO: FloatPoint = FloatPoint { x: 0.0, y: 0.0 };

    #[inline]
    pub const fn new(x: f64, y: f64) -> FloatPoint {
        FloatPoint { x, y }
    }

    pub fn bounding_octagon(points: &[FloatPoint]) -> IntOctagon {
        let mut min_x = i32::MAX as f64;
        let mut min_y = i32::MAX as f64;
        let mut max_x = i32::MIN as f64;
        let mut max_y = i32::MIN as f64;
        let mut min_ul = i32::MAX as f64;
        let mut max_lr = i32::MIN as f64;
        let mut min_ll = i32::MAX as f64;
        let mut max_ur = i32::MIN as f64;
        for c in points {
            min_x = min_x.min(c.x);
            min_y = min_y.min(c.y);
            max_x = max_x.max(c.x);
            max_y = max_y.max(c.y);
            let tmp = c.x - c.y;
            min_ul = min_ul.min(tmp);
            max_lr = max_lr.max(tmp);
            let tmp = c.x + c.y;
            min_ll = min_ll.min(tmp);
            max_ur = max_ur.max(tmp);
        }
        IntOctagon::new(
            min_x.floor() as i32,
            min_y.floor() as i32,
            max_x.ceil() as i32,
            max_y.ceil() as i32,
            min_ul.floor() as i32,
            max_lr.ceil() as i32,
            min_ll.floor() as i32,
            max_ur.ceil() as i32,
        )
    }

    #[inline]
    pub fn size_square(&self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    #[inline]
    pub fn size(&self) -> f64 {
        self.size_square().sqrt()
    }

    #[inline]
    pub fn distance_square(&self, other: &FloatPoint) -> f64 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        dx * dx + dy * dy
    }

    #[inline]
    pub fn distance(&self, other: &FloatPoint) -> f64 {
        self.distance_square(other).sqrt()
    }

    pub fn weighted_distance(&self, other: &FloatPoint, hw: f64, vw: f64) -> f64 {
        let dx = (self.x - other.x) * hw;
        let dy = (self.y - other.y) * vw;
        (dx * dx + dy * dy).sqrt()
    }

    #[inline]
    pub fn round(&self) -> IntPoint {
        IntPoint::new(java_round(self.x), java_round(self.y))
    }

    pub fn round_to_the_right(&self, dir: &Direction) -> IntPoint {
        let v = dir.get_vector().to_float();
        let rx = if v.y > 0.0 {
            self.x.ceil() as i32
        } else if v.y < 0.0 {
            self.x.floor() as i32
        } else {
            java_round(self.x)
        };
        let ry = if v.x > 0.0 {
            self.y.floor() as i32
        } else if v.x < 0.0 {
            self.y.ceil() as i32
        } else {
            java_round(self.y)
        };
        IntPoint::new(rx, ry)
    }

    pub fn round_to_grid(&self, hg: i32, vg: i32) -> IntPoint {
        let rx = if hg > 0 {
            rint(self.x / hg as f64) * hg as f64
        } else {
            self.x
        };
        let ry = if vg > 0 {
            rint(self.y / vg as f64) * vg as f64
        } else {
            self.y
        };
        IntPoint::new(rx as i32, ry as i32)
    }

    pub fn round_to_the_left(&self, dir: &Direction) -> IntPoint {
        let v = dir.get_vector().to_float();
        let rx = if v.y > 0.0 {
            self.x.floor() as i32
        } else if v.y < 0.0 {
            self.x.ceil() as i32
        } else {
            java_round(self.x)
        };
        let ry = if v.x > 0.0 {
            self.y.ceil() as i32
        } else if v.x < 0.0 {
            self.y.floor() as i32
        } else {
            java_round(self.y)
        };
        IntPoint::new(rx, ry)
    }

    #[inline]
    pub fn add(&self, o: &FloatPoint) -> FloatPoint {
        FloatPoint::new(self.x + o.x, self.y + o.y)
    }

    #[inline]
    pub fn subtract(&self, o: &FloatPoint) -> FloatPoint {
        FloatPoint::new(self.x - o.x, self.y - o.y)
    }

    pub fn projection_approx(&self, line: &Line) -> FloatPoint {
        FloatLine::new(line.a.to_float(), line.b.to_float()).perpendicular_projection(self)
    }

    /// Scalar product of the vectors `p1 - self` and `p2 - self`.
    pub fn scalar_product(&self, p1: &FloatPoint, p2: &FloatPoint) -> f64 {
        let dx1 = p1.x - self.x;
        let dx2 = p2.x - self.x;
        let dy1 = p1.y - self.y;
        let dy2 = p2.y - self.y;
        dx1 * dx2 + dy1 * dy2
    }

    pub fn change_size(&self, new_size: f64) -> FloatPoint {
        if self.x == 0.0 && self.y == 0.0 {
            return *self;
        }
        let length = (self.x * self.x + self.y * self.y).sqrt();
        FloatPoint::new((self.x * new_size) / length, (self.y * new_size) / length)
    }

    pub fn change_length(&self, to: &FloatPoint, new_length: f64) -> FloatPoint {
        let dx = to.x - self.x;
        let dy = to.y - self.y;
        if dx == 0.0 && dy == 0.0 {
            return *to;
        }
        let length = (dx * dx + dy * dy).sqrt();
        FloatPoint::new(
            self.x + (dx * new_length) / length,
            self.y + (dy * new_length) / length,
        )
    }

    pub fn middle_point(&self, to: &FloatPoint) -> FloatPoint {
        FloatPoint::new(0.5 * (self.x + to.x), 0.5 * (self.y + to.y))
    }

    pub fn side_of(&self, p1: &FloatPoint, p2: &FloatPoint) -> Side {
        let d21x = p2.x - p1.x;
        let d21y = p2.y - p1.y;
        let d01x = self.x - p1.x;
        let d01y = self.y - p1.y;
        Side::of(d21x * d01y - d21y * d01x)
    }

    pub fn rotate(&self, angle: f64, pole: &FloatPoint) -> FloatPoint {
        if angle == 0.0 {
            return *self;
        }
        let dx = self.x - pole.x;
        let dy = self.y - pole.y;
        let (s, c) = angle.sin_cos();
        FloatPoint::new(pole.x + dx * c - dy * s, pole.y + dx * s + dy * c)
    }

    pub fn turn_90_degree(&self, factor: i32) -> FloatPoint {
        match factor.rem_euclid(4) {
            0 => *self,
            1 => FloatPoint::new(-self.y, self.x),
            2 => FloatPoint::new(-self.x, -self.y),
            _ => FloatPoint::new(self.y, -self.x),
        }
    }

    pub fn turn_90_degree_around(&self, factor: i32, pole: &FloatPoint) -> FloatPoint {
        pole.add(&self.subtract(pole).turn_90_degree(factor))
    }

    pub fn is_contained_in_box(&self, p1: &FloatPoint, p2: &FloatPoint, tolerance: f64) -> bool {
        let (min_x, max_x) = if p1.x < p2.x { (p1.x, p2.x) } else { (p2.x, p1.x) };
        if self.x < min_x - tolerance || self.x > max_x + tolerance {
            return false;
        }
        let (min_y, max_y) = if p1.y < p2.y { (p1.y, p2.y) } else { (p2.y, p1.y) };
        self.y >= min_y - tolerance && self.y <= max_y + tolerance
    }

    pub fn bounding_box(&self) -> IntBox {
        IntBox::new(
            self.x.floor() as i32,
            self.y.floor() as i32,
            self.x.ceil() as i32,
            self.y.ceil() as i32,
        )
    }

    /// Returns the tangential points from this point to the circle around `to` with radius `distance`.
    pub fn tangential_points(&self, to: &FloatPoint, distance: f64) -> Vec<FloatPoint> {
        let dx = (self.x - to.x).abs();
        let dy = (self.y - to.y).abs();
        let turned = dy > dx;
        let (pole, center) = if turned {
            (
                FloatPoint::new(-self.y, self.x),
                FloatPoint::new(-to.y, to.x),
            )
        } else {
            (*self, *to)
        };
        let dx = pole.x - center.x;
        let dy = pole.y - center.y;
        let dx_sq = dx * dx;
        let dy_sq = dy * dy;
        let dist_sq = dx_sq + dy_sq;
        let radius_sq = distance * distance;
        let discriminant = radius_sq * dy_sq - (radius_sq - dx_sq) * dist_sq;
        if discriminant <= 0.0 {
            return Vec::new();
        }
        let sq = discriminant.sqrt();
        let a1 = radius_sq * dy;
        let dy1 = (a1 + distance * sq) / dist_sq;
        let dy2 = (a1 - distance * sq) / dist_sq;
        let p1y = dy1 + center.y;
        let p1x = (radius_sq - dy * dy1) / dx + center.x;
        let p2y = dy2 + center.y;
        let p2x = (radius_sq - dy * dy2) / dx + center.x;
        if turned {
            vec![FloatPoint::new(p1y, -p1x), FloatPoint::new(p2y, -p2x)]
        } else {
            vec![FloatPoint::new(p1x, p1y), FloatPoint::new(p2x, p2y)]
        }
    }

    pub fn left_tangential_point(&self, to: &FloatPoint, distance: f64) -> Option<FloatPoint> {
        let t = self.tangential_points(to, distance);
        if t.len() < 2 {
            return None;
        }
        Some(if to.side_of(self, &t[0]) == Side::OnTheRight {
            t[0]
        } else {
            t[1]
        })
    }

    pub fn right_tangential_point(&self, to: &FloatPoint, distance: f64) -> Option<FloatPoint> {
        let t = self.tangential_points(to, distance);
        if t.len() < 2 {
            return None;
        }
        Some(if to.side_of(self, &t[0]) == Side::OnTheLeft {
            t[0]
        } else {
            t[1]
        })
    }

    pub fn circle_center(&self, p1: &FloatPoint, p2: &FloatPoint) -> FloatPoint {
        let slope1 = (p1.y - self.y) / (p1.x - self.x);
        let slope2 = (p2.y - p1.y) / (p2.x - p1.x);
        let cx = (slope1 * slope2 * (self.y - p2.y) + slope2 * (self.x + p1.x)
            - slope1 * (p1.x + p2.x))
            / (2.0 * (slope2 - slope1));
        let cy = (0.5 * (self.x + p1.x) - cx) / slope1 + 0.5 * (self.y + p1.y);
        FloatPoint::new(cx, cy)
    }

    pub fn inside_circle(&self, p1: &FloatPoint, p2: &FloatPoint, p3: &FloatPoint) -> bool {
        let center = p1.circle_center(p2, p3);
        let radius_sq = center.distance_square(p1);
        self.distance_square(&center) < radius_sq - 1.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatLine {
    pub a: FloatPoint,
    pub b: FloatPoint,
}

impl FloatLine {
    #[inline]
    pub const fn new(a: FloatPoint, b: FloatPoint) -> FloatLine {
        FloatLine { a, b }
    }

    pub fn opposite(&self) -> FloatLine {
        FloatLine::new(self.b, self.a)
    }

    pub fn adjust_direction(&self, other: &FloatLine) -> FloatLine {
        if self.b.side_of(&self.a, &other.a) == other.b.side_of(&self.a, &other.a) {
            return *self;
        }
        self.opposite()
    }

    pub fn intersection(&self, other: &FloatLine) -> Option<FloatPoint> {
        let d1x = self.b.x - self.a.x;
        let d1y = self.b.y - self.a.y;
        let d2x = other.b.x - other.a.x;
        let d2y = other.b.y - other.a.y;
        let det1 = self.a.x * self.b.y - self.a.y * self.b.x;
        let det2 = other.a.x * other.b.y - other.a.y * other.b.x;
        let det = d2x * d1y - d2y * d1x;
        if det == 0.0 {
            return None;
        }
        Some(FloatPoint::new(
            (d2x * det1 - d1x * det2) / det,
            (d2y * det1 - d1y * det2) / det,
        ))
    }

    pub fn translate(&self, dist: f64) -> FloatLine {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let dxdx = dx * dx;
        let dydy = dy * dy;
        let length = (dxdx + dydy).sqrt();
        let new_a = if dxdx <= dydy {
            let rel_x = (dist * length) / dy;
            FloatPoint::new(self.a.x - rel_x, self.a.y)
        } else {
            let rel_y = (dist * length) / dx;
            FloatPoint::new(self.a.x, self.a.y + rel_y)
        };
        let new_b = FloatPoint::new(new_a.x + dx, new_a.y + dy);
        FloatLine::new(new_a, new_b)
    }

    pub fn signed_distance(&self, p: &FloatPoint) -> f64 {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let det = dy * (p.x - self.a.x) - dx * (p.y - self.a.y);
        det / (dx * dx + dy * dy).sqrt()
    }

    pub fn perpendicular_projection(&self, p: &FloatPoint) -> FloatPoint {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        if dx == 0.0 && dy == 0.0 {
            return self.a;
        }
        let dxdx = dx * dx;
        let dydy = dy * dy;
        let dxdy = dx * dy;
        let denominator = dxdx + dydy;
        let det = self.a.x * self.b.y - self.b.x * self.a.y;
        FloatPoint::new(
            (p.x * dxdx + p.y * dxdy + det * dy) / denominator,
            (p.x * dxdy + p.y * dydy - det * dx) / denominator,
        )
    }

    pub fn segment_distance(&self, p: &FloatPoint) -> f64 {
        let projection = self.perpendicular_projection(p);
        if projection.is_contained_in_box(&self.a, &self.b, 0.01) {
            p.distance(&projection)
        } else {
            p.distance(&self.a).min(p.distance(&self.b))
        }
    }

    pub fn segment_projection(&self, seg: &FloatLine) -> Option<FloatLine> {
        if self.b.scalar_product(&self.a, &seg.a) < 0.0 {
            return None;
        }
        if self.a.scalar_product(&self.b, &seg.b) < 0.0 {
            return None;
        }
        let crit = CRIT_INT as f64;
        let pa = if self.a.scalar_product(&self.b, &seg.a) < 0.0 {
            self.a
        } else {
            let pa = self.perpendicular_projection(&seg.a);
            if pa.x.abs() >= crit || pa.y.abs() >= crit {
                return None;
            }
            pa
        };
        let pb = if self.b.scalar_product(&self.a, &seg.b) < 0.0 {
            self.b
        } else {
            self.perpendicular_projection(&seg.b)
        };
        if pb.x.abs() >= crit || pb.y.abs() >= crit {
            return None;
        }
        Some(FloatLine::new(pa, pb))
    }

    pub fn segment_projection_2(&self, seg: &FloatLine) -> Option<FloatLine> {
        if seg.a.scalar_product(&seg.b, &self.b) <= 0.0 {
            return None;
        }
        if seg.b.scalar_product(&seg.a, &self.a) <= 0.0 {
            return None;
        }
        let crit = CRIT_INT as f64;
        let pa = if seg.a.scalar_product(&seg.b, &self.a) < 0.0 {
            let perp = FloatLine::new(seg.a, seg.b.turn_90_degree_around(1, &seg.a));
            match perp.intersection(self) {
                Some(p) if p.x.abs() < crit && p.y.abs() < crit => p,
                _ => return None,
            }
        } else {
            self.a
        };
        let pb = if seg.b.scalar_product(&seg.a, &self.b) < 0.0 {
            let perp = FloatLine::new(seg.b, seg.a.turn_90_degree_around(1, &seg.b));
            match perp.intersection(self) {
                Some(p) if p.x.abs() < crit && p.y.abs() < crit => p,
                _ => return None,
            }
        } else {
            self.b
        };
        Some(FloatLine::new(pa, pb))
    }

    pub fn shrink_segment(&self, offset: f64) -> FloatLine {
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        if dx == 0.0 && dy == 0.0 {
            return *self;
        }
        let length = (dx * dx + dy * dy).sqrt();
        let off = offset.min(length / 2.0);
        let new_a = FloatPoint::new(self.a.x + (dx * off) / length, self.a.y + (dy * off) / length);
        let nl = length - off;
        let new_b = FloatPoint::new(self.a.x + (dx * nl) / length, self.a.y + (dy * nl) / length);
        FloatLine::new(new_a, new_b)
    }

    pub fn nearest_segment_point(&self, from: &FloatPoint) -> FloatPoint {
        let projection = self.perpendicular_projection(from);
        if projection.is_contained_in_box(&self.a, &self.b, 0.01) {
            return projection;
        }
        if from.distance_square(&self.a) <= from.distance_square(&self.b) {
            self.a
        } else {
            self.b
        }
    }

    pub fn divide_segment_into_sections(&self, count: usize) -> Vec<FloatLine> {
        if count == 0 {
            return Vec::new();
        }
        if count == 1 {
            return vec![*self];
        }
        let line_length = self.b.distance(&self.a);
        let section = line_length / count as f64;
        let dx = self.b.x - self.a.x;
        let dy = self.b.y - self.a.y;
        let mut result = Vec::with_capacity(count);
        let mut cur_a = self.a;
        for i in 0..count {
            let cur_b = if i == count - 1 {
                self.b
            } else {
                let d = (i + 1) as f64 * section;
                FloatPoint::new(
                    self.a.x + (dx * d) / line_length,
                    self.a.y + (dy * d) / line_length,
                )
            };
            result.push(FloatLine::new(cur_a, cur_b));
            cur_a = cur_b;
        }
        result
    }
}
