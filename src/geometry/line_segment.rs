//! Line segments given by a middle line cut off by a start and an end line.

use super::basics::{java_round, Side, Signum};
use super::{FloatPoint, IntBox, IntOctagon, IntPoint, IntVector, Line, Point, Polyline, Simplex, TileShape};
use std::cmp::Ordering;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct LineSegment {
    start: Line,
    middle: Line,
    end: Line,
    start_point: OnceLock<Point>,
    end_point: OnceLock<Point>,
}

impl LineSegment {
    pub fn new(start: Line, middle: Line, end: Line) -> LineSegment {
        LineSegment {
            start,
            middle,
            end,
            start_point: OnceLock::new(),
            end_point: OnceLock::new(),
        }
    }

    /// The segment number `no` of the polyline (`0 < no < lines.len() - 1`).
    pub fn from_polyline(polyline: &Polyline, no: usize) -> LineSegment {
        let lines = polyline.lines();
        if no == 0 || no + 1 >= lines.len() {
            log::warn!("LineSegment from Polyline: no out of range");
            let l = Line::from_coords(0, 0, 1, 0);
            return LineSegment::new(l, l, l);
        }
        LineSegment::new(lines[no - 1], lines[no], lines[no + 1])
    }

    /// The border segment number `no` of the tile shape.
    pub fn from_tile(shape: &TileShape, no: usize) -> LineSegment {
        let n = shape.border_line_count();
        let start = if no == 0 {
            shape.border_line(n - 1)
        } else {
            shape.border_line(no - 1)
        };
        let middle = shape.border_line(no);
        let end = if no == n - 1 {
            shape.border_line(0)
        } else {
            shape.border_line(no + 1)
        };
        LineSegment::new(start, middle, end)
    }

    pub fn start_point(&self) -> Point {
        self.start_point
            .get_or_init(|| self.middle.intersection(&self.start))
            .clone()
    }

    pub fn end_point(&self) -> Point {
        self.end_point
            .get_or_init(|| self.middle.intersection(&self.end))
            .clone()
    }

    pub fn start_point_approx(&self) -> FloatPoint {
        match self.start_point.get() {
            Some(p) => p.to_float(),
            None => self.start.intersection_approx(&self.middle),
        }
    }

    pub fn end_point_approx(&self) -> FloatPoint {
        match self.end_point.get() {
            Some(p) => p.to_float(),
            None => self.end.intersection_approx(&self.middle),
        }
    }

    #[inline]
    pub fn get_line(&self) -> Line {
        self.middle
    }

    #[inline]
    pub fn get_start_closing_line(&self) -> Line {
        self.start
    }

    #[inline]
    pub fn get_end_closing_line(&self) -> Line {
        self.end
    }

    pub fn opposite(&self) -> LineSegment {
        LineSegment::new(self.end.opposite(), self.middle.opposite(), self.start.opposite())
    }

    pub fn to_polyline(&self) -> Polyline {
        Polyline::from_lines(&[self.start, self.middle, self.end])
    }

    pub fn to_simplex(&self) -> Simplex {
        let l0 = if self.end_point().side_of_line(&self.start) == Side::OnTheRight {
            self.start.opposite()
        } else {
            self.start
        };
        let l3 = if self.start_point().side_of_line(&self.end) == Side::OnTheRight {
            self.end.opposite()
        } else {
            self.end
        };
        Simplex::get_instance(&[l0, self.middle, self.middle.opposite(), l3])
    }

    pub fn contains(&self, p: &Point) -> bool {
        let Point::Int(ip) = p else {
            log::warn!("LineSegments.contains currently only implemented for IntPoints");
            return false;
        };
        if self.middle.side_of(p) != Side::Collinear {
            return false;
        }
        let perp = self.middle.direction().turn_45_degree(2);
        let pl = Line::from_point_dir(*ip, &perp);
        let ss = pl.side_of(&self.start_point());
        let es = pl.side_of(&self.end_point());
        ss != es || ss == Side::Collinear
    }

