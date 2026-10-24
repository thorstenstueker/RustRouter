//! Polylines defined by a sequence of lines. The corners are the intersections of
//! consecutive lines; the first and the last line close the polyline at its ends.

use super::basics::Side;
use super::{
    Direction, FloatPoint, IntBox, IntOctagon, IntPoint, Line, LineSegment, Point, Polygon,
    TileShape, Vector,
};
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
struct PolylineInner {
    lines: Vec<Line>,
    corners: OnceLock<Box<[Point]>>,
    float_corners: OnceLock<Box<[FloatPoint]>>,
    bounding_box: OnceLock<IntBox>,
}

/// A polyline. Cloning is cheap (reference counted).
#[derive(Clone, Debug)]
pub struct Polyline(Arc<PolylineInner>);

const USE_BOUNDING_OCTAGON_FOR_OFFSET_SHAPES: bool = true;

impl Polyline {
    fn from_raw(lines: Vec<Line>) -> Polyline {
        Polyline(Arc::new(PolylineInner {
            lines,
            corners: OnceLock::new(),
            float_corners: OnceLock::new(),
            bounding_box: OnceLock::new(),
        }))
    }

    pub fn empty() -> Polyline {
        Polyline::from_raw(Vec::new())
    }

    pub fn from_polygon(polygon: &Polygon) -> Polyline {
        let points = polygon.corner_array();
        if points.len() < 2 {
            log::warn!("Polyline: must contain at least 2 different points");
            return Polyline::empty();
        }
        let n = points.len();
        let mut lines = Vec::with_capacity(n + 1);
        let dir = Direction::from_points(&points[0], &points[1]).unwrap();
        lines.push(Line::from_point_direction(&points[0], &dir.turn_45_degree(2)));
        for i in 1..n {
            lines.push(Line::from_points(&points[i - 1], &points[i]));
        }
        let dir = Direction::from_points(&points[n - 1], &points[n - 2]).unwrap();
        lines.push(Line::from_point_direction(&points[n - 1], &dir.turn_45_degree(2)));
        Polyline::from_raw(lines)
    }

    pub fn from_points(points: &[Point]) -> Polyline {
        Polyline::from_polygon(&Polygon::new(points))
    }

    pub fn from_int_points(points: &[IntPoint]) -> Polyline {
        let pts: Vec<Point> = points.iter().map(|p| Point::Int(*p)).collect();
        Polyline::from_points(&pts)
    }

    pub fn from_corners(from: &Point, to: &Point) -> Polyline {
        if from == to {
            return Polyline::empty();
        }
        let dir = Direction::from_points(from, to).unwrap();
        let lines = vec![
            Line::from_point_direction(from, &dir.turn_45_degree(2)),
            Line::from_points(from, to),
            Line::from_point_direction(to, &dir.turn_45_degree(2)),
        ];
        Polyline::from_raw(lines)
    }

    /// Creates a polyline from lines, removing parallel and overlapping lines.
    pub fn from_lines(input: &[Line]) -> Polyline {
        let filtered = remove_consecutive_parallel_lines(input);
        let mut filtered = remove_overlaps(&filtered);
        if filtered.len() < 3 {
            return Polyline::empty();
        }
        let n = filtered.len();
        for i in 1..n - 1 {
            let corner = filtered[i].intersection_approx(&filtered[i + 1]);
            let side_of_line = filtered[i - 1].side_of_float(&corner);
            if side_of_line != Side::Collinear {
                let d0 = filtered[i - 1].direction().get_vector();
                let d1 = filtered[i].direction().get_vector();
                let side1 = d0.side_of(&d1);
                if side1 != side_of_line {
                    filtered[i] = filtered[i].opposite();
                }
            }
        }
        Polyline::from_raw(filtered)
    }

    #[inline]
    pub fn lines(&self) -> &[Line] {
        &self.0.lines
    }

    #[inline]
    pub fn line(&self, i: usize) -> Line {
        self.0.lines[i]
    }

    #[inline]
    pub fn line_count(&self) -> usize {
        self.0.lines.len()
    }

    pub fn ptr_eq(&self, other: &Polyline) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    #[inline]
    pub fn corner_count(&self) -> usize {
        self.0.lines.len().saturating_sub(1)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.lines.len() < 3
    }

