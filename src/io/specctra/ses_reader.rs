//! Reader of Specctra session files (port of `SesReader` and `SesPathGeometry`).

use super::lexer::{Kw, LexState, Lexer, Tok};
use super::network::strip_dot_digits;
use super::reader::DsnError;
use super::shapes::{read_polygon_path_scope, round_i, DsnLayerStructure, DsnShapeKind, LayerRef};
use crate::board::{Board, CoordinateTransform, FixedState, Unit};
use crate::geometry::{basics::rint, IntDirection, IntPoint, Line, Point, Polyline, CRIT_INT};
use crate::rules::{ItemClass, NetClassId};
use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// Summary of a session import (`SesImportSummary`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SesImportSummary {
    pub wires_imported: usize,
    pub vias_imported: usize,
    pub errors_encountered: usize,
}

/// Reads a session file and imports its wires and vias into `board` (`SesReader.read`).
pub fn read_ses_str(text: &str, board: &mut Board) -> Result<SesImportSummary, DsnError> {
    let ct = board.communication.coordinate_transform.unwrap_or(CoordinateTransform::new(1.0, 0.0, 0.0));
    let scale = ct.dsn_to_board(1.0) / board.communication.resolution as f64;
    let mut r = SesReader {
        lx: Lexer::new(text),
        ls: DsnLayerStructure::from_board(&board.layer_structure),
        board,
        scale,
        summary: SesImportSummary::default(),
    };
    r.process_session_scope()?;
    log::info!(
        "SES file import complete: {} wires, {} vias imported{}",
        r.summary.wires_imported,
        r.summary.vias_imported,
        if r.summary.errors_encountered > 0 {
            format!(" ({} errors)", r.summary.errors_encountered)
        } else {
            String::new()
        }
    );
    Ok(r.summary)
}

/// Reads a session file from `path` into `board`.
pub fn read_ses(path: impl AsRef<std::path::Path>, board: &mut Board) -> Result<SesImportSummary, DsnError> {
    let bytes = std::fs::read(path)?;
    read_ses_str(&String::from_utf8_lossy(&bytes), board)
}

struct SesReader<'a> {
    lx: Lexer,
    ls: DsnLayerStructure,
    board: &'a mut Board,
    scale: f64,
    summary: SesImportSummary,
}