    pub fn bounding_box(&self) -> IntBox {
        let sc = self.middle.intersection_approx(&self.start);
        let ec = self.middle.intersection_approx(&self.end);
        IntBox::new(
            sc.x.min(ec.x).floor() as i32,
            sc.y.min(ec.y).floor() as i32,
            sc.x.max(ec.x).ceil() as i32,
            sc.y.max(ec.y).ceil() as i32,
        )
    }

    pub fn bounding_octagon(&self) -> IntOctagon {
        let sc = self.middle.intersection_approx(&self.start);
        let ec = self.middle.intersection_approx(&self.end);
        let lx = sc.x.min(ec.x).floor();
        let ly = sc.y.min(ec.y).floor();
        let rx = sc.x.max(ec.x).ceil();
        let uy = sc.y.max(ec.y).ceil();
        let s1 = sc.x - sc.y;
        let e1 = ec.x - ec.y;
        let ulx = s1.min(e1).floor();
        let lrx = s1.max(e1).ceil();
        let s2 = sc.x + sc.y;
        let e2 = ec.x + ec.y;
        let llx = s2.min(e2).floor();
        let urx = s2.max(e2).ceil();
        IntOctagon::new(
            lx as i32, ly as i32, rx as i32, uy as i32, ulx as i32, lrx as i32, llx as i32, urx as i32,
        )
        .normalize()
    }

    pub fn change_length_approx(&self, new_length: f64) -> LineSegment {
        let new_end = self
            .start_point_approx()
            .change_length(&self.end_point_approx(), new_length);
        let perp = self.middle.direction().turn_45_degree(2);
        let new_end_line = Line::from_point_dir(new_end.round(), &perp);
        LineSegment::new(self.start, self.middle, new_end_line)
    }

    /// Returns the lines of `other` cutting this segment: empty, one line for a crossing,
    /// two lines (the closing lines of the overlap) for overlapping segments.
    pub fn intersection(&self, other: &LineSegment) -> Vec<Line> {
        if !self.bounding_box().intersects(&other.bounding_box()) {
            return Vec::new();
        }
        let sps = self.start_point().side_of_line(&other.middle);
        let eps = self.end_point().side_of_line(&other.middle);
        if sps == Side::Collinear && eps == Side::Collinear {
            let ts = self.sort_endpoints_in_xy();
            let os = other.sort_endpoints_in_xy();
            let (left, right) = if ts.start_point().compare_xy(&os.start_point()) != Ordering::Greater {
                (ts, os)
            } else {
                (os, ts)
            };
            let cmp = left.end_point().compare_xy(&right.start_point());
            if cmp == Ordering::Less {
                return Vec::new();
            }
            if cmp == Ordering::Equal {
                return vec![left.end];
            }
            let second = if right.end_point().compare_xy(&left.end_point()) != Ordering::Less {
                left.end
            } else {
                right.end
            };
            return vec![right.start, second];
        }
        if sps == eps
            || other.start_point().side_of_line(&self.middle) == other.end_point().side_of_line(&self.middle)
        {
            return Vec::new();
        }
        vec![other.middle]
    }

    pub fn intersects(&self, other: &LineSegment) -> bool {
        !self.intersection(other).is_empty()
    }

    pub fn overlaps(&self, other: &LineSegment) -> bool {
        self.intersection(other).len() > 1
    }

