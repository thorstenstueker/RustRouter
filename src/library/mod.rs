//! Library of padstacks, packages and logical parts (port of `app.freerouting.core.library`).

use crate::geometry::{Area, Direction, FloatPoint, IntVector, Shape};
use regex_lite::Regex;
use std::sync::LazyLock;

/// 1-based id of a padstack as in the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PadstackId(pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackageId(pub i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LogicalPartId(pub i32);

#[derive(Clone, Debug)]
pub struct Padstack {
    pub name: String,
    pub id: PadstackId,
    pub attach_allowed: bool,
    pub placed_absolute: bool,
    /// Convex shapes (tile shapes or circles) per board layer.
    shapes: Vec<Option<Shape>>,
    auxiliary_shapes: Option<Vec<Option<Vec<Shape>>>>,
    pub has_non_convex_geometry: bool,
    pub hole_only: bool,
    drill_radius: std::sync::OnceLock<f64>,
}

impl Padstack {
    pub fn get_shape(&self, layer: i32) -> Option<&Shape> {
        if layer < 0 || layer as usize >= self.shapes.len() {
            log::warn!("Padstack.get_layer layer out of range");
            return None;
        }
        self.shapes[layer as usize].as_ref()
    }

    pub fn shapes(&self) -> &[Option<Shape>] {
        &self.shapes
    }

    pub fn get_auxiliary_shapes(&self, layer: i32) -> Option<&[Shape]> {
        let aux = self.auxiliary_shapes.as_ref()?;
        if layer < 0 || layer as usize >= aux.len() {
            return None;
        }
        aux[layer as usize].as_deref()
    }

    pub fn has_auxiliary_shapes(&self) -> bool {
        self.auxiliary_shapes
            .as_ref()
            .map(|a| a.iter().any(|x| x.as_ref().map(|v| !v.is_empty()).unwrap_or(false)))
            .unwrap_or(false)
    }

    pub fn has_non_convex(&self) -> bool {
        self.has_non_convex_geometry || self.has_auxiliary_shapes()
    }

    pub fn from_layer(&self) -> i32 {
        let mut r = 0;
        while (r as usize) < self.shapes.len() && self.shapes[r as usize].is_none() {
            r += 1;
        }
        r
    }

    pub fn to_layer(&self) -> i32 {
        let mut r = self.shapes.len() as i32 - 1;
        while r >= 0 && self.shapes[r as usize].is_none() {
            r -= 1;
        }
        r
    }

    pub fn board_layer_count(&self) -> i32 {
        self.shapes.len() as i32
    }

    fn smallest_radius(&self) -> f64 {
        let mut min = f64::MAX;
        for s in self.shapes.iter().flatten() {
            let b = s.bounding_box();
            let r = (b.width().min(b.height())) as f64 / 2.0;
            if r < min {
                min = r;
            }
        }
        if min == f64::MAX {
            0.0
        } else {
            min
        }
    }

    /// The drill radius, derived from the padstack name if possible.
    pub fn get_drill_radius(&self) -> f64 {
        *self.drill_radius.get_or_init(|| {
            if let Some(r) = self.drill_radius_from_name() {
                return r;
            }
            self.smallest_radius() * 0.45
        })
    }

    fn drill_radius_from_name(&self) -> Option<f64> {
        let name = &self.name;
        let colon = name.find(':')?;
        let after = &name[colon + 1..];
        let drill_str = match after.find('_') {
            Some(u) if u > 0 => &after[..u],
            _ => after,
        };
        let digits = |s: &str| s.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect::<String>();
        let drill_dia: f64 = digits(drill_str).parse().ok()?;
        let last_underscore = name[..colon].rfind('_')?;
        let outer_str = &name[last_underscore + 1..colon];
        let outer_dia: f64 = if outer_str.contains('x') {
            let parts: Vec<&str> = outer_str.split('x').collect();
            let w: f64 = digits(parts.first()?).parse().ok()?;
            let h: f64 = digits(parts.get(1)?).parse().ok()?;
            w.min(h)
        } else {
            digits(outer_str).parse().ok()?
        };
        if outer_dia > 0.0 {
            let actual = self.smallest_radius();
            if actual > 0.0 {
                return Some(actual * (drill_dia / outer_dia));
            }
        }
        None
    }

