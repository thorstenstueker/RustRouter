//! Convex shapes defined as intersection of half planes (to the right of their lines).

use super::basics::{Side, CRIT_INT};
use super::{Direction, FloatPoint, IntBox, IntDirection, IntOctagon, IntPoint, Line, Point, Vector};
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
struct SimplexInner {
    lines: Vec<Line>,
    corners: OnceLock<Box<[Point]>>,
    float_corners: OnceLock<Box<[FloatPoint]>>,
    bounding_box: OnceLock<IntBox>,
    bounding_octagon: OnceLock<Option<IntOctagon>>,
}

/// A convex shape given by its border lines sorted by direction angle.
/// The lines are kept in an `Arc`, so cloning is cheap.
#[derive(Clone, Debug)]
pub struct Simplex(Arc<SimplexInner>);

static EMPTY_SIMPLEX: OnceLock<Simplex> = OnceLock::new();

impl Simplex {
    pub fn new(lines: Vec<Line>) -> Simplex {
        Simplex(Arc::new(SimplexInner {
            lines,
            corners: OnceLock::new(),
            float_corners: OnceLock::new(),
            bounding_box: OnceLock::new(),
            bounding_octagon: OnceLock::new(),
        }))
    }

    pub fn empty() -> Simplex {
        EMPTY_SIMPLEX.get_or_init(|| Simplex::new(Vec::new())).clone()
    }

    /// Creates a normalized simplex from arbitrary lines.
    pub fn get_instance(lines: &[Line]) -> Simplex {
        if lines.is_empty() {
            return Simplex::empty();
        }
        let mut arr = lines.to_vec();
        arr.sort_by(|a, b| a.compare_to(b));
        Simplex::new(arr).remove_redundant_lines()
    }

    #[inline]
    pub fn lines(&self) -> &[Line] {
        &self.0.lines
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.lines.is_empty()
    }

    pub fn get_id(&self) -> i32 {
        let mut r: i32 = 0;
        for l in &self.0.lines {
            r = r.wrapping_mul(31).wrapping_add(l.get_id());
        }
        r
    }

    fn clamp_index(&self, i: usize) -> usize {
        let n = self.0.lines.len();
        if i >= n {
            n.saturating_sub(1)
        } else {
            i
        }
    }

    pub fn corner_is_bounded(&self, corner_index: usize) -> bool {
        let lines = &self.0.lines;
        let no = self.clamp_index(corner_index);
        if lines.len() == 1 {
            return false;
        }
        let prev_no = if no == 0 { lines.len() - 1 } else { no - 1 };
        let prev_dir = lines[prev_no].direction().get_vector();
        let cur_dir = lines[no].direction().get_vector();
        prev_dir.determinant(&cur_dir) > 0
    }

    pub fn is_bounded(&self) -> bool {
        let n = self.0.lines.len();
        if n == 0 {
            return true;
        }
        if n < 3 {
            return false;
        }
        (0..n).all(|i| self.corner_is_bounded(i))
    }

    #[inline]
    pub fn border_line_count(&self) -> usize {
        self.0.lines.len()
    }

    fn corners(&self) -> &[Point] {
        self.0.corners.get_or_init(|| {
            let lines = &self.0.lines;
            let n = lines.len();
            (0..n)
                .map(|no| {
                    let prev = if no == 0 { &lines[n - 1] } else { &lines[no - 1] };
                    lines[no].intersection(prev)
                })
                .collect()
        })
    }

    fn float_corners(&self) -> &[FloatPoint] {
        self.0.float_corners.get_or_init(|| {
            let lines = &self.0.lines;
            let n = lines.len();
            (0..n)
                .map(|no| {
                    let prev = if no == 0 { &lines[n - 1] } else { &lines[no - 1] };
                    lines[no].intersection_approx(prev)
                })
                .collect()
        })
    }

    /// The intersection of the border line `no` with the previous border line.
    pub fn corner(&self, corner_index: usize) -> Point {
        let no = self.clamp_index(corner_index);
        self.corners()[no].clone()
    }