impl SesReader<'_> {
    fn process_session_scope(&mut self) -> Result<(), DsnError> {
        let mut next = Tok::Eof;
        for i in 0..3 {
            next = self.lx.next_token();
            let ok = match i {
                0 => next == Tok::Open,
                1 => {
                    let ok = next.is_kw(Kw::Session);
                    self.lx.yybegin(LexState::Name);
                    ok
                }
                _ => true,
            };
            if !ok {
                return Err(DsnError::Parse(format!(
                    "SesReader: not a Specctra session file — expected '(session <name>' header, got: {next:?}"
                )));
            }
        }
        loop {
            let prev = std::mem::replace(&mut next, self.lx.next_token());
            if next == Tok::Eof {
                return Ok(());
            }
            if next == Tok::Close {
                break;
            }
            if prev == Tok::Open {
                if next.is_kw(Kw::Routes) {
                    self.process_routes_scope()?;
                } else {
                    self.lx.skip_scope();
                }
            }
        }
        Ok(())
    }

    fn process_routes_scope(&mut self) -> Result<(), DsnError> {
        let mut next = Tok::Eof;
        loop {
            let prev = std::mem::replace(&mut next, self.lx.next_token());
            if next == Tok::Eof {
                log::warn!("SesReader.processRoutesScope: unexpected end of file at '{}'", self.lx.scope_identifier);
                return Ok(());
            }
            if next == Tok::Close {
                break;
            }
            if prev == Tok::Open {
                if next.is_kw(Kw::NetworkOut) {
                    self.process_network_scope();
                } else if next.is_kw(Kw::ResolutionScope) {
                    self.read_session_resolution()?;
                } else {
                    self.lx.skip_scope();
                }
            }
        }
        Ok(())
    }

    fn read_session_resolution(&mut self) -> Result<(), DsnError> {
        let unit = self.lx.next_token();
        let res = self.lx.next_token();
        let close = self.lx.next_token();
        let err = || {
            DsnError::Parse("SesReader: invalid routes resolution; expected (resolution <unit> <positive integer>)".into())
        };
        let Tok::Str(name) = unit else { return Err(err()) };
        let Some(u) = Unit::from_str_opt(&name) else { return Err(err()) };
        let Tok::Int(r) = res else { return Err(err()) };
        if r <= 0 || close != Tok::Close {
            return Err(err());
        }
        self.scale = self.board.communication.get_resolution(u) / r as f64;
        Ok(())
    }

    fn net_class_of(&self, nets: &[i32]) -> NetClassId {
        nets.first()
            .and_then(|n| self.board.rules.nets.get(*n))
            .map(|n| n.net_class)
            .unwrap_or(NetClassId(0))
    }

    fn process_network_scope(&mut self) {
        let mut next = Tok::Eof;
        loop {
            let prev = std::mem::replace(&mut next, self.lx.next_token());
            if next == Tok::Eof {
                log::warn!("SesReader.processNetworkScope: unexpected end of file at '{}'", self.lx.scope_identifier);
                return;
            }
            if next == Tok::Close {
                break;
            }
            if prev == Tok::Open {
                if next.is_kw(Kw::Net) {
                    self.process_net_scope();
                } else {
                    self.lx.skip_scope();
                }
            }
        }
    }

    fn process_net_scope(&mut self) {
        let mut next = self.lx.next_token();
        let Tok::Str(name) = next.clone() else {
            log::warn!("SesReader.processNetScope: String expected at '{}'", self.lx.scope_identifier);
            self.summary.errors_encountered += 1;
            return;
        };
        self.lx.scope_identifier = name.clone();
        let Some(net) = self.board.rules.nets.get_by_name_subnet(&name, 1) else {
            log::warn!("SesReader: net not found: '{name}' — skipping");
            self.summary.errors_encountered += 1;
            self.lx.skip_scope();
            return;
        };
        let nets = [net.net_number];
        loop {
            let prev = std::mem::replace(&mut next, self.lx.next_token());
            if next == Tok::Eof {
                return;
            }
            if next == Tok::Close {
                break;
            }
            if prev == Tok::Open {
                if next.is_kw(Kw::Wire) {
                    if !self.process_wire_scope(&nets) {
                        self.summary.errors_encountered += 1;
                    }
                } else if next.is_kw(Kw::Via) {
                    if !self.process_via_scope(&nets) {
                        self.summary.errors_encountered += 1;
                    }
                } else {
                    self.lx.skip_scope();
                }
            }
        }
    }

    fn process_wire_scope(&mut self, nets: &[i32]) -> bool {
        let mut path = None;
        let mut next = Tok::Eof;
        loop {
            let prev = std::mem::replace(&mut next, self.lx.next_token());
            if next == Tok::Eof {
                log::warn!("SesReader.processWireScope: unexpected end of file at '{}'", self.lx.scope_identifier);
                return false;
            }
            if next == Tok::Close {
                break;
            }
            if prev == Tok::Open {
                if next.is_kw(Kw::PolygonPath) {
                    path = read_polygon_path_scope(&mut self.lx, Some(&self.ls));
                } else {
                    self.lx.skip_scope();
                }
            }
        }
        let Some(path) = path else {
            // conduction areas have no polygon path
            return true;
        };
        let DsnShapeKind::PolygonPath { width, coords } = &path.kind else { return false };
        let layer = path.layer.no();
        if path.layer == LayerRef::Missing || layer < 0 || layer >= self.board.layer_count() {
            log::warn!("SesReader.processWireScope: failed to import wire — Invalid SES wire layer");
            return false;
        }
        let polyline = match ses_path_read(coords, self.scale) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("SesReader.processWireScope: failed to import wire — {}", e.0);
                return false;
            }
        };
        let half_width = match ses_half_width(*width, self.scale) {
            Ok(h) => h,
            Err(e) => {
                log::warn!("SesReader.processWireScope: failed to import wire — {}", e.0);
                return false;
            }
        };
        let cl = self
            .board
            .rules
            .net_classes
            .get(self.net_class_of(nets))
            .default_item_clearance_classes
            .get(ItemClass::Trace);
        if self
            .board
            .insert_trace_without_cleaning(polyline, layer, half_width, nets, cl, FixedState::UserFixed)
            .is_none()
        {
            return false;
        }
        self.summary.wires_imported += 1;
        true
    }

    fn process_via_scope(&mut self, nets: &[i32]) -> bool {
        let Tok::Str(ps_name) = self.lx.next_token() else {
            log::warn!("SesReader.processViaScope: padstack name expected at '{}'", self.lx.scope_identifier);
            return false;
        };
        self.lx.scope_identifier = ps_name.clone();
        let mut location = [0.0; 2];
        for l in location.iter_mut() {
            match self.lx.next_token().as_number() {
                Some(v) => *l = v,
                None => {
                    log::warn!("SesReader.processViaScope: number expected at '{}'", self.lx.scope_identifier);
                    return false;
                }
            }
        }
        let mut t = self.lx.next_token();
        while t == Tok::Open {
            self.lx.skip_scope();
            t = self.lx.next_token();
        }
        if t != Tok::Close {
            log::warn!("SesReader.processViaScope: closing bracket expected at '{}'", self.lx.scope_identifier);
            return false;
        }
        let ps = self
            .board
            .library
            .padstacks
            .get_by_name(&ps_name)
            .or_else(|| self.board.library.padstacks.get_by_name(&strip_dot_digits(&ps_name)));
        let Some(ps) = ps else {
            log::warn!("SesReader.processViaScope: via padstack not found: {ps_name}");
            return false;
        };
        let x = round_i(location[0] * self.scale);
        let y = round_i(location[1] * self.scale);
        let cl = self
            .board
            .rules
            .net_classes
            .get(self.net_class_of(nets))
            .default_item_clearance_classes
            .get(ItemClass::Via);
        self.board
            .insert_via(ps, &Point::Int(IntPoint::new(x, y)), nets, cl, FixedState::UserFixed, true);
        self.summary.vias_imported += 1;
        true
    }
}