    pub fn get_trace_exit_directions(&self, layer: i32, factor: f64) -> Vec<Direction> {
        let mut result = Vec::new();
        if layer < 0 || layer as usize >= self.shapes.len() {
            return result;
        }
        let Some(shape) = &self.shapes[layer as usize] else {
            return result;
        };
        let is_regular = matches!(
            shape,
            Shape::Tile(crate::geometry::TileShape::Box(_)) | Shape::Tile(crate::geometry::TileShape::Octagon(_))
        );
        if !is_regular {
            return result;
        }
        let b = shape.bounding_box();
        let all_dirs = (b.width().max(b.height()) as f64) < factor * b.width().min(b.height()) as f64;
        if all_dirs || b.width() >= b.height() {
            result.push(Direction::RIGHT);
            result.push(Direction::LEFT);
        }
        if all_dirs || b.width() <= b.height() {
            result.push(Direction::UP);
            result.push(Direction::DOWN);
        }
        result
    }
}

#[derive(Clone, Debug, Default)]
pub struct Padstacks {
    padstacks: Vec<Padstack>,
    layer_count: usize,
}

impl Padstacks {
    pub fn new(layer_count: usize) -> Padstacks {
        Padstacks {
            padstacks: Vec::new(),
            layer_count,
        }
    }

    pub fn get_by_name(&self, name: &str) -> Option<PadstackId> {
        if let Some(p) = self.padstacks.iter().find(|p| p.name == name) {
            return Some(p.id);
        }
        self.padstacks.iter().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.id)
    }

    #[inline]
    pub fn get(&self, id: PadstackId) -> &Padstack {
        &self.padstacks[id.0 as usize - 1]
    }

    pub fn try_get(&self, id: PadstackId) -> Option<&Padstack> {
        if id.0 <= 0 || id.0 as usize > self.padstacks.len() {
            log::warn!("Padstacks.get: 1 <= padstackId <= {} expected", self.padstacks.len());
            return None;
        }
        Some(&self.padstacks[id.0 as usize - 1])
    }

    pub fn count(&self) -> usize {
        self.padstacks.len()
    }

    pub fn layer_count(&self) -> usize {
        self.layer_count
    }

    pub fn add(
        &mut self,
        name: &str,
        shapes: Vec<Option<Shape>>,
        auxiliary_shapes: Option<Vec<Option<Vec<Shape>>>>,
        drill_allowed: bool,
        placed_absolute: bool,
    ) -> PadstackId {
        let id = PadstackId(self.padstacks.len() as i32 + 1);
        self.padstacks.push(Padstack {
            name: name.to_string(),
            id,
            attach_allowed: drill_allowed,
            placed_absolute,
            shapes,
            auxiliary_shapes,
            has_non_convex_geometry: false,
            hole_only: false,
            drill_radius: std::sync::OnceLock::new(),
        });
        id
    }

    pub fn add_unnamed(&mut self, shapes: Vec<Option<Shape>>) -> PadstackId {
        let name = format!("padstack#{}", self.padstacks.len() + 1);
        self.add(&name, shapes, None, false, false)
    }

    pub fn add_shape_range(&mut self, shape: Shape, from_layer: i32, to_layer: i32) -> PadstackId {
        let mut shapes = vec![None; self.layer_count];
        let first = from_layer.max(0);
        let last = to_layer.min(self.layer_count as i32 - 1);
        for i in first..=last {
            shapes[i as usize] = Some(shape.clone());
        }
        self.add_unnamed(shapes)
    }

    pub fn get_mut(&mut self, id: PadstackId) -> &mut Padstack {
        &mut self.padstacks[id.0 as usize - 1]
    }

    pub fn iter(&self) -> impl Iterator<Item = &Padstack> {
        self.padstacks.iter()
    }
}