    pub fn corner_ref(&self, corner_index: usize) -> &Point {
        let no = self.clamp_index(corner_index);
        &self.corners()[no]
    }

    pub fn corner_approx(&self, corner_index: usize) -> FloatPoint {
        if self.0.lines.is_empty() {
            return FloatPoint::ZERO;
        }
        let no = self.clamp_index(corner_index);
        self.float_corners()[no]
    }

    pub fn corner_approx_arr(&self) -> Vec<FloatPoint> {
        if self.0.lines.is_empty() {
            return Vec::new();
        }
        self.float_corners().to_vec()
    }

    pub fn border_line(&self, edge_index: usize) -> Line {
        let no = self.clamp_index(edge_index);
        self.0.lines[no]
    }

    pub fn dimension(&self) -> i32 {
        let lines = &self.0.lines;
        let n = lines.len();
        if n == 0 {
            return -1;
        }
        if n > 4 || n == 1 {
            return 2;
        }
        if n == 2 {
            return if lines[0].overlaps(&lines[1]) { 1 } else { 2 };
        }
        if n == 3 {
            if lines[0].overlaps(&lines[1]) || lines[0].overlaps(&lines[2]) || lines[1].overlaps(&lines[2]) {
                return 1;
            }
            let is = lines[1].intersection(&lines[2]);
            let side = lines[0].side_of(&is);
            if side == Side::OnTheRight {
                return 2;
            }
            if side == Side::OnTheLeft {
                return -1;
            }
            return 0;
        }
        let c02 = lines[0].overlaps(&lines[2]);
        let c13 = lines[1].overlaps(&lines[3]);
        if c02 && c13 {
            return 0;
        }
        if c02 || c13 {
            return 1;
        }
        2
    }

    pub fn centre_of_gravity(&self) -> FloatPoint {
        let n = self.border_line_count();
        let mut x = 0.0;
        let mut y = 0.0;
        for i in 0..n {
            let c = self.corner_approx(i);
            x += c.x;
            y += c.y;
        }
        FloatPoint::new(x / n as f64, y / n as f64)
    }

    pub fn max_width(&self) -> f64 {
        if !self.is_bounded() {
            return i32::MAX as f64;
        }
        let mut md = i32::MIN as f64;
        let mut md2 = i32::MIN as f64;
        let g = self.centre_of_gravity();
        for l in &self.0.lines {
            let d = l.signed_distance(&g).abs();
            if d > md {
                md2 = md;
                md = d;
            } else if d > md2 {
                md2 = d;
            }
        }
        md + md2
    }

    pub fn min_width(&self) -> f64 {
        if !self.is_bounded() {
            return i32::MAX as f64;
        }
        let mut md = i32::MAX as f64;
        let mut md2 = i32::MAX as f64;
        let g = self.centre_of_gravity();
        for l in &self.0.lines {
            let d = l.signed_distance(&g).abs();
            if d < md {
                md2 = md;
                md = d;
            } else if d < md2 {
                md2 = d;
            }
        }
        md + md2
    }

    pub fn is_int_box(&self) -> bool {
        for (i, l) in self.0.lines.iter().enumerate() {
            if !l.is_orthogonal() {
                return false;
            }
            if !self.corner_is_bounded(i) {
                return false;
            }
        }
        true
    }

    pub fn is_int_octagon(&self) -> bool {
        for (i, l) in self.0.lines.iter().enumerate() {
            if !l.is_multiple_of_45_degree() {
                return false;
            }
            if !self.corner_is_bounded(i) {
                return false;
            }
        }
        true
    }