    pub fn is_point(&self) -> bool {
        if self.0.lines.len() < 3 {
            return true;
        }
        let first = self.corner_ref(0);
        for i in 1..self.0.lines.len() - 1 {
            if self.corner_ref(i) != first {
                return false;
            }
        }
        true
    }

    pub fn is_orthogonal(&self) -> bool {
        self.0.lines.iter().all(|l| l.is_orthogonal())
    }

    pub fn is_multiple_of_45_degree(&self) -> bool {
        self.0.lines.iter().all(|l| l.is_multiple_of_45_degree())
    }

    pub fn first_corner(&self) -> Point {
        self.corner(0)
    }

    pub fn last_corner(&self) -> Point {
        self.corner(self.0.lines.len().saturating_sub(2))
    }

    fn corners_arr(&self) -> &[Point] {
        self.0.corners.get_or_init(|| {
            let l = &self.0.lines;
            if l.len() < 2 {
                return Box::new([]);
            }
            (0..l.len() - 1).map(|i| l[i].intersection(&l[i + 1])).collect()
        })
    }

    fn float_corners_arr(&self) -> &[FloatPoint] {
        self.0.float_corners.get_or_init(|| {
            let l = &self.0.lines;
            if l.len() < 2 {
                return Box::new([]);
            }
            (0..l.len() - 1).map(|i| l[i].intersection_approx(&l[i + 1])).collect()
        })
    }

    pub fn corners(&self) -> Vec<Point> {
        self.corners_arr().to_vec()
    }

    pub fn corner_approx_arr(&self) -> &[FloatPoint] {
        self.float_corners_arr()
    }

    fn clamp(&self, i: usize) -> usize {
        let max = self.0.lines.len().saturating_sub(2);
        if i > max {
            max
        } else {
            i
        }
    }

    pub fn corner_approx(&self, i: usize) -> FloatPoint {
        let no = self.clamp(i);
        self.float_corners_arr()[no]
    }

    pub fn corner(&self, i: usize) -> Point {
        self.corner_ref(i).clone()
    }

    pub fn corner_ref(&self, i: usize) -> &Point {
        let no = self.clamp(i);
        &self.corners_arr()[no]
    }

    pub fn reverse(&self) -> Polyline {
        let l = &self.0.lines;
        let rev: Vec<Line> = (0..l.len()).map(|i| l[l.len() - i - 1].opposite()).collect();
        Polyline::from_lines(&rev)
    }

    pub fn length_approx_range(&self, from: i32, to: i32) -> f64 {
        let from = from.max(0) as usize;
        let to = to.min(self.0.lines.len() as i32 - 2);
        let mut result = 0.0;
        if to <= 0 {
            return 0.0;
        }
        for i in from..to as usize {
            result += self.corner_approx(i + 1).distance(&self.corner_approx(i));
        }
        result
    }

    pub fn length_approx(&self) -> f64 {
        self.length_approx_range(0, self.0.lines.len() as i32 - 2)
    }

    pub fn offset_shapes(&self, half_width: i32) -> Vec<TileShape> {
        self.offset_shapes_range(half_width, 0, self.0.lines.len() as i32 - 1)
    }