#[derive(Clone, Debug)]
pub struct PackagePin {
    pub name: String,
    pub padstack_id: PadstackId,
    pub relative_location: IntVector,
    pub rotation_in_degree: f64,
    exact_relative_location: FloatPoint,
}

impl PackagePin {
    pub fn new(name: &str, padstack_id: PadstackId, relative_location: FloatPoint, rotation_in_degree: f64) -> Self {
        let r = relative_location.round();
        PackagePin {
            name: name.to_string(),
            padstack_id,
            relative_location: IntVector::new(r.x, r.y),
            rotation_in_degree,
            exact_relative_location: relative_location,
        }
    }

    pub fn get_exact_relative_location(&self) -> FloatPoint {
        self.exact_relative_location
    }
}

#[derive(Clone, Debug)]
pub struct Keepout {
    pub name: String,
    pub area: Area,
    pub layer: i32,
}

#[derive(Clone, Debug)]
pub struct Package {
    pub name: String,
    pub id: PackageId,
    pins: Vec<PackagePin>,
    pub outline: Vec<Shape>,
    pub outline_widths: Vec<f64>,
    pub outline_is_closed: Vec<bool>,
    pub keepouts: Vec<Keepout>,
    pub via_keepouts: Vec<Keepout>,
    pub place_keepouts: Vec<Keepout>,
    pub is_front: bool,
}

impl Package {
    pub fn get_pin(&self, index: usize) -> Option<&PackagePin> {
        self.pins.get(index)
    }

    pub fn get_pin_index(&self, name: &str) -> i32 {
        self.pins.iter().position(|p| p.name == name).map(|i| i as i32).unwrap_or(-1)
    }

    pub fn pin_count(&self) -> usize {
        self.pins.len()
    }

    pub fn pins(&self) -> &[PackagePin] {
        &self.pins
    }
}

static SUFFIX_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"::\d+$").unwrap());

#[derive(Clone, Debug, Default)]
pub struct Packages {
    packages: Vec<Package>,
}

impl Packages {
    pub fn get_by_name(&self, name: &str, is_front: bool) -> Option<PackageId> {
        let mut other: Option<PackageId> = None;
        for p in &self.packages {
            if p.name == name {
                if p.is_front == is_front {
                    return Some(p.id);
                }
                other = Some(p.id);
            }
        }
        if other.is_some() {
            return other;
        }
        for p in &self.packages {
            if p.name.eq_ignore_ascii_case(name) {
                if p.is_front == is_front {
                    return Some(p.id);
                }
                other = Some(p.id);
            }
        }
        if other.is_some() {
            return other;
        }
        let base = SUFFIX_PATTERN.replace(name, "").to_string();
        if !base.eq_ignore_ascii_case(name) {
            for p in &self.packages {
                if p.name.eq_ignore_ascii_case(&base) {
                    if p.is_front == is_front {
                        log::warn!("Package '{name}' not found; falling back to base package '{base}'");
                        return Some(p.id);
                    }
                    other = Some(p.id);
                }
            }
            if other.is_some() {
                log::warn!("Package '{name}' not found; falling back to other-side base package '{base}'");
                return other;
            }
        }
        None
    }

    #[inline]
    pub fn get(&self, id: PackageId) -> &Package {
        &self.packages[id.0 as usize - 1]
    }

    pub fn count(&self) -> usize {
        self.packages.len()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        name: &str,
        pins: Vec<PackagePin>,
        outline: Vec<Shape>,
        outline_widths: Vec<f64>,
        outline_is_closed: Vec<bool>,
        keepouts: Vec<Keepout>,
        via_keepouts: Vec<Keepout>,
        place_keepouts: Vec<Keepout>,
        is_front: bool,
    ) -> PackageId {
        let id = PackageId(self.packages.len() as i32 + 1);
        self.packages.push(Package {
            name: name.to_string(),
            id,
            pins,
            outline,
            outline_widths,
            outline_is_closed,
            keepouts,
            via_keepouts,
            place_keepouts,
            is_front,
        });
        id
    }