    pub fn stair_approximation(&self, width: f64, to_the_right: bool) -> Vec<IntPoint> {
        let sp = self.start_point().to_float().round();
        let ep = self.end_point().to_float().round();
        if sp == ep {
            return Vec::new();
        }
        if sp.x == ep.x || sp.y == ep.y {
            return vec![sp, ep];
        }
        let dx = ep.x - sp.x;
        let dy = ep.y - sp.y;
        let adx = dx.abs();
        let ady = dy.abs();
        let function_of_x = adx >= ady;
        let (mut stair_width, stair_count);
        if function_of_x {
            stair_width = 1.max(java_round((width * adx as f64) / ady as f64));
            stair_count = (adx - 1) / stair_width + 1;
            if ep.x < sp.x {
                stair_width = -stair_width;
            }
        } else {
            stair_width = 1.max(java_round((width * ady as f64) / adx as f64));
            stair_count = (ady - 1) / stair_width + 1;
            if ep.y < sp.y {
                stair_width = -stair_width;
            }
        }
        let mut result = Vec::with_capacity((2 * stair_count + 1) as usize);
        result.push(sp);
        let det = dx as f64 * dy as f64;
        let change_x_first = to_the_right && det > 0.0 || !to_the_right && det < 0.0;
        let mut px = sp.x;
        let mut py = sp.y;
        for i in 1..stair_count {
            let (cx, cy);
            if function_of_x {
                cx = sp.x + i * stair_width;
                cy = java_round(self.get_line().function_value_approx(cx as f64));
            } else {
                cy = sp.y + i * stair_width;
                cx = java_round(self.get_line().function_in_y_value_approx(cy as f64));
            }
            if change_x_first {
                result.push(IntPoint::new(cx, py));
            } else {
                result.push(IntPoint::new(px, cy));
            }
            result.push(IntPoint::new(cx, cy));
            px = cx;
            py = cy;
        }
        if change_x_first {
            result.push(IntPoint::new(ep.x, py));
        } else {
            result.push(IntPoint::new(px, ep.y));
        }
        result.push(ep);
        result
    }

    pub fn stair_approximation_45(&self, width: f64, to_the_right: bool) -> Vec<IntPoint> {
        let sp = self.start_point().to_float().round();
        let ep = self.end_point().to_float().round();
        if sp == ep {
            return Vec::new();
        }
        let delta = ep.difference_by(&sp);
        if delta.is_orthogonal() || delta.is_diagonal() {
            return vec![sp, ep];
        }
        let ad = IntVector::new(delta.x.abs(), delta.y.abs());
        let function_of_x = ad.x >= ad.y;
        let det = delta.x as f64 * delta.y as f64;
        let (mut stair_width, stair_count);
        if function_of_x {
            stair_width = 1.max(java_round((width * ad.x as f64) / ad.y as f64));
            stair_count = (ad.x - 1) / stair_width + 1;
            if ep.x < sp.x {
                stair_width = -stair_width;
            }
        } else {
            stair_width = 1.max(java_round((width * ad.y as f64) / ad.x as f64));
            stair_count = (ad.x - 1) / stair_width + 1;
            if ep.y < sp.y {
                stair_width = -stair_width;
            }
        }
        let mut result = Vec::with_capacity((2 * stair_count.max(0) + 1) as usize);
        result.push(sp);
        let mut prev = sp;
        let sw_sign = Signum::as_int(stair_width as f64);
        for i in 1..=stair_count {
            let cur = if i == stair_count {
                ep
            } else if function_of_x {
                let cx = sp.x + i * stair_width;
                IntPoint::new(cx, java_round(self.get_line().function_value_approx(cx as f64)))
            } else {
                let cy = sp.y + i * stair_width;
                IntPoint::new(java_round(self.get_line().function_in_y_value_approx(cy as f64)), cy)
            };
            let (cx, cy);
            if function_of_x {
                let diagonal_first = to_the_right && det < 0.0 || !to_the_right && det > 0.0;
                if diagonal_first {
                    cx = prev.x + sw_sign * (cur.y - prev.y).abs();
                    cy = cur.y;
                } else {
                    cx = cur.x - sw_sign * (cur.y - prev.y).abs();
                    cy = prev.y;
                }
            } else {
                let diagonal_first = to_the_right && det > 0.0 || !to_the_right && det < 0.0;
                if diagonal_first {
                    cx = cur.x;
                    cy = prev.y + sw_sign * (cur.x - prev.x).abs();
                } else {
                    cx = prev.x;
                    cy = cur.y - sw_sign * (cur.x - prev.x).abs();
                }
            }
            result.push(IntPoint::new(cx, cy));
            result.push(cur);
            prev = cur;
        }
        result
    }

