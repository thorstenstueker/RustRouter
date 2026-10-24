//! Circles with integer center and radius.

use super::basics::java_round;
use super::{Direction, FloatPoint, IntBox, IntOctagon, IntPoint, IntVector, Line, Point, TileShape, Vector};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Circle {
    pub center: IntPoint,
    pub radius: i32,
}

impl Circle {
    pub fn new(center: IntPoint, radius: i32) -> Circle {
        if radius < 0 {
            log::warn!("Circle: unexpected negative radius");
        }
        Circle {
            center,
            radius: radius.abs(),
        }
    }

    pub fn dimension(&self) -> i32 {
        if self.radius == 0 {
            0
        } else {
            2
        }
    }

    pub fn circumference(&self) -> f64 {
        2.0 * std::f64::consts::PI * self.radius as f64
    }

    pub fn area(&self) -> f64 {
        (std::f64::consts::PI * self.radius as f64) * self.radius as f64
    }

    pub fn centre_of_gravity(&self) -> FloatPoint {
        self.center.to_float()
    }

    fn r2(&self) -> f64 {
        self.radius as f64 * self.radius as f64
    }

    pub fn is_outside(&self, p: &Point) -> bool {
        p.to_float().distance_square(&self.center.to_float()) > self.r2()
    }

    pub fn contains(&self, p: &Point) -> bool {
        !self.is_outside(p)
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        p.distance_square(&self.center.to_float()) <= self.r2()
    }

    pub fn contains_inside(&self, p: &Point) -> bool {
        p.to_float().distance_square(&self.center.to_float()) < self.r2()
    }

    pub fn contains_on_border(&self, p: &Point) -> bool {
        p.to_float().distance_square(&self.center.to_float()) == self.r2()
    }

    pub fn distance(&self, p: &FloatPoint) -> f64 {
        (p.distance(&self.center.to_float()) - self.radius as f64).max(0.0)
    }

    pub fn smallest_radius(&self) -> f64 {
        self.radius as f64
    }

    pub fn bounding_box(&self) -> IntBox {
        IntBox::new(
            self.center.x - self.radius,
            self.center.y - self.radius,
            self.center.x + self.radius,
            self.center.y + self.radius,
        )
    }

    pub fn bounding_octagon(&self) -> IntOctagon {
        let lx = self.center.x - self.radius;
        let rx = self.center.x + self.radius;
        let ly = self.center.y - self.radius;
        let uy = self.center.y + self.radius;
        let s = std::f64::consts::SQRT_2 - 1.0;
        let ceil_v = (s * self.radius as f64).ceil() as i32;
        let floor_v = (s * self.radius as f64).floor() as i32;
        let ulx = lx - (self.center.y + floor_v);
        let lrx = rx - (self.center.y - ceil_v);
        let llx = lx + (self.center.y - floor_v);
        let urx = rx + (self.center.y + ceil_v);
        IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx)
    }

    pub fn bounding_tile(&self) -> TileShape {
        TileShape::Octagon(self.bounding_octagon())
    }

    pub fn bounding_tile_max_segment(&self, max_segment_length: i32) -> TileShape {
        if max_segment_length <= 0 {
            return self.bounding_tile();
        }
        self.bounding_tile_with_divisions(self.radius / max_segment_length + 1)
    }

    pub fn bounding_tile_with_divisions(&self, quadrant_division_count: i32) -> TileShape {
        if self.radius <= 0 || quadrant_division_count <= 2 {
            return self.bounding_tile();
        }
        let d = quadrant_division_count as usize;
        let mut tangents = vec![Line::from_coords(0, 0, 1, 0); d * 4];
        for i in 0..d {
            let border_delta = if i == 0 {
                IntVector::new(self.radius, 0)
            } else {
                let a = i as f64 * std::f64::consts::PI / (2.0 * d as f64);
                IntVector::new(
                    (a.cos() * self.radius as f64).ceil() as i32,
                    (a.sin() * self.radius as f64).ceil() as i32,
                )
            };
            let current_a = self.center.translate_by_int(border_delta);
            let dir = if i == 0 {
                Direction::UP
            } else {
                let a = i as f64 * std::f64::consts::PI / (2.0 * d as f64);
                Direction::get_instance_approx(a + std::f64::consts::PI / 2.0)
            };
            let tangent = Line::from_point_direction(&Point::Int(current_a), &dir);
            tangents[d + i] = tangent;
            tangents[2 * d + i] = tangent.turn_90_degree(1, &self.center);
            tangents[3 * d + i] = tangent.turn_90_degree(2, &self.center);
            tangents[i] = tangent.turn_90_degree(3, &self.center);
        }
        TileShape::from_lines(&tangents)
    }

    pub fn is_contained_in(&self, b: &IntBox) -> bool {
        b.ll.x <= self.center.x - self.radius
            && b.ll.y <= self.center.y - self.radius
            && b.ur.x >= self.center.x + self.radius
            && b.ur.y >= self.center.y + self.radius
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> Circle {
        Circle::new(self.center.turn_90_degree(factor, pole), self.radius)
    }

    pub fn rotate_approx(&self, angle: f64, pole: &FloatPoint) -> Circle {
        Circle::new(self.center.to_float().rotate(angle, pole).round(), self.radius)
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> Circle {
        Circle::new(self.center.mirror_vertical(pole), self.radius)
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> Circle {
        Circle::new(self.center.mirror_horizontal(pole), self.radius)
    }

    pub fn max_width(&self) -> f64 {
        2.0 * self.radius as f64
    }

    pub fn min_width(&self) -> f64 {
        2.0 * self.radius as f64
    }

    pub fn offset(&self, offset: f64) -> Circle {
        Circle::new(self.center, java_round(self.radius as f64 + offset))
    }

    pub fn shrink(&self, offset: f64) -> Circle {
        Circle::new(self.center, java_round(self.radius as f64 - offset).max(1))
    }

    pub fn translate_by(&self, v: &Vector) -> Circle {
        match v {
            Vector::Int(iv) => {
                if iv.is_zero() {
                    return *self;
                }
                Circle::new(self.center.translate_by_int(*iv), self.radius)
            }
            _ => {
                log::warn!("Circle.translate_by only implemented for IntVectors till now");
                *self
            }
        }
    }

    pub fn border_distance(&self, p: &FloatPoint) -> f64 {
        (p.distance(&self.center.to_float()) - self.radius as f64).abs()
    }

    pub fn enlarge(&self, offset: f64) -> Circle {
        if offset == 0.0 {
            return *self;
        }
        Circle::new(self.center, self.radius + java_round(offset))
    }

    pub fn intersects_circle(&self, other: &Circle) -> bool {
        let mut rs = (self.radius + other.radius) as f64;
        rs *= rs;
        self.center.distance_square(&other.center) <= rs
    }

    pub fn intersects_tile(&self, t: &TileShape) -> bool {
        t.distance(&self.center.to_float()) <= self.radius as f64
    }

    pub fn split_to_convex(&self) -> Vec<TileShape> {
        if self.radius >= 16 {
            vec![self.bounding_tile_with_divisions(16)]
        } else {
            vec![self.bounding_tile()]
        }
    }
}