// ------------------------------------------------------------------ exact fractions

/// A reduced fraction with positive denominator (the canonical form of `BigFraction`
/// for all values occurring here).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frac {
    n: BigInt,
    d: BigInt,
}

#[derive(Debug)]
struct GeomError(String, bool);

impl GeomError {
    /// An `IOException` of the original.
    fn io(msg: &str) -> GeomError {
        GeomError(msg.to_string(), false)
    }

    /// An `ArithmeticException` of the original.
    fn arith(msg: &str) -> GeomError {
        GeomError(msg.to_string(), true)
    }
}

type GResult<T> = Result<T, GeomError>;

impl Frac {
    fn new(n: BigInt, d: BigInt) -> Frac {
        let g = n.gcd(&d);
        let (mut n, mut d) = if g.is_zero() || g.is_one() { (n, d) } else { (n / &g, d / &g) };
        if d.is_negative() {
            n = -n;
            d = -d;
        }
        Frac { n, d }
    }

    fn int(v: BigInt) -> Frac {
        Frac { n: v, d: BigInt::one() }
    }

    fn of_i64(v: i64) -> Frac {
        Frac::int(BigInt::from(v))
    }

    /// `BigFraction.from(double)`: the exact value of a double.
    fn from_f64(v: f64) -> Frac {
        if v == 0.0 {
            return Frac::of_i64(0);
        }
        let bits = v.to_bits();
        let sign = if (bits >> 63) != 0 { -1 } else { 1 };
        let exp = ((bits >> 52) & 0x7ff) as i64;
        let mut m = (bits & ((1u64 << 52) - 1)) as i64;
        let e = if exp == 0 {
            -1074
        } else {
            m |= 1 << 52;
            exp - 1075
        };
        let m = BigInt::from(sign * m);
        if e >= 0 {
            Frac::int(m << (e as usize))
        } else {
            Frac::new(m, BigInt::one() << ((-e) as usize))
        }
    }

    fn add(&self, o: &Frac) -> Frac {
        Frac::new(&self.n * &o.d + &o.n * &self.d, &self.d * &o.d)
    }

    fn sub(&self, o: &Frac) -> Frac {
        Frac::new(&self.n * &o.d - &o.n * &self.d, &self.d * &o.d)
    }