    pub fn to_int_octagon(&self) -> Option<IntOctagon> {
        if !self.is_int_octagon() {
            return None;
        }
        if self.is_empty() {
            return Some(IntOctagon::EMPTY);
        }
        let mut rx = CRIT_INT;
        let mut uy = CRIT_INT;
        let mut lrx = CRIT_INT;
        let mut urx = CRIT_INT;
        let mut lx = -CRIT_INT;
        let mut ly = -CRIT_INT;
        let mut llx = -CRIT_INT;
        let mut ulx = -CRIT_INT;
        for l in &self.0.lines {
            let (a, b) = (l.a, l.b);
            if a.y == b.y {
                if b.x >= a.x {
                    ly = a.y;
                }
                if b.x <= a.x {
                    uy = a.y;
                }
            }
            if a.x == b.x {
                if b.y >= a.y {
                    rx = a.x;
                }
                if b.y <= a.y {
                    lx = a.x;
                }
            }
            if a.y < b.y {
                if a.x < b.x {
                    lrx = a.x - a.y;
                } else if a.x > b.x {
                    urx = a.x + a.y;
                }
            } else if a.y > b.y {
                if a.x < b.x {
                    llx = a.x + a.y;
                } else if a.x > b.x {
                    ulx = a.x - a.y;
                }
            }
        }
        Some(IntOctagon::new(lx, ly, rx, uy, ulx, lrx, llx, urx).normalize())
    }

    pub fn translate_by(&self, v: &Vector) -> Simplex {
        if v.is_zero() {
            return self.clone();
        }
        Simplex::new(self.0.lines.iter().map(|l| l.translate_by(v)).collect())
    }

    pub fn bounding_box(&self) -> IntBox {
        if self.0.lines.is_empty() {
            return IntBox::EMPTY;
        }
        *self.0.bounding_box.get_or_init(|| {
            let mut llx = i32::MAX as f64;
            let mut lly = i32::MAX as f64;
            let mut urx = i32::MIN as f64;
            let mut ury = i32::MIN as f64;
            for c in self.float_corners() {
                llx = llx.min(c.x);
                lly = lly.min(c.y);
                urx = urx.max(c.x);
                ury = ury.max(c.y);
            }
            IntBox::new(llx.floor() as i32, lly.floor() as i32, urx.ceil() as i32, ury.ceil() as i32)
        })
    }

    /// Returns None, if the bounding octagon exceeds the integer limits.
    pub fn bounding_octagon(&self) -> Option<IntOctagon> {
        *self.0.bounding_octagon.get_or_init(|| {
            let mut lx = i32::MAX as f64;
            let mut ly = i32::MAX as f64;
            let mut rx = i32::MIN as f64;
            let mut uy = i32::MIN as f64;
            let mut ulx = i32::MAX as f64;
            let mut lrx = i32::MIN as f64;
            let mut llx = i32::MAX as f64;
            let mut urx = i32::MIN as f64;
            for c in self.float_corners() {
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
            }
            let crit = CRIT_INT as f64;
            if lx.min(ly) < -crit || rx.max(uy) > crit || ulx.min(llx) < -crit || lrx.max(urx) > crit {
                return None;
            }
            Some(IntOctagon::new(
                lx.floor() as i32,
                ly.floor() as i32,
                rx.ceil() as i32,
                uy.ceil() as i32,
                ulx.floor() as i32,
                lrx.ceil() as i32,
                llx.floor() as i32,
                urx.ceil() as i32,
            ))
        })
    }

    pub fn offset(&self, width: f64) -> Simplex {
        if width == 0.0 {
            return self.clone();
        }
        let s = Simplex::new(self.0.lines.iter().map(|l| l.translate(-width)).collect());
        if width < 0.0 {
            s.remove_redundant_lines()
        } else {
            s
        }
    }

    pub fn enlarge(&self, offset: f64) -> Simplex {
        if offset == 0.0 {
            return self.clone();
        }
        let os = self.offset(offset);
        let Some(bo) = self.bounding_octagon() else {
            return Simplex::empty();
        };
        let oo = bo.offset(offset);
        os.intersection(&oo.to_simplex())
    }

    pub fn index_of_right_most_corner(&self, from: &Point) -> usize {
        let mut rmc = self.corner(0);
        let mut result = 0;
        for i in 1..self.0.lines.len() {
            let c = self.corner(i);
            if c.side_of(from, &rmc) == Side::OnTheRight {
                rmc = c;
                result = i;
            }
        }
        result
    }

