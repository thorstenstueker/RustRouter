//! Polygons (corner lists) and polygon shapes (possibly concave, without holes).

use super::basics::{java_round, JavaRandom, Side};
use super::{
    Circle, FloatLine, FloatPoint, IntBox, IntOctagon, IntPoint, Line, Point, TileShape, Vector,
};
use std::sync::{Arc, OnceLock};

/// A closed polygon given by its corners; consecutive equal corners and
/// collinear corners are removed.
#[derive(Clone, Debug)]
pub struct Polygon {
    corners: Vec<Point>,
}

impl Polygon {
    pub fn new(points: &[Point]) -> Polygon {
        let mut corners: Vec<Point> = points.to_vec();
        if corners.is_empty() {
            return Polygon { corners };
        }
        let mut corner_removed = true;
        while corner_removed {
            corner_removed = false;
            if corners.is_empty() {
                break;
            }
            // remove consecutive equal corners
            let mut i = 1;
            while i < corners.len() {
                if corners[i] == corners[i - 1] {
                    corners.remove(i);
                    corner_removed = true;
                } else {
                    i += 1;
                }
            }
            if corners.len() < 2 {
                continue;
            }
            // remove the first collinear corner
            for k in 1..corners.len().saturating_sub(1) {
                if corners[k].side_of(&corners[k - 1], &corners[k + 1]) == Side::Collinear {
                    corners.remove(k);
                    corner_removed = true;
                    break;
                }
            }
        }
        Polygon { corners }
    }

    pub fn corner_array(&self) -> Vec<Point> {
        self.corners.clone()
    }

    pub fn revert_corners(&self) -> Polygon {
        let rev: Vec<Point> = self.corners.iter().rev().cloned().collect();
        Polygon::new(&rev)
    }

    pub fn winding_number_after_closing(&self) -> i32 {
        let corners = &self.corners;
        if corners.len() < 2 {
            return 0;
        }
        let first_side = corners[1].difference_by(&corners[0]);
        let mut prev_side = first_side.clone();
        let mut n = corners.len();
        if corners[0] == corners[n - 1] {
            n -= 1;
        }
        let mut angle_sum = 0.0;
        for i in 1..n.saturating_sub(1) {
            let next = corners[i + 1].difference_by(&corners[i]);
            angle_sum += prev_side.angle_approx_to(&next);
            prev_side = next;
        }
        if n > 1 {
            let next = corners[0].difference_by(&corners[n - 1]);
            angle_sum += prev_side.angle_approx_to(&next);
            prev_side = next;
        }
        angle_sum += prev_side.angle_approx_to(&first_side);
        angle_sum /= 2.0 * std::f64::consts::PI;
        if angle_sum.abs() < 0.5 {
            log::warn!("Polygon.winding_number_after_closing: winding number != 0 expected");
        }
        java_round(angle_sum)
    }
}

#[derive(Debug)]
struct PolygonShapeInner {
    corners: Vec<Point>,
    bounding_box: OnceLock<IntBox>,
    bounding_octagon: OnceLock<IntOctagon>,
    convex_pieces: OnceLock<Option<Vec<TileShape>>>,
}

/// A polygon shape with counterclockwise corners starting at the lowest left corner.
#[derive(Clone, Debug)]
pub struct PolygonShape(Arc<PolygonShapeInner>);

const SEED: i64 = 99;