    /// Returns the indices of the border lines of `shape` intersected by this segment,
    /// sorted by distance from the start point.
    pub fn border_intersections(&self, shape: &TileShape) -> Vec<usize> {
        if !self.bounding_box().intersects(&shape.bounding_box()) {
            return Vec::new();
        }
        let edge_count = shape.border_line_count();
        let mut prev_line = shape.border_line(edge_count - 1);
        let mut current_line = shape.border_line(0);
        let mut result: [usize; 2] = [0; 2];
        let mut intersection: [Option<Point>; 2] = [None, None];
        let mut count = 0usize;
        let line_start = self.start_point();
        let line_end = self.end_point();
        for edge in 0..edge_count {
            let next_line = if edge == edge_count - 1 {
                shape.border_line(0)
            } else {
                shape.border_line(edge + 1)
            };
            let sps = current_line.side_of(&line_start);
            let eps = current_line.side_of(&line_end);
            if sps == Side::OnTheLeft && eps == Side::OnTheLeft {
                return Vec::new();
            }
            if sps == Side::Collinear && eps != Side::OnTheRight {
                return Vec::new();
            }
            if eps == Side::Collinear && sps != Side::OnTheRight {
                return Vec::new();
            }
            if sps != Side::OnTheRight || eps != Side::OnTheRight {
                let is = self.middle.intersection(&current_line);
                let pls = prev_line.side_of(&is);
                let nls = next_line.side_of(&is);
                if pls != Side::OnTheLeft && nls != Side::OnTheLeft {
                    if pls == Side::Collinear {
                        let ppc = if edge == 0 {
                            shape.corner(edge_count - 1)
                        } else {
                            shape.corner(edge - 1)
                        };
                        let nc = if edge == edge_count - 1 {
                            shape.corner(0)
                        } else {
                            shape.corner(edge + 1)
                        };
                        let ppcs = self.middle.side_of(&ppc);
                        let ncs = self.middle.side_of(&nc);
                        if ppcs == Side::Collinear || ncs == Side::Collinear || ppcs == ncs {
                            return Vec::new();
                        }
                    }
                    if nls == Side::Collinear {
                        let pc = shape.corner(edge);
                        let nnc = if edge + 2 == edge_count {
                            shape.corner(0)
                        } else if edge + 1 == edge_count {
                            shape.corner(1)
                        } else {
                            shape.corner(edge + 2)
                        };
                        let pcs = self.middle.side_of(&pc);
                        let nncs = self.middle.side_of(&nnc);
                        if pcs == Side::Collinear || nncs == Side::Collinear || pcs == nncs {
                            return Vec::new();
                        }
                    }
                    let handled = intersection[..count].iter().any(|p| p.as_ref() == Some(&is));
                    if !handled {
                        if count < 2 {
                            result[count] = edge;
                            intersection[count] = Some(is);
                            count += 1;
                        } else {
                            log::warn!("border_intersections: intersection_count ({count}) is too big!");
                        }
                    }
                }
            }
            prev_line = current_line;
            current_line = next_line;
        }
        if count == 0 {
            return Vec::new();
        }
        if count == 2 {
            let is0 = intersection[0].as_ref().unwrap().to_float();
            let is1 = intersection[1].as_ref().unwrap().to_float();
            let cs = line_start.to_float();
            if cs.distance_square(&is1) < cs.distance_square(&is0) {
                result.swap(0, 1);
            }
            return result.to_vec();
        }
        vec![result[0]]
    }

    pub fn sort_endpoints_in_xy(&self) -> LineSegment {
        if self.start_point().compare_xy(&self.end_point()) == Ordering::Greater {
            let r = LineSegment::new(self.end, self.middle, self.start);
            if let Some(p) = self.end_point.get() {
                let _ = r.start_point.set(p.clone());
            }
            if let Some(p) = self.start_point.get() {
                let _ = r.end_point.set(p.clone());
            }
            r
        } else {
            self.clone()
        }
    }
}