    pub fn add_unnamed(&mut self, pins: Vec<PackagePin>) -> PackageId {
        let name = format!("Package#{}", self.packages.len() + 1);
        self.add(&name, pins, Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), true)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }
}

#[derive(Clone, Debug)]
pub struct PartPin {
    pub pin_index: i32,
    pub pin_name: String,
    pub gate_name: String,
    pub gate_swap_code: i32,
    pub gate_pin_name: String,
    pub gate_pin_swap_code: i32,
}

#[derive(Clone, Debug)]
pub struct LogicalPart {
    pub name: String,
    pub id: LogicalPartId,
    pins: Vec<PartPin>,
}

impl LogicalPart {
    pub fn pin_count(&self) -> usize {
        self.pins.len()
    }

    pub fn get_pin(&self, i: usize) -> Option<&PartPin> {
        self.pins.get(i)
    }
}

#[derive(Clone, Debug, Default)]
pub struct LogicalParts {
    parts: Vec<LogicalPart>,
}

impl LogicalParts {
    pub fn add(&mut self, name: &str, pins: Vec<PartPin>) -> LogicalPartId {
        let id = LogicalPartId(self.parts.len() as i32 + 1);
        self.parts.push(LogicalPart {
            name: name.to_string(),
            id,
            pins,
        });
        id
    }

    pub fn get(&self, id: LogicalPartId) -> &LogicalPart {
        &self.parts[id.0 as usize - 1]
    }

    pub fn get_by_name(&self, name: &str) -> Option<LogicalPartId> {
        self.parts.iter().find(|p| p.name == name).map(|p| p.id)
    }

    pub fn count(&self) -> usize {
        self.parts.len()
    }
}

#[derive(Clone, Debug, Default)]
pub struct BoardLibrary {
    pub padstacks: Padstacks,
    pub packages: Packages,
    pub logical_parts: LogicalParts,
    via_padstacks: Vec<PadstackId>,
}

impl BoardLibrary {
    pub fn new(padstacks: Padstacks, packages: Packages) -> BoardLibrary {
        BoardLibrary {
            padstacks,
            packages,
            logical_parts: LogicalParts::default(),
            via_padstacks: Vec::new(),
        }
    }

    pub fn via_padstack_count(&self) -> usize {
        self.via_padstacks.len()
    }

    pub fn get_via_padstack(&self, no: usize) -> Option<PadstackId> {
        self.via_padstacks.get(no).copied()
    }

    pub fn get_via_padstack_by_name(&self, name: &str) -> Option<PadstackId> {
        self.via_padstacks
            .iter()
            .copied()
            .find(|p| self.padstacks.get(*p).name == name)
    }

    pub fn get_via_padstacks(&self) -> &[PadstackId] {
        &self.via_padstacks
    }

    pub fn set_via_padstacks(&mut self, ps: Vec<PadstackId>) {
        self.via_padstacks = ps;
    }

    pub fn add_via_padstack(&mut self, ps: PadstackId) -> bool {
        if self.get_via_padstack_by_name(&self.padstacks.get(ps).name.clone()).is_some() {
            return false;
        }
        self.via_padstacks.push(ps);
        true
    }

    pub fn get_mirrored_via_padstack(&self, ps: PadstackId) -> Option<PadstackId> {
        let layer_count = self.padstacks.layer_count() as i32;
        let p = self.padstacks.get(ps);
        if p.from_layer() == 0 && p.to_layer() == layer_count - 1 {
            return Some(ps);
        }
        let nf = layer_count - p.to_layer() - 1;
        let nt = layer_count - p.from_layer() - 1;
        self.via_padstacks.iter().copied().find(|v| {
            let vp = self.padstacks.get(*v);
            vp.from_layer() == nf && vp.to_layer() == nt
        })
    }
}