    /// Calculates the tiles covering the segments from `from_no` to `to_no` enlarged by
    /// `half_width`.
    pub fn offset_shapes_range(&self, half_width: i32, requested_from: i32, requested_to: i32) -> Vec<TileShape> {
        let lines = &self.0.lines;
        let from_no = requested_from.max(0) as usize;
        let to_no = requested_to.min(lines.len() as i32 - 1);
        if to_no < 0 {
            return Vec::new();
        }
        let to_no = to_no as usize;
        let shape_count = (to_no as i64 - from_no as i64 - 1).max(0) as usize;
        let mut shapes = Vec::with_capacity(shape_count);
        if shape_count == 0 {
            return shapes;
        }
        let hw = half_width as f64;
        let mut prev_dir = Vector::Int(lines[from_no].direction().get_vector());
        let mut current_direction = Vector::Int(lines[from_no + 1].direction().get_vector());
        for i in from_no + 1..to_no {
            let next_dir = Vector::Int(lines[i + 1].direction().get_vector());
            let mut offset_lines = [lines[i]; 4];
            offset_lines[0] = lines[i].translate(-hw);
            let next_dir_from_curr_dir = next_dir.side_of(&current_direction);
            offset_lines[1] = if next_dir_from_curr_dir == Side::OnTheLeft {
                lines[i + 1].translate(-hw)
            } else {
                lines[i + 1].opposite().translate(-hw)
            };
            offset_lines[2] = lines[i].opposite().translate(-hw);
            let current_dir_from_prev_dir = current_direction.side_of(&prev_dir);
            offset_lines[3] = if current_dir_from_prev_dir == Side::OnTheLeft {
                lines[i - 1].translate(-hw)
            } else {
                lines[i - 1].opposite().translate(-hw)
            };
            let mut corner_to_check: Option<FloatPoint> = None;
            let mut current_line = offset_lines[1];
            let mut check_line = if next_dir_from_curr_dir == Side::OnTheLeft {
                offset_lines[2]
            } else {
                offset_lines[0]
            };
            let mut check_distance_corner = self.corner_approx(i);
            let check_dist_square = 2.0 * hw * hw;
            let mut cut_dog_ear_lines: Vec<Line> = Vec::new();
            let mut tmp_curr_dir = next_dir.clone();
            let mut direction_changed = false;
            for j in i + 2..lines.len() - 1 {
                if self.corner_approx(j - 1).distance_square(&check_distance_corner) > check_dist_square {
                    break;
                }
                if !direction_changed {
                    corner_to_check = Some(current_line.intersection_approx(&check_line));
                }
                let tmp_next_dir = Vector::Int(lines[j].direction().get_vector());
                let s = tmp_next_dir.side_of(&tmp_curr_dir);
                direction_changed = s != next_dir_from_curr_dir;
                if !direction_changed {
                    let next_border_line = if s == Side::OnTheLeft {
                        lines[j].translate(-hw)
                    } else {
                        lines[j].opposite().translate(-hw)
                    };
                    if let Some(c) = corner_to_check {
                        if next_border_line.side_of_float(&c) == Side::OnTheLeft
                            && next_border_line.side_of(self.corner_ref(i)) == Side::OnTheRight
                            && next_border_line.side_of(self.corner_ref(i - 1)) == Side::OnTheRight
                        {
                            cut_dog_ear_lines.push(next_border_line);
                        }
                    }
                    tmp_curr_dir = tmp_next_dir;
                    current_line = next_border_line;
                }
            }
            check_distance_corner = self.corner_approx(i - 1);
            check_line = if current_dir_from_prev_dir == Side::OnTheLeft {
                offset_lines[2]
            } else {
                offset_lines[0]
            };
            current_line = offset_lines[3];
            tmp_curr_dir = prev_dir.clone();
            direction_changed = false;
            let mut j = i as isize - 2;
            while j >= 1 {
                let ju = j as usize;
                if self.corner_approx(ju).distance_square(&check_distance_corner) > check_dist_square {
                    break;
                }
                if !direction_changed {
                    corner_to_check = Some(current_line.intersection_approx(&check_line));
                }
                let tmp_prev_dir = Vector::Int(lines[ju].direction().get_vector());
                let s = tmp_curr_dir.side_of(&tmp_prev_dir);
                direction_changed = s != current_dir_from_prev_dir;
                if !direction_changed {
                    let prev_border_line = if s == Side::OnTheLeft {
                        lines[ju].translate(-hw)
                    } else {
                        lines[ju].opposite().translate(-hw)
                    };
                    if let Some(c) = corner_to_check {
                        if prev_border_line.side_of_float(&c) == Side::OnTheLeft
                            && prev_border_line.side_of(self.corner_ref(i)) == Side::OnTheRight
                            && prev_border_line.side_of(self.corner_ref(i - 1)) == Side::OnTheRight
                        {
                            cut_dog_ear_lines.push(prev_border_line);
                        }
                    }
                    tmp_curr_dir = tmp_prev_dir;
                    current_line = prev_border_line;
                }
                j -= 1;
            }
            let mut s1 = TileShape::from_lines(&offset_lines);
            if !cut_dog_ear_lines.is_empty() {
                s1 = s1.intersection(&TileShape::from_lines(&cut_dog_ear_lines));
            }
            let bounding_shape = if USE_BOUNDING_OCTAGON_FOR_OFFSET_SHAPES {
                TileShape::Octagon(self.bounding_octagon_range(i as i32 - 1, i as i32).offset(hw))
            } else {
                TileShape::Simplex(self.bounding_box_range(i as i32 - 1, i as i32).offset(hw).to_simplex())
            };
            let shape = bounding_shape.intersection_with_simplify(&s1);
            if shape.is_empty() {
                log::warn!("offset_shapes: shape is empty");
            }
            shapes.push(shape);
            prev_dir = current_direction;
            current_direction = next_dir;
        }
        shapes
    }