    fn mul(&self, o: &Frac) -> Frac {
        Frac::new(&self.n * &o.n, &self.d * &o.d)
    }

    fn mul_int(&self, i: &BigInt) -> Frac {
        Frac::new(&self.n * i, self.d.clone())
    }

    fn div_int(&self, i: &BigInt) -> GResult<Frac> {
        if i.is_zero() {
            return Err(GeomError::arith("Division by zero"));
        }
        Ok(Frac::new(self.n.clone(), &self.d * i))
    }

    fn signum(&self) -> i32 {
        match self.n.sign() {
            Sign::Minus => -1,
            Sign::NoSign => 0,
            Sign::Plus => 1,
        }
    }

    fn is_int(&self) -> bool {
        self.d.is_one()
    }

    /// Correctly rounded (half even) conversion to double.
    fn to_f64(&self) -> f64 {
        if self.n.is_zero() {
            return 0.0;
        }
        let neg = self.n.is_negative();
        let n = self.n.abs();
        let d = &self.d;
        // scale so that the quotient has 54..=55 significant bits
        let shift = 55i64 - (n.bits() as i64 - d.bits() as i64);
        let (num, den) = if shift >= 0 {
            (n << (shift as usize), d.clone())
        } else {
            (n, d << ((-shift) as usize))
        };
        let (q, r) = num.div_rem(&den);
        // q has 55 or 56 bits; round to 53 bits half even with the sticky remainder
        let qb = q.bits() as i64;
        let extra = qb - 53;
        let mut m = q.clone() >> (extra as usize);
        let rem_bits = &q - (&m << (extra as usize));
        let half = BigInt::one() << ((extra - 1) as usize);
        let sticky = !r.is_zero();
        let round_up = if rem_bits > half {
            true
        } else if rem_bits == half {
            sticky || m.is_odd()
        } else {
            false
        };
        if round_up {
            m += 1;
        }
        let m = m.to_f64().unwrap();
        // value = m * 2^(extra - shift)
        let e = (extra - shift) as i32;
        let v = m * 2f64.powi(e.clamp(-1000, 1000)) * 2f64.powi(e - e.clamp(-1000, 1000));
        if neg {
            -v
        } else {
            v
        }
    }

    /// `bigDecimalValue(0, HALF_EVEN / HALF_UP).toBigInteger()`.
    fn round(&self, half_even: bool) -> BigInt {
        let (q, r) = self.n.div_rem(&self.d); // truncating
        if r.is_zero() {
            return q;
        }
        let twice: BigInt = r.abs() * 2;
        let away = match twice.cmp(&self.d) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => !half_even || q.is_odd(),
        };
        if away {
            if self.n.is_negative() {
                q - 1
            } else {
                q + 1
            }
        } else {
            q
        }
    }
}

/// `Math.ulp(double)`.
fn ulp(x: f64) -> f64 {
    let a = x.abs();
    if !a.is_finite() {
        return f64::INFINITY;
    }
    if a == f64::MAX {
        return 2f64.powi(971);
    }
    f64::from_bits(a.to_bits() + 1) - a
}

