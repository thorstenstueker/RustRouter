use super::*;

#[test]
fn line_intersection_orthogonal() {
    let l1 = Line::from_coords(0, 0, 10, 0);
    let l2 = Line::from_coords(5, -5, 5, 5);
    assert_eq!(l1.intersection(&l2), Point::Int(IntPoint::new(5, 0)));
}

#[test]
fn line_intersection_rational() {
    let l1 = Line::from_coords(0, 0, 3, 1);
    let l2 = Line::from_coords(0, 1, 1, 0);
    let p = l1.intersection(&l2);
    let f = p.to_float();
    assert!((f.x - 0.75).abs() < 1e-9 && (f.y - 0.25).abs() < 1e-9, "{f:?}");
}

#[test]
fn box_simplex_roundtrip() {
    let b = IntBox::new(0, 0, 100, 50);
    let s = b.to_simplex();
    assert_eq!(s.border_line_count(), 4);
    let t = TileShape::Simplex(s).simplify();
    assert!(matches!(t, TileShape::Box(bb) if bb == b));
}

#[test]
fn octagon_normalize_and_simplex() {
    let o = IntOctagon::new(0, 0, 100, 100, -80, 80, 20, 180).normalize();
    let s = o.to_simplex();
    assert_eq!(s.border_line_count(), 8);
    let back = s.to_int_octagon().unwrap();
    assert_eq!(back, o);
}

#[test]
fn box_cutout() {
    let outer = TileShape::Box(IntBox::new(0, 0, 100, 100));
    let inner = TileShape::Box(IntBox::new(40, 40, 60, 60));
    let pieces = outer.cutout(&inner);
    assert_eq!(pieces.len(), 4);
    let area: f64 = pieces.iter().map(|p| p.area()).sum();
    assert!((area - (10000.0 - 400.0)).abs() < 1e-6);
}

#[test]
fn simplex_cutout_area() {
    let outer = TileShape::Simplex(IntBox::new(0, 0, 100, 100).to_simplex());
    let inner = TileShape::Simplex(IntOctagon::new(30, 30, 70, 70, -20, 20, 70, 130).normalize().to_simplex());
    let pieces = outer.cutout(&inner);
    let area: f64 = pieces.iter().map(|p| p.area()).sum();
    let expected = 10000.0 - inner.area();
    assert!((area - expected).abs() < 1.0, "area {area} expected {expected}");
}

#[test]
fn polyline_offset_shapes() {
    let pl = Polyline::from_int_points(&[IntPoint::new(0, 0), IntPoint::new(1000, 0), IntPoint::new(1000, 1000)]);
    assert_eq!(pl.corner_count(), 3);
    let shapes = pl.offset_shapes(50);
    assert_eq!(shapes.len(), 2);
    assert!(shapes[0].contains(&Point::Int(IntPoint::new(500, 40))));
    assert!(!shapes[0].contains(&Point::Int(IntPoint::new(500, 60))));
}

#[test]
fn polygon_split_to_convex() {
    let pts: Vec<Point> = [(0, 0), (100, 0), (100, 100), (50, 50), (0, 100)]
        .iter()
        .map(|(x, y)| Point::Int(IntPoint::new(*x, *y)))
        .collect();
    let ps = PolygonShape::new(&pts);
    let pieces = ps.split_to_convex();
    assert!(pieces.len() >= 2);
    let area: f64 = pieces.iter().map(|p| p.area()).sum();
    assert!((area - ps.area()).abs() < 1.0, "{} vs {}", area, ps.area());
}

#[test]
fn java_random_matches() {
    // reference values from the JDK
    let mut r = basics::JavaRandom::new(42);
    assert_eq!(r.next_int(100), 30);
    assert_eq!(r.next_int(100), 63);
    assert_eq!(r.next_int(7), 6);
    assert_eq!(r.next_int(16), 0);
    let mut q = basics::JavaRandom::new(99);
    assert_eq!(q.next_int(5), 2);
    assert_eq!(q.next_int(13), 1);
}