    pub fn offset_shape(&self, half_width: i32, no: usize) -> Option<TileShape> {
        if no + 3 > self.0.lines.len() {
            log::warn!("Polyline.offsetShape: no out of range");
            return None;
        }
        self.offset_shapes_range(half_width, no as i32, no as i32 + 2).into_iter().next()
    }

    pub fn offset_box(&self, half_width: i32, no: usize) -> IntBox {
        LineSegment::from_polyline(self, no + 1)
            .bounding_box()
            .offset(half_width as f64)
    }

    pub fn translate_by(&self, v: &Vector) -> Polyline {
        if v.is_zero() {
            return self.clone();
        }
        let nl: Vec<Line> = self.0.lines.iter().map(|l| l.translate_by(v)).collect();
        Polyline::from_lines(&nl)
    }

    pub fn turn_90_degree(&self, factor: i32, pole: &IntPoint) -> Polyline {
        let nl: Vec<Line> = self.0.lines.iter().map(|l| l.turn_90_degree(factor, pole)).collect();
        Polyline::from_lines(&nl)
    }

    pub fn rotate_approx(&self, angle: f64, pole: &FloatPoint) -> Polyline {
        if angle == 0.0 {
            return self.clone();
        }
        let nc: Vec<Point> = (0..self.corner_count())
            .map(|i| Point::Int(self.corner_approx(i).rotate(angle, pole).round()))
            .collect();
        Polyline::from_points(&nc)
    }

    pub fn mirror_vertical(&self, pole: &IntPoint) -> Polyline {
        let nl: Vec<Line> = self.0.lines.iter().map(|l| l.mirror_vertical(pole)).collect();
        Polyline::from_lines(&nl)
    }

    pub fn mirror_horizontal(&self, pole: &IntPoint) -> Polyline {
        let nl: Vec<Line> = self.0.lines.iter().map(|l| l.mirror_horizontal(pole)).collect();
        Polyline::from_lines(&nl)
    }

    pub fn bounding_box_range(&self, from: i32, to: i32) -> IntBox {
        let from = from.max(0);
        let to = to.min(self.0.lines.len() as i32 - 2);
        let mut llx = i32::MAX as f64;
        let mut lly = llx;
        let mut urx = i32::MIN as f64;
        let mut ury = urx;
        let mut i = from;
        while i <= to {
            let c = self.corner_approx(i as usize);
            llx = llx.min(c.x);
            lly = lly.min(c.y);
            urx = urx.max(c.x);
            ury = ury.max(c.y);
            i += 1;
        }
        IntBox::new(llx.floor() as i32, lly.floor() as i32, urx.ceil() as i32, ury.ceil() as i32)
    }

    pub fn bounding_box(&self) -> IntBox {
        *self
            .0
            .bounding_box
            .get_or_init(|| self.bounding_box_range(0, self.corner_count() as i32 - 1))
    }

    pub fn bounding_octagon_range(&self, from: i32, to: i32) -> IntOctagon {
        let from = from.max(0);
        let to = to.min(self.0.lines.len() as i32 - 2);
        let mut lx = i32::MAX as f64;
        let mut ly = i32::MAX as f64;
        let mut rx = i32::MIN as f64;
        let mut uy = i32::MIN as f64;
        let mut ulx = i32::MAX as f64;
        let mut lrx = i32::MIN as f64;
        let mut llx = i32::MAX as f64;
        let mut urx = i32::MIN as f64;
        let mut i = from;
        while i <= to {
            let c = self.corner_approx(i as usize);
            lx = lx.min(c.x);
            ly = ly.min(c.y);
            rx = rx.max(c.x);
            uy = uy.max(c.y);
            let t = c.x - c.y;
            ulx = ulx.min(t);
            lrx = lrx.max(t);
            let t = c.x + c.y;
            llx = llx.min(t);
            urx = urx.max(t);
            i += 1;
        }
        IntOctagon::new(
            lx.floor() as i32,
            ly.floor() as i32,
            rx.ceil() as i32,
            uy.ceil() as i32,
            ulx.floor() as i32,
            lrx.ceil() as i32,
            llx.floor() as i32,
            urx.ceil() as i32,
        )
    }