impl PolygonShape {
    pub fn from_polygon(polygon: &Polygon) -> PolygonShape {
        let mut cur = polygon.clone();
        if polygon.winding_number_after_closing() < 0 {
            cur = polygon.revert_corners();
        }
        let cc = cur.corner_array();
        let mut last_corner_no = cc.len() as isize - 1;
        if last_corner_no > 0 && cc[0] == cc[last_corner_no as usize] {
            last_corner_no -= 1;
        }
        let mut last_point_collinear = false;
        if last_corner_no >= 2 {
            let l = last_corner_no as usize;
            last_point_collinear = cc[l].side_of(&cc[l - 1], &cc[0]) == Side::Collinear;
        }
        if last_point_collinear {
            last_corner_no -= 1;
        }
        let mut first_corner_no: isize = 0;
        let mut first_point_collinear = false;
        if last_corner_no - first_corner_no >= 2 {
            first_point_collinear = cc[0].side_of(&cc[1], &cc[last_corner_no as usize]) == Side::Collinear;
        }
        if first_point_collinear {
            first_corner_no += 1;
        }
        let mut result = Vec::new();
        if last_corner_no >= first_corner_no {
            let mut start_corner_no = first_corner_no as usize;
            let mut start_corner = cc[start_corner_no].to_float();
            for i in start_corner_no + 1..=last_corner_no as usize {
                let c = cc[i].to_float();
                if c.y < start_corner.y || c.y == start_corner.y && c.x < start_corner.x {
                    start_corner_no = i;
                    start_corner = c;
                }
            }
            for i in start_corner_no..=last_corner_no as usize {
                result.push(cc[i].clone());
            }
            for i in first_corner_no as usize..start_corner_no {
                result.push(cc[i].clone());
            }
        }
        PolygonShape::from_raw(result)
    }

    fn from_raw(corners: Vec<Point>) -> PolygonShape {
        PolygonShape(Arc::new(PolygonShapeInner {
            corners,
            bounding_box: OnceLock::new(),
            bounding_octagon: OnceLock::new(),
            convex_pieces: OnceLock::new(),
        }))
    }

    pub fn new(corners: &[Point]) -> PolygonShape {
        PolygonShape::from_polygon(&Polygon::new(corners))
    }

    #[inline]
    pub fn corners(&self) -> &[Point] {
        &self.0.corners
    }

    pub fn corner(&self, no: usize) -> Point {
        self.0.corners[no].clone()
    }

    pub fn border_line_count(&self) -> usize {
        self.0.corners.len()
    }