    pub fn intersection(&self, other: &Simplex) -> Simplex {
        if self.is_empty() || other.is_empty() {
            return Simplex::empty();
        }
        let mut arr = Vec::with_capacity(self.0.lines.len() + other.0.lines.len());
        arr.extend_from_slice(&self.0.lines);
        arr.extend_from_slice(&other.0.lines);
        arr.sort_by(|a, b| a.compare_to(b));
        Simplex::new(arr).remove_redundant_lines()
    }

    pub fn intersects(&self, other: &Simplex) -> bool {
        !self.intersection(other).is_empty()
    }

    pub fn border_line_index(&self, line: &Line) -> i32 {
        for (i, l) in self.0.lines.iter().enumerate() {
            if line == l {
                return i as i32;
            }
        }
        -1
    }

    pub fn remove_border_line(&self, no: usize) -> Simplex {
        if no >= self.0.lines.len() {
            return self.clone();
        }
        let mut v = self.0.lines.clone();
        v.remove(no);
        Simplex::new(v)
    }

    /// Cuts this simplex out of `outer`. Returns None, if this simplex is not 2-dimensional.
    pub fn cutout_from(&self, outer: &Simplex) -> Option<Vec<Simplex>> {
        if self.dimension() < 2 {
            log::warn!("Simplex.cutout_from only implemented for 2-dim simplex");
            return None;
        }
        let inner = self.intersection(outer);
        if inner.dimension() < 2 {
            return Some(vec![outer.clone()]);
        }
        let inner_corner_count = inner.0.lines.len();
        let mut division_lines: Vec<Vec<Line>> = Vec::with_capacity(inner_corner_count);
        for no in 0..inner_corner_count {
            match inner.calc_division_lines(no, outer) {
                Some(d) => division_lines.push(d),
                None => {
                    log::warn!("Simplex.cutout_from: division line is null");
                    return Some(vec![outer.clone()]);
                }
            }
        }
        let mut check_cross_first_line = false;
        let prev_division_line: Option<Line> = None;
        let first_division_line = division_lines[0][0];
        let first_direction = first_division_line.direction();
        let mut result = Vec::new();
        for inner_corner_no in 0..inner_corner_count {
            let next_corner_no = (inner_corner_no + 1) % inner_corner_count;
            let next_division_line = division_lines[next_corner_no][0];
            let cur = &division_lines[inner_corner_no];
            if cur.len() == 2 {
                let cur_dir = cur[0].direction();
                let mut merge_prev = false;
                let mut merge_first = false;
                if let Some(pdl) = prev_division_line {
                    if cur_dir.determinant(&pdl.direction()) > 0.0 {
                        merge_prev = true;
                    }
                }
                if !check_cross_first_line {
                    check_cross_first_line =
                        inner_corner_no > 0 && cur_dir.determinant(&first_direction) > 0.0;
                }
                if check_cross_first_line && cur[1].direction().determinant(&first_direction) < 0.0 {
                    merge_first = true;
                }
                let mut piece = vec![Line::new(cur[1].b, cur[1].a), cur[0]];
                if merge_prev {
                    piece.push(prev_division_line.unwrap());
                }
                if merge_first {
                    piece.push(Line::new(first_division_line.b, first_division_line.a));
                }
                result.push(Simplex::new(piece).intersection(outer));
            }
            let merge_next = next_division_line.b != next_division_line.a;
            let last_cur = cur[cur.len() - 1];
            let last_cur_dir = last_cur.direction();
            let merge_last_cur = last_cur.b != last_cur.a;
            let mut merge_prev = false;
            let mut merge_first = false;
            if let Some(pdl) = prev_division_line {
                if last_cur_dir.determinant(&pdl.direction()) > 0.0 {
                    merge_prev = true;
                }
            }
            if !check_cross_first_line {
                check_cross_first_line = inner_corner_no > 0
                    && last_cur_dir.determinant(&first_direction) > 0.0
                    && last_cur_dir.get_vector().scalar_product(&first_direction.get_vector()) < 0.0;
            }
            if check_cross_first_line && next_division_line.direction().determinant(&first_direction) < 0.0 {
                merge_first = true;
            }
            let cl = inner.0.lines[inner_corner_no];
            let mut piece = vec![Line::new(cl.b, cl.a)];
            if merge_next {
                piece.push(Line::new(next_division_line.b, next_division_line.a));
            }
            if merge_last_cur {
                piece.push(last_cur);
            }
            if merge_prev {
                piece.push(prev_division_line.unwrap());
            }
            if merge_first {
                piece.push(Line::new(first_division_line.b, first_division_line.a));
            }
            result.push(Simplex::new(piece).intersection(outer));
            // In the original `nextDivisionLine = prevDivisionLine;` has no effect,
            // prevDivisionLine stays null.
        }
        Some(result)
    }