    pub fn nearest_point_approx(&self, from: &FloatPoint) -> FloatPoint {
        let mut min_distance = f64::MAX;
        let corners = self.float_corners_arr();
        let mut nearest = from.clone();
        for c in corners {
            let d = c.distance(from);
            if d < min_distance {
                min_distance = d;
                nearest = *c;
            }
        }
        let tol = 1.0;
        let lines = &self.0.lines;
        for i in 1..lines.len().saturating_sub(1) {
            let projection = from.projection_approx(&lines[i]);
            let d = projection.distance(from);
            if d < min_distance {
                let seg_len = corners[i].distance(&corners[i - 1]);
                if projection.distance(&corners[i]) + projection.distance(&corners[i - 1]) < seg_len + tol {
                    min_distance = d;
                    nearest = projection;
                }
            }
        }
        nearest
    }

    pub fn distance(&self, from: &FloatPoint) -> f64 {
        from.distance(&self.nearest_point_approx(from))
    }

    /// Combines the two polylines, if they have a common end corner.
    pub fn combine(&self, other: &Polyline) -> Polyline {
        let tl = &self.0.lines;
        let ol = &other.0.lines;
        if tl.len() < 3 || ol.len() < 3 {
            return self.clone();
        }
        let (combine_at_start, combine_other_at_start);
        if self.first_corner() == other.first_corner() {
            combine_at_start = true;
            combine_other_at_start = true;
        } else if self.first_corner() == other.last_corner() {
            combine_at_start = true;
            combine_other_at_start = false;
        } else if self.last_corner() == other.first_corner() {
            combine_at_start = false;
            combine_other_at_start = true;
        } else if self.last_corner() == other.last_corner() {
            combine_at_start = false;
            combine_other_at_start = false;
        } else {
            return self.clone();
        }
        let mut nl: Vec<Line> = Vec::with_capacity(tl.len() + ol.len() - 2);
        if combine_at_start {
            if combine_other_at_start {
                for i in 0..ol.len() - 1 {
                    nl.push(ol[ol.len() - i - 1].opposite());
                }
            } else {
                nl.extend_from_slice(&ol[..ol.len() - 1]);
            }
            nl.extend_from_slice(&tl[1..]);
        } else {
            nl.extend_from_slice(&tl[..tl.len() - 1]);
            if combine_other_at_start {
                nl.extend_from_slice(&ol[1..]);
            } else {
                for i in 1..ol.len() {
                    nl.push(ol[ol.len() - i - 1].opposite());
                }
            }
        }
        Polyline::from_lines(&nl)
    }

    /// Splits the polyline at the intersection of the line `line_index` with `end_line`.
    pub fn split(&self, line_index: usize, end_line: &Line) -> Option<(Polyline, Polyline)> {
        let lines = &self.0.lines;
        if line_index < 1 || line_index + 2 > lines.len() {
            log::warn!("Polyline.split: lineIndex out of range");
            return None;
        }
        if lines[line_index].is_parallel(end_line) {
            return None;
        }
        let new_end_corner = lines[line_index].intersection(end_line);
        if line_index == 1 && new_end_corner == self.first_corner()
            || line_index + 2 >= lines.len() && new_end_corner == self.last_corner()
        {
            return None;
        }
        let first_piece: Vec<Line> = if *self.corner_ref(line_index - 1) == new_end_corner {
            lines[..line_index + 1].to_vec()
        } else {
            let mut v = lines[..line_index + 1].to_vec();
            v.push(*end_line);
            v
        };
        let second_piece: Vec<Line> = if *self.corner_ref(line_index) == new_end_corner {
            lines[line_index..].to_vec()
        } else {
            let mut v = vec![*end_line];
            v.extend_from_slice(&lines[line_index..]);
            v
        };
        let r0 = Polyline::from_lines(&first_piece);
        let r1 = Polyline::from_lines(&second_piece);
        if r0.is_point() || r1.is_point() {
            return None;
        }
        Some((r0, r1))
    }

    /// Removes the lines from `from_no` to `to_no` (inclusive).
    pub fn skip_lines(&self, from_no: usize, to_no: usize) -> Polyline {
        let lines = &self.0.lines;
        if to_no + 1 > lines.len() || from_no > to_no {
            return self.clone();
        }
        let mut nl = lines[..from_no].to_vec();
        nl.extend_from_slice(&lines[to_no + 1..]);
        Polyline::from_lines(&nl)
    }