    pub fn corner_approx(&self, no: usize) -> FloatPoint {
        self.0.corners[no].to_float()
    }

    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        self.0.corners.iter().map(|c| c.to_float()).collect()
    }

    pub fn border_distance(&self, p: &FloatPoint) -> f64 {
        let c = &self.0.corners;
        if c.is_empty() {
            return 0.0;
        }
        let mut prev = c[c.len() - 1].to_float();
        let mut result = prev.distance(p);
        for corner in c {
            let cur = corner.to_float();
            result = result.min(FloatLine::new(prev, cur).segment_distance(p));
            prev = cur;
        }
        result
    }

    pub fn centre_of_gravity(&self) -> FloatPoint {
        let n = self.border_line_count();
        let mut x = 0.0;
        let mut y = 0.0;
        for c in &self.0.corners {
            let f = c.to_float();
            x += f.x;
            y += f.y;
        }
        FloatPoint::new(x / n as f64, y / n as f64)
    }

    pub fn smallest_radius(&self) -> f64 {
        self.border_distance(&self.centre_of_gravity())
    }

    pub fn contains_float(&self, p: &FloatPoint) -> bool {
        self.split_to_convex().iter().any(|t| t.contains_float(p))
    }

    pub fn contains(&self, p: &Point) -> bool {
        !self.is_outside(p)
    }

    pub fn contains_inside(&self, p: &Point) -> bool {
        !self.is_outside(p)
    }

    pub fn is_outside(&self, p: &Point) -> bool {
        self.split_to_convex().iter().all(|t| t.is_outside(p))
    }

    pub fn translate_by(&self, v: &Vector) -> PolygonShape {
        if v.is_zero() {
            return self.clone();
        }
        let nc: Vec<Point> = self.0.corners.iter().map(|c| c.translate_by(v)).collect();
        PolygonShape::new(&nc)
    }

    pub fn bounding_box(&self) -> IntBox {
        *self.0.bounding_box.get_or_init(|| {
            let mut llx = i32::MAX as f64;
            let mut lly = i32::MAX as f64;
            let mut urx = i32::MIN as f64;
            let mut ury = i32::MIN as f64;
            for c in &self.0.corners {
                let f = c.to_float();
                llx = llx.min(f.x);
                lly = lly.min(f.y);
                urx = urx.max(f.x);
                ury = ury.max(f.y);
            }
            IntBox::new(llx.floor() as i32, lly.floor() as i32, urx.ceil() as i32, ury.ceil() as i32)
        })
    }

    pub fn bounding_octagon(&self) -> IntOctagon {
        *self.0.bounding_octagon.get_or_init(|| {
            let pts: Vec<FloatPoint> = self.0.corners.iter().map(|c| c.to_float()).collect();
            FloatPoint::bounding_octagon(&pts)
        })
    }

    pub fn is_convex(&self) -> bool {
        let c = &self.0.corners;
        let n = c.len();
        if n <= 2 {
            return true;
        }
        let mut prev = &c[n - 1];
        let mut cur = &c[0];
        let mut next = &c[1];
        for ind in 0..n {
            if next.side_of(prev, cur) == Side::OnTheRight {
                return false;
            }
            prev = cur;
            cur = next;
            next = if ind == n - 2 {
                &c[0]
            } else if ind == n - 1 {
                &c[1]
            } else {
                &c[ind + 2]
            };
        }
        let first_line = Line::from_points(&c[n - 1], &c[0]);
        let mut current_line = Line::from_points(&c[0], &c[1]);
        let first_dir = first_line.direction();
        let mut last_det = first_dir.determinant(&current_line.direction());
        for corner in c.iter().skip(2) {
            current_line = Line::new(current_line.b, corner.to_int());
            let cur_det = first_dir.determinant(&current_line.direction());
            if last_det <= 0.0 && cur_det > 0.0 {
                return false;
            }
            last_det = cur_det;
        }
        true
    }

    pub fn convex_hull(&self) -> PolygonShape {
        let c = &self.0.corners;
        let n = c.len();
        if n <= 2 {
            return self.clone();
        }
        let mut prev = &c[n - 1];
        let mut cur = &c[0];
        for ind in 0..n {
            let next = if ind == n - 1 { &c[0] } else { &c[ind + 1] };
            if next.side_of(prev, cur) != Side::OnTheLeft {
                let mut nc: Vec<Point> = c.clone();
                nc.remove(ind);
                return PolygonShape::new(&nc).convex_hull();
            }
            prev = cur;
            cur = next;
        }
        self.clone()
    }

    pub fn bounding_tile(&self) -> TileShape {
        let hull = self.convex_hull();
        let hc = hull.corners();
        let n = hc.len();
        let mut lines = Vec::with_capacity(n);
        for i in 0..n - 1 {
            lines.push(Line::from_points(&hc[i], &hc[i + 1]));
        }
        lines.push(Line::from_points(&hc[n - 1], &hc[0]));
        TileShape::from_lines(&lines)
    }

    pub fn area(&self) -> f64 {
        if self.dimension() < 2 {
            return 0.0;
        }
        let c = &self.0.corners;
        let n = c.len();
        let mut result = 0.0;
        let mut prev = c[n - 2].to_float();
        let mut cur = c[n - 1].to_float();
        for corner in c {
            let next = corner.to_float();
            result += cur.x * (next.y - prev.y);
            prev = cur;
            cur = next;
        }
        0.5 * result.abs()
    }

    pub fn circumference(&self) -> f64 {
        let n = self.border_line_count();
        if n == 0 {
            return 0.0;
        }
        let mut result = 0.0;
        let mut prev = self.corner_approx(n - 1);
        for i in 0..n {
            let c = self.corner_approx(i);
            result += c.distance(&prev);
            prev = c;
        }
        result
    }

    pub fn dimension(&self) -> i32 {
        match self.0.corners.len() {
            0 => -1,
            1 => 0,
            2 => 1,
            _ => 2,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.corners.is_empty()
    }

    pub fn border_line(&self, no: usize) -> Line {
        let c = &self.0.corners;
        let next = if no == c.len() - 1 { &c[0] } else { &c[no + 1] };
        Line::from_points(&c[no], next)
    }

    pub fn nearest_point_approx(&self, from: &FloatPoint) -> Option<FloatPoint> {
        let mut min_dist = f64::MAX;
        let mut result = None;
        for t in self.split_to_convex() {
            if let Some(n) = t.nearest_point_approx(from) {
                let d = n.distance_square(from);
                if d < min_dist {
                    min_dist = d;
                    result = Some(n);
                }
            }
        }
        result
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> PolygonShape {
        let p = Point::Int(*pole);
        let nc: Vec<Point> = self.0.corners.iter().map(|c| c.turn_90_degree(factor, &p)).collect();
        PolygonShape::new(&nc)
    }

    pub fn rotate_approx(&self, angle: f64, pole: &FloatPoint) -> PolygonShape {
        if angle == 0.0 {
            return self.clone();
        }
        let nc: Vec<Point> = self
            .0
            .corners
            .iter()
            .map(|c| Point::Int(c.to_float().rotate(angle, pole).round()))
            .collect();
        PolygonShape::new(&nc)
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> PolygonShape {
        let p = Point::Int(*pole);
        let nc: Vec<Point> = self.0.corners.iter().map(|c| c.mirror_vertical(&p)).collect();
        PolygonShape::new(&nc)
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> PolygonShape {
        let p = Point::Int(*pole);
        let nc: Vec<Point> = self.0.corners.iter().map(|c| c.mirror_horizontal(&p)).collect();
        PolygonShape::new(&nc)
    }

    pub fn intersects_tile(&self, t: &TileShape) -> bool {
        self.split_to_convex().iter().any(|p| p.intersects(t))
    }

    pub fn intersects_circle(&self, c: &Circle) -> bool {
        self.split_to_convex().iter().any(|p| c.intersects_tile(p))
    }

    pub fn intersects_polygon(&self, other: &PolygonShape) -> bool {
        let a = self.split_to_convex();
        let b = other.split_to_convex();
        a.iter().any(|p| b.iter().any(|q| p.intersects(q)))
    }

    /// Divides the shape into convex pieces. Returns an empty slice, if the division failed.
    pub fn split_to_convex(&self) -> &[TileShape] {
        self.0
            .convex_pieces
            .get_or_init(|| {
                // The original resets a shared generator to the same seed for each call,
                // so a fresh generator per call is equivalent.
                let mut rnd = JavaRandom::new(SEED);
                let pieces = self.split_to_convex_recu(&mut rnd)?;
                Some(
                    pieces
                        .into_iter()
                        .map(|p| TileShape::from_convex_polygon(p.corners()))
                        .collect(),
                )
            })
            .as_deref()
            .unwrap_or(&[])
    }

    /// Like `split_to_convex`, but returns None, if the division failed.
    pub fn try_split_to_convex(&self) -> Option<&[TileShape]> {
        let _ = self.split_to_convex();
        self.0.convex_pieces.get().unwrap().as_deref()
    }

    fn split_to_convex_recu(&self, rnd: &mut JavaRandom) -> Option<Vec<PolygonShape>> {
        let c = &self.0.corners;
        let n = c.len();
        if n == 0 {
            return Some(vec![self.clone()]);
        }
        let mut start_corner_no = rnd.next_int(n as i32) as usize;
        let mut current_corner = &c[start_corner_no];
        let mut prev_corner = if start_corner_no != 0 {
            &c[start_corner_no - 1]
        } else {
            &c[n - 1]
        };
        let mut concave_corner_no: isize = -1;
        for _ in 0..n {
            let next_corner = if start_corner_no < n - 1 {
                &c[start_corner_no + 1]
            } else {
                &c[0]
            };
            if next_corner.side_of(prev_corner, current_corner) == Side::OnTheRight {
                concave_corner_no = start_corner_no as isize;
                break;
            }
            prev_corner = current_corner;
            current_corner = next_corner;
            start_corner_no = (start_corner_no + 1) % n;
        }
        if concave_corner_no < 0 {
            return Some(vec![self.clone()]);
        }
        let ccn = concave_corner_no as usize;
        let (projection, corner_no_after_projection) = self.division_point(ccn);
        let projection = projection?;
        let mut corner_count = corner_no_after_projection as isize - ccn as isize;
        if corner_count < 0 {
            corner_count += n as isize;
        }
        corner_count += 1;
        let mut first_arr = Vec::with_capacity(corner_count as usize);
        let mut ci = ccn;
        for _ in 0..corner_count - 1 {
            first_arr.push(c[ci].clone());
            ci = (ci + 1) % n;
        }
        first_arr.push(Point::Int(projection.round()));
        let mut corner_count = ccn as isize - corner_no_after_projection as isize;
        if corner_count < 0 {
            corner_count += n as isize;
        }
        corner_count += 2;
        let mut last_arr = Vec::with_capacity(corner_count as usize);
        last_arr.push(Point::Int(projection.round()));
        let mut ci = corner_no_after_projection;
        for _ in 1..corner_count {
            last_arr.push(c[ci].clone());
            ci = (ci + 1) % n;
        }
        let last_piece = PolygonShape::new(&last_arr);
        let first_piece = PolygonShape::new(&first_arr);
        let mut c1 = first_piece.split_to_convex_recu(rnd)?;
        let c2 = last_piece.split_to_convex_recu(rnd)?;
        c1.extend(c2);
        Some(c1)
    }

    fn division_point(&self, concave_corner_no: usize) -> (Option<FloatPoint>, usize) {
        let c = &self.0.corners;
        let n = c.len();
        let concave = c[concave_corner_no].to_float();
        let before = if concave_corner_no != 0 {
            c[concave_corner_no - 1].to_float()
        } else {
            c[n - 1].to_float()
        };
        let after = if concave_corner_no == n - 1 {
            c[0].to_float()
        } else {
            c[concave_corner_no + 1].to_float()
        };
        let search_right = before.y > concave.y || concave.y > after.y;
        let search_left = before.y < concave.y || concave.y < after.y;
        let search_up = before.x < concave.x || concave.x < after.x;
        let search_down = before.x > concave.x || concave.x > after.x;
        let mut min_projection_dist = i32::MAX as f64;
        let mut min_projection = None;
        let mut corner_no_after_min_projection = 0;
        let mut cnacp = (concave_corner_no + 2) % n;
        let mut cbcp = if cnacp != 0 { &c[cnacp - 1] } else { &c[n - 1] };
        let mut cbpa = cbcp.to_float();
        let loop_end = n.saturating_sub(2);
        for _ in 0..loop_end {
            let cacp = &c[cnacp];
            let capa = cacp.to_float();
            if cbpa.y != capa.y {
                let (min_y, max_y) = if capa.y > cbpa.y { (cbpa.y, capa.y) } else { (capa.y, cbpa.y) };
                if concave.y >= min_y && concave.y <= max_y {
                    let cl = Line::from_points(cbcp, cacp);
                    let xi = cl.function_in_y_value_approx(concave.y);
                    let d = (xi - concave.x).abs();
                    let ok = d < min_projection_dist
                        && (search_right && xi > concave.x && concave.y <= capa.y
                            || search_left && xi < concave.x && concave.y >= capa.y);
                    if ok {
                        min_projection_dist = d;
                        corner_no_after_min_projection = cnacp;
                        min_projection = Some(FloatPoint::new(xi, concave.y));
                    }
                }
            }
            if cbpa.x != capa.x {
                let (min_x, max_x) = if capa.x > cbpa.x { (cbpa.x, capa.x) } else { (capa.x, cbpa.x) };
                if concave.x >= min_x && concave.x <= max_x {
                    let cl = Line::from_points(cbcp, cacp);
                    let yi = cl.function_value_approx(concave.x);
                    let d = (yi - concave.y).abs();
                    let ok = d < min_projection_dist
                        && (search_up && yi > concave.y && concave.x >= capa.x
                            || search_down && yi < concave.y && concave.x <= capa.x);
                    if ok {
                        min_projection_dist = d;
                        corner_no_after_min_projection = cnacp;
                        min_projection = Some(FloatPoint::new(concave.x, yi));
                    }
                }
            }
            cbcp = cacp;
            cbpa = capa;
            cnacp = if cnacp == n - 1 { 0 } else { cnacp + 1 };
        }
        if min_projection_dist == i32::MAX as f64 {
            log::warn!(
                "PolygonShape.DivisionPoint: projection not found for concave corner #{} in polygon with {} corners",
                concave_corner_no,
                n
            );
        }
        (min_projection, corner_no_after_min_projection)
    }
}