/// `BigFraction.from(value, epsilon, maxIterations)` of Commons Numbers 1.x (continued fractions
/// with a 32 bit limit of the convergents).
fn frac_from_cf(value: f64, epsilon: f64, max_iterations: i32) -> GResult<Frac> {
    const OVERFLOW: u64 = 1 << 31;
    if value == 0.0 {
        return Ok(Frac::of_i64(0));
    }
    let abs_value = value.abs();
    let mut r0 = abs_value;
    let mut a0 = r0.floor() as i64;
    if a0 > OVERFLOW as i64 {
        return Err(GeomError::arith("Overflow trying to convert to fraction"));
    }
    if r0 - a0 as f64 <= epsilon {
        if value < 0.0 {
            a0 = -a0;
        }
        return Ok(Frac::of_i64(a0));
    }
    let max_den: i64 = (i32::MIN as i64).abs();
    let mut p0: i64 = 1;
    let mut q0: i64 = 0;
    let mut p1: i64 = a0;
    let mut q1: i64 = 1;
    let mut p2: i64;
    let mut q2: i64;
    let mut n = 0;
    loop {
        n += 1;
        let r1 = 1.0 / (r0 - a0 as f64);
        let a1 = r1.floor() as i64;
        p2 = a1.wrapping_mul(p1).wrapping_add(p0);
        q2 = a1.wrapping_mul(q1).wrapping_add(q0);
        if (p2 as u64) > OVERFLOW || (q2 as u64) > OVERFLOW {
            if epsilon == 0.0 {
                p2 = p1;
                q2 = q1;
                break;
            }
            return Err(GeomError::arith("Overflow trying to convert to fraction"));
        }
        let convergent = p2 as f64 / q2 as f64;
        if n < max_iterations && (convergent - abs_value).abs() > epsilon && q2 < max_den {
            p0 = p1;
            p1 = p2;
            q0 = q1;
            q1 = q2;
            a0 = a1;
            r0 = r1;
        } else {
            break;
        }
    }
    if n >= max_iterations {
        return Err(GeomError::arith("Unable to convert to fraction"));
    }
    let (mut num, den) = if q2 <= max_den { (p2, q2) } else { (p1, q1) };
    let sign = |v: i64| (v as f32).signum() as f64 * if v == 0 { 0.0 } else { 1.0 };
    let s = sign(num) * sign(den);
    if s != value.signum() {
        num = -num;
    }
    Ok(Frac::new(BigInt::from(num), BigInt::from(den)))
}

// ------------------------------------------------------------------ SesPathGeometry

#[derive(Clone, Debug, PartialEq, Eq)]
struct Corner {
    x: Frac,
    y: Frac,
}

/// `SesPathGeometry.read`: restores the polyline of a SES wire.
fn ses_path_read(coordinates: &[f64], scale: f64) -> GResult<Polyline> {
    if coordinates.len() < 4 || coordinates.len() % 2 != 0 {
        return Err(GeomError::io("A SES wire needs at least two complete corners"));
    }
    let mut corners: Vec<Corner> = Vec::new();
    let mut i = 0;
    while i < coordinates.len() {
        let c = corner(coordinates[i] * scale, coordinates[i + 1] * scale)?;
        if corners.last() == Some(&c) {
            if coordinates[i - 2] != coordinates[i] || coordinates[i - 1] != coordinates[i + 1] {
                return Err(GeomError::io("A nonzero SES segment collapses at board coordinate precision"));
            }
        } else {
            corners.push(c);
        }
        i += 2;
    }
    if corners.len() == 1 {
        let crossing = crossing_lines(&corners[0])?;
        return Ok(Polyline::from_lines(&[crossing[0], crossing[1], crossing[0]]));
    }
    reject_collinear_reversals(&corners)?;
    let n = corners.len();
    let mut lines = vec![Line::from_coords(0, 0, 0, 0); n + 1];
    for k in 1..n {
        lines[k] = supporting_line(&corners[k - 1], &corners[k])?;
    }
    lines[0] = endpoint_line(&corners[0], &lines[1])?;
    lines[n] = endpoint_line(&corners[n - 1], &lines[n - 1])?;
    let polyline = Polyline::from_lines(&lines);
    if polyline.corner_count() < 2 {
        return Err(GeomError::io("The SES path cannot be represented on this board"));
    }
    Ok(polyline)
}

/// `SesPathGeometry.halfWidth`.
fn ses_half_width(width: f64, scale: f64) -> GResult<i32> {
    let value = width * scale / 2.0;
    let rounded = rint(value);
    if !value.is_finite()
        || value <= 0.0
        || rounded < 1.0
        || rounded > CRIT_INT as f64
        || (value - rounded).abs() > 4.0 * ulp(value)
    {
        return Err(GeomError::io("The SES wire width cannot be represented on this board"));
    }
    Ok(rounded as i32)
}

fn corner(x: f64, y: f64) -> GResult<Corner> {
    let fx = coordinate(x)?;
    let fy = coordinate(y)?;
    for den in [fx.d.clone(), fy.d.clone()] {
        let sx = at_denominator(x, &den);
        let sy = at_denominator(y, &den);
        if (sx.to_f64() - x).abs() <= 4.0 * ulp(x) && (sy.to_f64() - y).abs() <= 4.0 * ulp(y) {
            return Ok(Corner { x: sx, y: sy });
        }
    }
    Ok(Corner { x: fx, y: fy })
}