    /// Removes lines, which are redundant for the definition of the shape.
    pub fn remove_redundant_lines(&self) -> Simplex {
        let src = &self.0.lines;
        if src.is_empty() {
            return self.clone();
        }
        let mut lines: Vec<Line> = Vec::with_capacity(src.len());
        lines.push(src[0]);
        let mut prev = src[0];
        for l in &src[1..] {
            if !l.fast_equals(&prev) {
                lines.push(*l);
                prev = *l;
            }
        }
        let mut new_length = lines.len();
        let mut intersection_sides: Vec<Option<Side>> = vec![None; new_length];
        let mut try_again = new_length > 2;
        let mut index_of_last_removed_line = new_length as isize;
        while try_again {
            try_again = false;
            let mut prev_ind = new_length - 1;
            let mut prev_line = lines[prev_ind];
            let mut current_line = lines[0];
            let mut ind: isize = 0;
            while (ind as usize) < new_length {
                let next_ind = if ind as usize == new_length - 1 { 0 } else { ind as usize + 1 };
                let next_line = lines[next_ind];
                let mut remove_line = false;
                let prev_dir = prev_line.direction();
                let next_dir = next_line.direction();
                let det = prev_dir.determinant(&next_dir);
                let u = ind as usize;
                if det != 0.0 {
                    if intersection_sides[u].is_none() {
                        intersection_sides[u] = Some(current_line.side_of_intersection(&prev_line, &next_line));
                    }
                    if det > 0.0 {
                        remove_line = intersection_sides[u] != Some(Side::OnTheLeft);
                    } else if intersection_sides[u] == Some(Side::OnTheLeft) {
                        let cur_dir = current_line.direction();
                        if prev_dir.determinant(&cur_dir) > 0.0 {
                            new_length = 0;
                            try_again = false;
                            break;
                        }
                    }
                } else if prev_line.side_of_int(&next_line.a) == Side::OnTheLeft {
                    new_length = 0;
                    try_again = false;
                    break;
                }
                if remove_line {
                    try_again = true;
                    new_length -= 1;
                    for i in u..new_length {
                        lines[i] = lines[i + 1];
                        intersection_sides[i] = intersection_sides[i + 1];
                    }
                    if new_length < 3 {
                        try_again = false;
                        break;
                    }
                    if u == 0 {
                        prev_ind = new_length - 1;
                    }
                    intersection_sides[prev_ind] = None;
                    let ni = if u >= new_length { 0 } else { u };
                    intersection_sides[ni] = None;
                    ind -= 1;
                    index_of_last_removed_line = ind;
                } else {
                    prev_line = current_line;
                    prev_ind = u;
                }
                current_line = next_line;
                if !try_again && ind >= index_of_last_removed_line {
                    break;
                }
                ind += 1;
            }
        }
        if new_length == 2 && lines[0].is_parallel(&lines[1]) {
            if lines[0].direction() == lines[1].direction() {
                if lines[1].side_of_int(&lines[0].a) == Side::OnTheLeft {
                    lines[0] = lines[1];
                }
                new_length -= 1;
            } else if lines[1].side_of_int(&lines[0].a) == Side::OnTheLeft {
                new_length = 0;
            }
        }
        if new_length == src.len() {
            return self.clone();
        }
        if new_length == 0 {
            return Simplex::empty();
        }
        lines.truncate(new_length);
        Simplex::new(lines)
    }