    pub fn contains(&self, p: &Point) -> bool {
        for i in 1..self.0.lines.len().saturating_sub(1) {
            if LineSegment::from_polyline(self, i).contains(p) {
                return true;
            }
        }
        false
    }

    pub fn projection_line(&self, p: &Point) -> Option<LineSegment> {
        let from = p.to_float();
        let mut min_distance = f64::MAX;
        let mut result_line: Option<Line> = None;
        let mut nearest_line: Option<Line> = None;
        let lines = &self.0.lines;
        for i in 1..lines.len().saturating_sub(1) {
            let projection = from.projection_approx(&lines[i]);
            let d = projection.distance(&from);
            if d < min_distance {
                let Some(dir) = lines[i].perpendicular_direction(p) else {
                    continue;
                };
                let crl = Line::from_point_direction(p, &Direction::Int(dir));
                let pcs = crl.side_of(self.corner_ref(i - 1));
                let ncs = crl.side_of(self.corner_ref(i));
                if pcs == ncs && pcs != Side::Collinear {
                    continue;
                }
                nearest_line = Some(lines[i]);
                min_distance = d;
                result_line = Some(crl);
            }
        }
        let nl = nearest_line?;
        let start_line = Line::from_point_direction(p, &Direction::Int(nl.direction()));
        Some(LineSegment::new(start_line, result_line.unwrap(), nl))
    }

    pub fn shorten(&self, new_line_count: usize, last_segment_length: f64) -> Polyline {
        let lines = &self.0.lines;
        let last_corner = self.corner_approx(new_line_count - 2);
        let prev_last_corner = self.corner_approx(new_line_count - 3);
        let new_last_corner = prev_last_corner.change_length(&last_corner, last_segment_length).round();
        if Point::Int(new_last_corner) == *self.corner_ref(self.corner_count() - 2) {
            return self.skip_lines(new_line_count - 1, new_line_count - 1);
        }
        let mut nl: Vec<Line> = lines[..new_line_count - 2].to_vec();
        let mut first_line_point = lines[new_line_count - 2].a;
        if first_line_point == new_last_corner {
            first_line_point = lines[new_line_count - 2].b;
        }
        let new_prev_last_line = Line::new(first_line_point, new_last_corner);
        nl.push(new_prev_last_line);
        nl.push(Line::from_point_dir(new_last_corner, &new_prev_last_line.direction().turn_45_degree(6)));
        Polyline::from_lines(&nl)
    }
}

fn remove_consecutive_parallel_lines(lines: &[Line]) -> Vec<Line> {
    if lines.len() < 3 {
        return lines.to_vec();
    }
    let mut tmp: Vec<Line> = Vec::with_capacity(lines.len());
    tmp.push(lines[0]);
    for l in &lines[1..] {
        if !tmp.last().unwrap().is_parallel(l) {
            tmp.push(*l);
        }
    }
    if tmp.len() == lines.len() {
        return tmp;
    }
    if tmp.len() < 3 {
        return Vec::new();
    }
    tmp
}

fn remove_overlaps(lines: &[Line]) -> Vec<Line> {
    let n = lines.len();
    if n < 4 {
        return lines.to_vec();
    }
    let mut tmp: Vec<Line> = vec![lines[0]; n];
    let mut new_length: usize = 0;
    tmp[0] = lines[0];
    if !lines[0].is_equal_or_opposite(&lines[2]) {
        new_length += 1;
    }
    tmp[new_length] = lines[1];
    new_length += 1;
    for i in 2..n - 2 {
        if new_length > 0 && tmp[new_length - 1].is_equal_or_opposite(&lines[i + 1]) {
            new_length -= 1;
        } else {
            tmp[new_length] = lines[i];
            new_length += 1;
        }
    }
    tmp[new_length] = lines[n - 2];
    new_length += 1;
    if new_length >= 2 && !lines[n - 1].is_equal_or_opposite(&tmp[new_length - 2]) {
        tmp[new_length] = lines[n - 1];
        new_length += 1;
    }
    if new_length == n {
        return lines.to_vec();
    }
    if new_length < 3 {
        return Vec::new();
    }
    tmp.truncate(new_length);
    tmp
}