fn at_denominator(value: f64, den: &BigInt) -> Frac {
    let num = Frac::from_f64(value).mul_int(den).round(true);
    Frac::new(num, den.clone())
}

fn coordinate(value: f64) -> GResult<Frac> {
    if !value.is_finite() || value.abs() > CRIT_INT as f64 {
        return Err(GeomError::io("Non-finite or out-of-range SES wire coordinate"));
    }
    let tolerance = 4.0 * ulp(value);
    let nearest = rint(value);
    if (value - nearest).abs() <= tolerance {
        return Ok(Frac::of_i64(nearest as i64));
    }
    let whole = value as i64;
    match frac_from_cf(value - whole as f64, tolerance, 100) {
        Ok(f) => {
            let restored = f.add(&Frac::of_i64(whole));
            if (restored.to_f64() - value).abs() > tolerance {
                return Err(GeomError::io("SES rational recovery exceeds the serialization tolerance"));
            }
            Ok(restored)
        }
        Err(_) => Err(GeomError::io("The SES corner cannot be restored at board coordinate precision")),
    }
}

fn reject_collinear_reversals(corners: &[Corner]) -> GResult<()> {
    for i in 1..corners.len().saturating_sub(1) {
        let (b, c, a) = (&corners[i - 1], &corners[i], &corners[i + 1]);
        let dx = c.x.sub(&b.x);
        let dy = c.y.sub(&b.y);
        let nx = a.x.sub(&c.x);
        let ny = a.y.sub(&c.y);
        if dx.mul(&ny) == dy.mul(&nx) && dx.mul(&nx).add(&dy.mul(&ny)).signum() < 0 {
            return Err(GeomError::io("A collinear SES reversal cannot be represented without losing copper"));
        }
    }
    Ok(())
}

fn common_denominator(x: &Frac, y: &Frac) -> BigInt {
    let g = x.d.gcd(&y.d);
    (&x.d / g) * &y.d
}

fn supporting_line(from: &Corner, to: &Corner) -> GResult<Line> {
    let dxf = to.x.sub(&from.x);
    let dyf = to.y.sub(&from.y);
    let den = common_denominator(&dxf, &dyf);
    let dx = dxf.mul_int(&den).n;
    let dy = dyf.mul_int(&den).n;
    integer_line(from, dx, dy)
}

/// `BigInteger.mod`: the non negative remainder.
fn big_mod(a: &BigInt, m: &BigInt) -> BigInt {
    a.mod_floor(m)
}

/// `BigInteger.modInverse`.
fn mod_inverse(a: &BigInt, m: &BigInt) -> GResult<BigInt> {
    if m.is_one() {
        return Ok(BigInt::zero());
    }
    a.modinv(m).ok_or_else(|| GeomError::arith("BigInteger not invertible."))
}

fn integer_line(from: &Corner, dx: BigInt, dy: BigInt) -> GResult<Line> {
    let divisor = dx.gcd(&dy);
    if divisor.is_zero() {
        return Err(GeomError::arith("BigInteger divide by zero"));
    }
    let dx = &dx / &divisor;
    let dy = &dy / &divisor;
    let intercept = from.x.mul_int(&dy).sub(&from.y.mul_int(&dx));
    if !intercept.is_int() {
        return Err(GeomError::io("The SES segment has no integer supporting line; refusing to move copper"));
    }
    let constant = intercept.n;
    let (x, y);
    if dx.is_zero() {
        x = &constant / &dy;
        y = from.y.round(false);
    } else if dy.is_zero() {
        x = from.x.round(false);
        y = -&constant / &dx;
    } else {
        let adx = dx.abs();
        let inv = mod_inverse(&big_mod(&dy, &adx), &adx)?;
        let x0 = big_mod(&(&constant * inv), &adx);
        let y0 = (&dy * &x0 - &constant) / &dx;
        let steps = from.x.sub(&Frac::int(x0.clone())).div_int(&dx)?.round(false);
        x = x0 + &steps * &dx;
        y = y0 + &steps * &dy;
    }
    let a = integer_point(&x, &y)?;
    let b = integer_point(&(&x + &dx), &(&y + &dy))?;
    Ok(Line::new(a, b))
}