    fn calc_division_lines(&self, inner_corner_no: usize, outer: &Simplex) -> Option<Vec<Line>> {
        let lines = &self.0.lines;
        let current_inner_line = lines[inner_corner_no];
        let prev_inner_line = if inner_corner_no != 0 {
            lines[inner_corner_no - 1]
        } else {
            lines[lines.len() - 1]
        };
        let intersection = current_inner_line.intersection_approx(&prev_inner_line);
        if intersection.x >= i32::MAX as f64 {
            log::warn!("Simplex.calc_division_lines: intersection expected");
            return None;
        }
        let inner_corner = intersection.round();
        let tol = 0.0001;
        let is_exact = (inner_corner.x as f64 - intersection.x).abs() < tol
            && (inner_corner.y as f64 - intersection.y).abs() < tol;
        if !is_exact {
            return Some(vec![prev_inner_line]);
        }
        let inner_corner_p = Point::Int(inner_corner);
        let mut first_projection_dir = IntDirection::NULL;
        let mut second_projection_dir = IntDirection::NULL;
        let prev_inner_dir = prev_inner_line.direction().opposite();
        let next_inner_dir = current_inner_line.direction();
        let outer_lines = outer.lines();
        let mut outer_line_no = 0;
        let mut min_distance = i32::MAX as f64;
        for _ in 0..outer_lines.len() {
            let outer_line = outer_lines[outer_line_no];
            let cpd = perpendicular_int_direction(&inner_corner_p, &outer_line);
            if cpd == IntDirection::NULL {
                return Some(vec![Line::new(inner_corner, inner_corner)]);
            }
            let projection_visible = prev_inner_dir.determinant(&cpd) >= 0.0;
            if projection_visible {
                let mut current_distance = outer_line.signed_distance(&inner_corner.to_float()).abs();
                let second_division_necessary = cpd.determinant(&next_inner_dir) < 0.0;
                let mut current_second_projection_dir = cpd;
                if second_division_necessary {
                    let mut second_visible = false;
                    let mut tmp_no = outer_line_no;
                    while !second_visible {
                        if tmp_no == outer_lines.len() - 1 {
                            tmp_no = 0;
                        } else {
                            tmp_no += 1;
                        }
                        current_second_projection_dir =
                            perpendicular_int_direction(&inner_corner_p, &outer_lines[tmp_no]);
                        if current_second_projection_dir == IntDirection::NULL {
                            return Some(vec![Line::new(inner_corner, inner_corner)]);
                        }
                        if cpd.determinant(&current_second_projection_dir) < 0.0 {
                            current_distance = i32::MAX as f64;
                            break;
                        }
                        second_visible = current_second_projection_dir.determinant(&next_inner_dir) >= 0.0;
                    }
                    current_distance += outer_lines[tmp_no].signed_distance(&inner_corner.to_float()).abs();
                }
                if current_distance < min_distance {
                    min_distance = current_distance;
                    first_projection_dir = cpd;
                    second_projection_dir = current_second_projection_dir;
                }
            }
            if outer_line_no == outer_lines.len() - 1 {
                outer_line_no = 0;
            } else {
                outer_line_no += 1;
            }
        }
        if min_distance == i32::MAX as f64 {
            log::warn!("Simplex.calc_division_lines: division not found");
            return None;
        }
        if Direction::Int(first_projection_dir) == Direction::Int(second_projection_dir) {
            Some(vec![Line::from_point_dir(inner_corner, &first_projection_dir)])
        } else {
            Some(vec![
                Line::from_point_dir(inner_corner, &first_projection_dir),
                Line::from_point_dir(inner_corner, &second_projection_dir),
            ])
        }
    }
}

/// `Point.perpendicularDirection(line)` restricted to integer directions; NULL if collinear.
fn perpendicular_int_direction(p: &Point, line: &Line) -> IntDirection {
    let side = p.side_of_line(line);
    if side == Side::Collinear {
        return IntDirection::NULL;
    }
    if side == Side::OnTheRight {
        line.direction().turn_45_degree(2)
    } else {
        line.direction().turn_45_degree(6)
    }
}

#[allow(dead_code)]
fn _unused(_: IntPoint) {}
