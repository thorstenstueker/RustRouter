//! Board structure: layers, units, communication settings and components.

use crate::geometry::{FloatPoint, IntPoint, PlacementTransform, Point, Vector};
use crate::library::{LogicalPartId, PackageId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum FixedState {
    #[default]
    Unfixed,
    ShoveFixed,
    UserFixed,
    SystemFixed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub is_signal: bool,
}

impl Layer {
    pub fn new(name: &str, is_signal: bool) -> Layer {
        Layer {
            name: name.to_string(),
            is_signal,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerStructure {
    pub layers: Vec<Layer>,
}

impl LayerStructure {
    pub fn new(layers: Vec<Layer>) -> LayerStructure {
        LayerStructure { layers }
    }

    pub fn count(&self) -> usize {
        self.layers.len()
    }

    pub fn get_no(&self, name: &str) -> i32 {
        self.layers.iter().position(|l| l.name == name).map(|i| i as i32).unwrap_or(-1)
    }

    pub fn signal_layer_count(&self) -> usize {
        self.layers.iter().filter(|l| l.is_signal).count()
    }

    /// The layer index of the signal layer with signal index `no`.
    pub fn get_layer_no(&self, signal_layer_no: usize) -> usize {
        let mut found = 0;
        for (i, l) in self.layers.iter().enumerate() {
            if l.is_signal {
                if signal_layer_no == found {
                    return i;
                }
                found += 1;
            }
        }
        self.layers.len() - 1
    }

    pub fn get_signal_layer_no(&self, layer: usize) -> i32 {
        let mut found = 0;
        for (i, l) in self.layers.iter().enumerate() {
            if i == layer {
                return found;
            }
            if l.is_signal {
                found += 1;
            }
        }
        -1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Mil,
    Inch,
    Mm,
    Um,
}

impl Unit {
    pub fn micrometers(self) -> f64 {
        match self {
            Unit::Mil => 25.4,
            Unit::Inch => 25_400.0,
            Unit::Mm => 1000.0,
            Unit::Um => 1.0,
        }
    }

    pub fn scale(value: f64, from: Unit, to: Unit) -> f64 {
        value * from.micrometers() / to.micrometers()
    }

    pub fn from_str_opt(s: &str) -> Option<Unit> {
        match s.to_ascii_lowercase().as_str() {
            "mil" => Some(Unit::Mil),
            "inch" => Some(Unit::Inch),
            "mm" => Some(Unit::Mm),
            "um" => Some(Unit::Um),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Unit::Mil => "mil",
            Unit::Inch => "inch",
            Unit::Mm => "mm",
            Unit::Um => "um",
        }
    }
}

/// Transformation between DSN file coordinates and board coordinates.
#[derive(Clone, Copy, Debug)]
pub struct CoordinateTransform {
    pub scale_factor: f64,
    pub base_x: f64,
    pub base_y: f64,
}

impl CoordinateTransform {
    pub fn new(user_unit_factor: f64, base_x: f64, base_y: f64) -> CoordinateTransform {
        CoordinateTransform {
            scale_factor: user_unit_factor,
            base_x,
            base_y,
        }
    }

    pub fn dsn_to_board(&self, value: f64) -> f64 {
        value * self.scale_factor
    }

    pub fn board_to_dsn(&self, value: f64) -> f64 {
        value / self.scale_factor
    }
}

#[derive(Clone, Debug, Default)]
pub struct WriteResolution {
    pub char_name: String,
    pub positive_int: i32,
}

#[derive(Clone, Debug, Default)]
pub struct SpecctraParserInfo {
    pub string_quote: String,
    pub host_cad: Option<String>,
    pub host_version: Option<String>,
    pub constants: Vec<(String, String)>,
    pub write_resolution: Option<WriteResolution>,
    pub dsn_file_generated_by_host: bool,
}

/// Communication information with the host system (units, resolution, id generator).
#[derive(Clone, Debug)]
pub struct Communication {
    pub coordinate_transform: Option<CoordinateTransform>,
    pub unit: Unit,
    pub resolution: i32,
    pub specctra_parser_info: SpecctraParserInfo,
    pub id_generator: IdGenerator,
}

impl Default for Communication {
    fn default() -> Self {
        Communication {
            coordinate_transform: Some(CoordinateTransform::new(1.0, 0.0, 0.0)),
            unit: Unit::Mil,
            resolution: 1,
            specctra_parser_info: SpecctraParserInfo {
                string_quote: "\"".to_string(),
                ..Default::default()
            },
            id_generator: IdGenerator::default(),
        }
    }
}

impl Communication {
    pub fn host_cad_is_fusion(&self) -> bool {
        match &self.specctra_parser_info.host_cad {
            Some(c) => {
                let c = c.to_lowercase();
                c.contains("fusion") || c.contains("cadsoft") || c.contains("eagle")
            }
            None => false,
        }
    }

    pub fn host_cad_is_kicad(&self) -> bool {
        self.specctra_parser_info
            .host_cad
            .as_ref()
            .map(|c| c.to_lowercase().contains("kicad"))
            .unwrap_or(false)
    }

    pub fn host_is_old_kicad(&self) -> bool {
        let info = &self.specctra_parser_info;
        let (Some(cad), Some(ver)) = (&info.host_cad, &info.host_version) else {
            return false;
        };
        if cad.to_lowercase().contains("kicad") {
            let digits: String = ver
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(v) = digits.parse::<i32>() {
                return v <= 5;
            }
        }
        false
    }

    pub fn host_cad_exists(&self) -> bool {
        self.specctra_parser_info.host_cad.is_some()
    }

    pub fn get_resolution(&self, unit: Unit) -> f64 {
        match &self.coordinate_transform {
            Some(t) => Unit::scale(t.dsn_to_board(1.0), unit, self.unit),
            None => Unit::scale(self.resolution as f64, unit, self.unit),
        }
    }

    pub fn physical_to_board(&self, value: f64, unit: Unit) -> f64 {
        match &self.coordinate_transform {
            Some(t) => Unit::scale(value * t.dsn_to_board(1.0), unit, self.unit),
            None => Unit::scale(value * self.resolution as f64, unit, self.unit),
        }
    }
}

/// Generator of unique item ids.
#[derive(Clone, Debug, Default)]
pub struct IdGenerator {
    last_id: i32,
}

impl IdGenerator {
    pub fn new_id(&mut self) -> i32 {
        self.last_id += 1;
        if super::snapshot::evlog::id_log_enabled() {
            super::snapshot::evlog::line(&format!("ID {}", self.last_id));
        }
        static IDSTACK: std::sync::OnceLock<Vec<i32>> = std::sync::OnceLock::new();
        let ids = IDSTACK.get_or_init(|| {
            std::env::var("RR_IDSTACK").map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default()
        });
        if !ids.is_empty() {
            if ids.contains(&self.last_id) {
                eprintln!("IDSTACK {} {}", self.last_id, std::backtrace::Backtrace::force_capture());
            }
        }
        self.last_id
    }

    pub fn max_generated_id(&self) -> i32 {
        self.last_id
    }
}

#[derive(Clone, Debug)]
pub struct Component {
    pub name: String,
    pub id: i32,
    pub position_fixed: bool,
    package_front: PackageId,
    package_back: PackageId,
    part_number: Option<String>,
    location: Option<IntPoint>,
    exact_location: Option<FloatPoint>,
    rotation_in_degree: f64,
    logical_part: Option<LogicalPartId>,
    on_front: bool,
}

fn normalize_degree(mut r: f64) -> f64 {
    while r >= 360.0 {
        r -= 360.0;
    }
    while r < 0.0 {
        r += 360.0;
    }
    r
}

impl Component {
    pub fn get_location(&self) -> Option<IntPoint> {
        match self.exact_location {
            Some(e) => Some(e.round()),
            None => self.location,
        }
    }

    pub fn get_exact_location(&self) -> Option<FloatPoint> {
        match self.exact_location {
            Some(e) => Some(e),
            None => self.location.map(|l| l.to_float()),
        }
    }

    pub fn placement_transform(&self, rotate_first: bool) -> PlacementTransform {
        PlacementTransform::new(
            self.get_exact_location().unwrap_or(FloatPoint::ZERO),
            self.rotation_in_degree,
            !self.on_front,
            rotate_first,
        )
    }

    pub fn get_rotation_in_degree(&self) -> f64 {
        self.rotation_in_degree
    }

    pub fn is_placed(&self) -> bool {
        self.exact_location.is_some() || self.location.is_some()
    }

    pub fn placed_on_front(&self) -> bool {
        self.on_front
    }

    pub fn get_package(&self) -> PackageId {
        if self.on_front {
            self.package_front
        } else {
            self.package_back
        }
    }

    pub fn get_part_number(&self) -> Option<&str> {
        self.part_number.as_deref()
    }

    pub fn get_logical_part(&self) -> Option<LogicalPartId> {
        self.logical_part
    }

    pub fn set_logical_part(&mut self, p: Option<LogicalPartId>) {
        self.logical_part = p;
    }

    pub fn translate_by(&mut self, v: &Vector) {
        if let Some(e) = self.exact_location {
            let vf = v.to_float();
            let n = FloatPoint::new(e.x + vf.x, e.y + vf.y);
            self.exact_location = Some(n);
            self.location = Some(n.round());
        } else if let Some(l) = self.location {
            let n = Point::Int(l).translate_by(v).to_int();
            self.location = Some(n);
            self.exact_location = Some(n.to_float());
        }
    }

    pub fn turn_90_degree(&mut self, factor: i32, pole: &IntPoint, flip_style_rotate_first: bool) {
        if factor == 0 {
            return;
        }
        let f = if flip_style_rotate_first && !self.on_front {
            -factor
        } else {
            factor
        };
        self.rotation_in_degree = normalize_degree(self.rotation_in_degree + (f * 90) as f64);
        if let Some(e) = self.exact_location {
            let n = e.turn_90_degree_around(factor, &pole.to_float());
            self.exact_location = Some(n);
            self.location = Some(n.round());
        } else if let Some(l) = self.location {
            let n = l.turn_90_degree(factor, pole);
            self.location = Some(n);
            self.exact_location = Some(n.to_float());
        }
    }

    pub fn rotate(&mut self, angle: f64, pole: &IntPoint, flip_style_rotate_first: bool) {
        if angle == 0.0 {
            return;
        }
        if angle % 90.0 == 0.0 {
            self.turn_90_degree((angle / 90.0) as i32, pole, flip_style_rotate_first);
            return;
        }
        let turn = if flip_style_rotate_first && !self.on_front {
            360.0 - angle
        } else {
            angle
        };
        self.rotation_in_degree = normalize_degree(self.rotation_in_degree + turn);
        let pf = pole.to_float();
        if let Some(e) = self.exact_location {
            let n = e.rotate(angle.to_radians(), &pf);
            self.exact_location = Some(n);
            self.location = Some(n.round());
        } else if let Some(l) = self.location {
            let n = l.to_float().rotate(angle.to_radians(), &pf).round();
            self.location = Some(n);
            self.exact_location = Some(n.to_float());
        }
    }

    pub fn change_side(&mut self, pole: &IntPoint, flip_style_rotate_first: bool) {
        if !flip_style_rotate_first {
            self.rotation_in_degree = (360.0 - self.rotation_in_degree) % 360.0;
        }
        self.on_front = !self.on_front;
        if let Some(e) = self.exact_location {
            let n = FloatPoint::new(2.0 * pole.x as f64 - e.x, e.y);
            self.exact_location = Some(n);
            self.location = Some(n.round());
        } else if let Some(l) = self.location {
            let n = l.mirror_vertical(pole);
            self.location = Some(n);
            self.exact_location = Some(n.to_float());
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Components {
    arr: Vec<Component>,
    flip_style_rotate_first: bool,
}

impl Components {
    #[allow(clippy::too_many_arguments)]
    pub fn add_precise(
        &mut self,
        name: &str,
        exact_location: Option<FloatPoint>,
        rotation_in_degree: f64,
        on_front: bool,
        package_front: PackageId,
        package_back: PackageId,
        position_fixed: bool,
        part_number: Option<String>,
    ) -> i32 {
        let id = self.arr.len() as i32 + 1;
        self.arr.push(Component {
            name: name.to_string(),
            id,
            position_fixed,
            package_front,
            package_back,
            part_number,
            location: exact_location.map(|e| e.round()),
            exact_location,
            rotation_in_degree: normalize_degree(rotation_in_degree),
            logical_part: None,
            on_front,
        });
        id
    }

    pub fn get_by_name(&self, name: &str) -> Option<&Component> {
        self.arr.iter().find(|c| c.name == name)
    }

    #[inline]
    pub fn get(&self, id: i32) -> &Component {
        &self.arr[id as usize - 1]
    }

    pub fn try_get(&self, id: i32) -> Option<&Component> {
        if id <= 0 {
            return None;
        }
        self.arr.get(id as usize - 1)
    }

    pub fn get_mut(&mut self, id: i32) -> &mut Component {
        &mut self.arr[id as usize - 1]
    }

    pub fn count(&self) -> usize {
        self.arr.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Component> {
        self.arr.iter()
    }

    pub fn get_flip_style_rotate_first(&self) -> bool {
        self.flip_style_rotate_first
    }

    pub fn set_flip_style_rotate_first(&mut self, v: bool) {
        self.flip_style_rotate_first = v;
    }
}