fn endpoint_line(corner: &Corner, segment: &Line) -> GResult<Line> {
    if corner.x.is_int() && corner.y.is_int() {
        let p = integer_point(&corner.x.n, &corner.y.n)?;
        return Ok(Line::from_point_dir(p, &segment.direction().turn_45_degree(2)));
    }
    let crossing = crossing_lines(corner)?;
    Ok(if crossing[0].is_parallel(segment) { crossing[1] } else { crossing[0] })
}

#[derive(Clone)]
struct Lattice {
    x: BigInt,
    y: BigInt,
}

impl Lattice {
    fn dot(&self, o: &Lattice) -> BigInt {
        &self.x * &o.x + &self.y * &o.y
    }

    fn subtract(&self, o: &Lattice, multiple: &BigInt) -> Lattice {
        Lattice {
            x: &self.x - &o.x * multiple,
            y: &self.y - &o.y * multiple,
        }
    }
}

fn crossing_lines(corner: &Corner) -> GResult<[Line; 2]> {
    let den = common_denominator(&corner.x, &corner.y);
    let x = corner.x.mul_int(&den).n;
    let y = corner.y.mul_int(&den).n;
    if den.is_one() {
        let p = integer_point(&x, &y)?;
        return Ok([Line::from_point_dir(p, &IntDirection::RIGHT), Line::from_point_dir(p, &IntDirection::UP)]);
    }
    let divisor = y.gcd(&den);
    let horizontal = &den / &divisor;
    let inv = mod_inverse(&big_mod(&(&y / &divisor), &horizontal), &horizontal)?;
    let residue = big_mod(&(&x * inv), &horizontal);
    let mut first = Lattice {
        x: horizontal,
        y: BigInt::zero(),
    };
    let mut second = Lattice { x: residue, y: divisor };
    let mut reduced = false;
    for _ in 0..128 {
        if first.dot(&first) > second.dot(&second) {
            std::mem::swap(&mut first, &mut second);
        }
        let multiple = Frac::new(first.dot(&second), first.dot(&first)).round(true);
        if multiple.is_zero() {
            reduced = true;
            break;
        }
        second = second.subtract(&first, &multiple);
    }
    if !reduced {
        return Err(GeomError::io("SES supporting-line reduction did not converge"));
    }
    let mut first_line: Option<Line> = None;
    let candidates = [
        first.clone(),
        second.clone(),
        first.subtract(&second, &BigInt::one()),
        first.subtract(&second, &-BigInt::one()),
    ];
    for dir in candidates {
        match integer_line(corner, dir.x, dir.y) {
            Ok(c) => match first_line {
                None => first_line = Some(c),
                Some(f) => {
                    if !f.is_parallel(&c) {
                        return Ok([f, c]);
                    }
                }
            },
            Err(e) if !e.1 => {}
            Err(e) => return Err(e),
        }
    }
    Err(GeomError::io("The SES corner has no supported pair of integer lines"))
}

fn integer_point(x: &BigInt, y: &BigInt) -> GResult<IntPoint> {
    let lim = BigInt::from(CRIT_INT);
    if x.abs() > lim || y.abs() > lim {
        return Err(GeomError::io("SES supporting-line coordinates exceed the board's exact integer range"));
    }
    Ok(IntPoint::new(x.to_i32().unwrap(), y.to_i32().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frac_to_f64_is_correctly_rounded() {
        for v in [0.1, 1.0 / 3.0, 12345.678, -0.000123, 1e-300, 3.0e20] {
            assert_eq!(Frac::from_f64(v).to_f64(), v);
        }
        assert_eq!(Frac::new(BigInt::from(1), BigInt::from(3)).to_f64(), 1.0 / 3.0);
        assert_eq!(Frac::new(BigInt::from(-2), BigInt::from(7)).to_f64(), -2.0 / 7.0);
    }

    #[test]
    fn continued_fraction() {
        let f = frac_from_cf(1.0 / 3.0, 4.0 * ulp(1.0 / 3.0), 100).unwrap();
        assert_eq!(f, Frac::new(BigInt::from(1), BigInt::from(3)));
    }
}
