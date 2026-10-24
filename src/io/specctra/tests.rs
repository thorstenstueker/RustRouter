//! Tests of the Specctra I/O with the fixtures of the original project.

use super::*;
use crate::board::item::BoardItemType;
use crate::board::{Board, FixedState};
use crate::geometry::{IntBox, Point, Shape, TileShape};
use crate::rules::{AngleRestriction, ItemClass, NetClassId, ViaRuleId};
use std::fmt::Write as _;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../freerouting/fixtures")
}

fn fixture(name: &str) -> PathBuf {
    fixtures_dir().join(name)
}

fn jd(v: f64) -> String {
    java_double_to_string(v)
}

fn box_str(b: &IntBox) -> String {
    if b.is_empty() {
        "[empty]".to_string()
    } else {
        format!("[{},{},{},{}]", b.ll.x, b.ll.y, b.ur.x, b.ur.y)
    }
}

fn pt(p: &Point) -> String {
    match p {
        Point::Int(i) => format!("({},{})", i.x, i.y),
        _ => {
            let f = p.to_float();
            format!("(~{},~{})", jd(f.x), jd(f.y))
        }
    }
}

fn shape_str(s: Option<&Shape>) -> String {
    let Some(s) = s else { return "null".to_string() };
    let kind = match s {
        Shape::Tile(TileShape::Box(_)) => "B",
        Shape::Tile(TileShape::Octagon(_)) => "O",
        Shape::Tile(TileShape::Simplex(_)) => "S",
        Shape::Circle(_) => "C",
        Shape::Polygon(_) => "P",
    };
    format!("{}{}{}", kind, s.border_line_count(), box_str(&s.bounding_box()))
}

fn tile_str(t: &TileShape) -> String {
    shape_str(Some(&Shape::Tile(t.clone())))
}

fn fixed_str(f: FixedState) -> &'static str {
    match f {
        FixedState::Unfixed => "UNFIXED",
        FixedState::ShoveFixed => "SHOVE_FIXED",
        FixedState::UserFixed => "USER_FIXED",
        FixedState::SystemFixed => "SYSTEM_FIXED",
    }
}

fn type_str(t: BoardItemType) -> &'static str {
    match t {
        BoardItemType::Trace => "TRACE",
        BoardItemType::Pin => "PIN",
        BoardItemType::Via => "VIA",
        BoardItemType::ObstacleArea => "OBSTACLE_AREA",
        BoardItemType::ViaObstacleArea => "VIA_OBSTACLE_AREA",
        BoardItemType::ConductionArea => "CONDUCTION_AREA",
        BoardItemType::ComponentObstacleArea => "COMPONENT_OBSTACLE_AREA",
        BoardItemType::BoardOutline => "BOARD_OUTLINE",
        BoardItemType::ComponentOutline => "COMPONENT_OUTLINE",
    }
}

/// Dumps the board in the format of the Java harness `Dump.java` (see the report).
pub fn dump_board(b: &Board) -> String {
    let mut o = String::new();
    let lc = b.layer_count();
    writeln!(o, "bbox {}", box_str(&b.bounding_box)).unwrap();
    let ct = b.communication.coordinate_transform.unwrap();
    writeln!(
        o,
        "scale {} unit {} res {}",
        jd(ct.dsn_to_board(1.0)),
        b.communication.unit.as_str(),
        b.communication.resolution
    )
    .unwrap();
    for (i, l) in b.layer_structure.layers.iter().enumerate() {
        writeln!(o, "layer {} {} {}", i, l.name, l.is_signal).unwrap();
    }
    let ru = &b.rules;
    let angle = match ru.get_trace_angle_restriction() {
        AngleRestriction::None => "NONE",
        AngleRestriction::FortyfiveDegree => "FORTYFIVE_DEGREE",
        AngleRestriction::NinetyDegree => "NINETY_DEGREE",
    };
    writeln!(
        o,
        "rules angle={} pin_edge={} minhw={} maxhw={}",
        angle,
        jd(ru.get_pin_edge_to_turn_dist()),
        ru.get_min_trace_half_width(),
        ru.get_max_trace_half_width()
    )
    .unwrap();
    let cm = &ru.clearance_matrix;
    for i in 0..cm.get_class_count() {
        let mut s = format!("cl {} {}:", i, cm.get_name(i as i32).unwrap_or(""));
        for j in 0..cm.get_class_count() {
            s.push(' ');
            for l in 0..cm.get_layer_count() {
                if l > 0 {
                    s.push('/');
                }
                write!(s, "{}", cm.get_value(i as i32, j as i32, l as i32, false)).unwrap();
            }
        }
        writeln!(o, "{s}").unwrap();
    }
    let via_rule_name = |r: Option<ViaRuleId>| match r {
        Some(r) => ru.via_rule(r).name.clone(),
        None => "null".to_string(),
    };
    for (i, c) in ru.net_classes.iter() {
        let mut s = format!("netclass {} {} hw=", i.0, c.get_name());
        for l in 0..c.layer_count() {
            if l > 0 {
                s.push('/');
            }
            write!(s, "{}", c.get_trace_half_width(l)).unwrap();
        }
        write!(s, " cl={} via_rule={}", c.get_trace_clearance_class(), via_rule_name(c.get_via_rule())).unwrap();
        s.push_str(" active=");
        for l in 0..c.layer_count() {
            s.push(if c.is_active_routing_layer(l as i32) { '1' } else { '0' });
        }
        s.push_str(" dicc=");
        for ic in ItemClass::ALL {
            write!(s, "{},", c.default_item_clearance_classes.get(ic)).unwrap();
        }
        write!(
            s,
            " pull={} shove={} minl={} maxl={}",
            c.get_pull_tight(),
            c.is_shove_fixed(),
            jd(c.get_minimum_trace_length()),
            jd(c.get_maximum_trace_length())
        )
        .unwrap();
        writeln!(o, "{s}").unwrap();
    }
    for n in ru.nets.iter() {
        writeln!(
            o,
            "net {} {} {} class={} plane={}",
            n.net_number,
            n.name,
            n.subnet_number,
            n.net_class.0,
            n.contains_plane()
        )
        .unwrap();
    }
    for i in 0..ru.via_infos.count() {
        let v = ru.via_infos.get(crate::rules::ViaInfoId(i));
        writeln!(
            o,
            "viainfo {} {} {} {}",
            v.name,
            b.library.padstacks.get(v.padstack).name,
            v.clearance_class,
            v.attach_smd_allowed
        )
        .unwrap();
    }
    for r in &ru.via_rules {
        let mut s = format!("viarule {}:", r.name);
        for v in r.vias() {
            write!(s, " {}", ru.via_infos.get(*v).name).unwrap();
        }
        writeln!(o, "{s}").unwrap();
    }
    for p in b.library.padstacks.iter() {
        let mut s = format!(
            "padstack {} {} attach={} abs={} nonconvex={}:",
            p.id.0, p.name, p.attach_allowed, p.placed_absolute, p.has_non_convex_geometry
        );
        for l in 0..lc {
            s.push(' ');
            s.push_str(&shape_str(p.shapes().get(l as usize).and_then(|x| x.as_ref())));
            if let Some(a) = p.get_auxiliary_shapes(l) {
                write!(s, "+aux{}", a.len()).unwrap();
            }
        }
        writeln!(o, "{s}").unwrap();
    }
    let mut s = "via_padstacks:".to_string();
    for p in b.library.get_via_padstacks() {
        write!(s, " {}", b.library.padstacks.get(*p).name).unwrap();
    }
    writeln!(o, "{s}").unwrap();
    for p in b.library.packages.iter() {
        let mut s = format!(
            "package {} {} front={} outlines={} keepouts={}/{}/{}:",
            p.id.0,
            p.name,
            p.is_front,
            p.outline.len(),
            p.keepouts.len(),
            p.via_keepouts.len(),
            p.place_keepouts.len()
        );
        for pin in p.pins() {
            let e = pin.get_exact_relative_location();
            write!(s, " {}/{}@{},{} r{}", pin.name, pin.padstack_id.0, jd(e.x), jd(e.y), jd(pin.rotation_in_degree)).unwrap();
        }
        writeln!(o, "{s}").unwrap();
    }
    for c in b.components.iter() {
        let loc = match c.get_exact_location() {
            Some(e) if c.is_placed() => format!("{},{}", jd(e.x), jd(e.y)),
            _ => "unplaced".to_string(),
        };
        let lp = match c.get_logical_part() {
            Some(l) => b.library.logical_parts.get(l).name.clone(),
            None => "-".to_string(),
        };
        writeln!(
            o,
            "component {} {} pkg={} {} rot={} front={} fixed={} lp={}",
            c.id,
            c.name,
            c.get_package().0,
            loc,
            jd(c.get_rotation_in_degree()),
            c.placed_on_front(),
            c.position_fixed,
            lp
        )
        .unwrap();
    }
    let mut count = 0;
    for it in b.items.iter() {
        count += 1;
        let nets: Vec<String> = it.net_numbers.iter().map(|n| n.to_string()).collect();
        let mut s = format!(
            "item {} {} nets=[{}] cl={} comp={} fix={} L={}-{} bb={}",
            it.id.0,
            type_str(it.board_item_type()),
            nets.join(","),
            it.clearance_class,
            it.component_id,
            fixed_str(it.fixed_state),
            it.first_layer_with(lc),
            it.last_layer_with(lc),
            box_str(&it.bounding_box())
        );
        if let Some(d) = it.as_drill() {
            write!(s, " c={} ps={}", pt(&d.center), d.padstack.0).unwrap();
        }
        if let Some(p) = it.as_pin() {
            write!(s, " pin={}", p.pin_index).unwrap();
        }
        if let Some(t) = it.as_trace() {
            write!(s, " hw={} corners=", t.half_width).unwrap();
            for c in t.polyline.corners() {
                s.push_str(&pt(&c));
            }
        }
        if let Some(oa) = it.as_obstacle() {
            write!(s, " name={}", oa.name.as_deref().unwrap_or("null")).unwrap();
        }
        if let Some(bo) = it.as_board_outline() {
            write!(s, " shapes={}", bo.shapes.len()).unwrap();
        }
        let n = b.tile_shape_count(it.id);
        write!(s, " tiles={}:", n).unwrap();
        for k in 0..n {
            s.push(' ');
            match b.tile_shape(it.id, k) {
                Some(t) => s.push_str(&tile_str(&t)),
                None => s.push_str("null"),
            }
        }
        writeln!(o, "{s}").unwrap();
    }
    writeln!(o, "itemcount {count}").unwrap();
    o
}

#[test]
fn lexer_tokens() {
    use super::lexer::{LexState, Lexer, Tok};
    let mut lx = Lexer::new("(pcb abc) 123 -45 007 1.5 1e5 2e .5 a\"b #c\n x /* y */ z /*w*/ \"q q\"");
    let mut toks = Vec::new();
    loop {
        let t = lx.next_token();
        if t == Tok::Eof {
            break;
        }
        toks.push(t);
    }
    let expect = vec![
        Tok::Open,
        Tok::Kw(super::lexer::Kw::PcbScope),
        Tok::Str("abc".into()),
        Tok::Close,
        Tok::Int(123),
        Tok::Int(-45),
        Tok::Dbl(7.0),
        Tok::Dbl(1.5),
        Tok::Dbl(100000.0),
        Tok::Int(2),
        Tok::Str("e".into()),
        Tok::Str(".5".into()),
        Tok::Str("a\"b".into()),
        Tok::Str("x".into()),
        Tok::Str("z".into()),
        Tok::Str("/*w*/".into()),
        Tok::Str("q q".into()),
    ];
    assert_eq!(toks, expect);
    let mut lx = Lexer::new("-12 x");
    lx.yybegin(LexState::Name);
    assert_eq!(lx.next_token(), Tok::Str("-12".into()));
}

#[test]
fn read_empty_board() {
    let r = read_dsn(fixture("empty_board.dsn")).unwrap();
    let b = &r.board;
    assert_eq!(b.layer_count(), 2);
    assert_eq!(b.items.len(), 1);
    assert!(b.get_outline().is_some());
    assert_eq!(b.rules.nets.max_net_number(), 0);
    let ses = ses_string(b, "empty_board.dsn").unwrap();
    let expected = "(session \"empty_board.ses\"\n  (base_design \"empty_board.dsn\")\n  (placement\n    (resolution um 10)\n  )\n  (was_is\n  )\n  (routes \n    (resolution um 10)\n    (parser\n    )\n    (library_out \n    )\n    (network_out \n    )\n  )\n)";
    assert_eq!(ses, expected);
}

#[test]
fn read_issue026_j2() {
    let r = read_dsn(fixture("Issue026-J2_reference.dsn")).unwrap();
    let b = &r.board;
    assert_eq!(b.layer_count(), 2);
    assert_eq!(b.rules.nets.max_net_number(), 24);
    assert_eq!(b.components.count(), 2);
    assert_eq!(b.library.padstacks.count(), 5);
    assert!(b.get_pin_count() > 0);
    assert_eq!(b.rules.net_classes.get(NetClassId(0)).get_name(), "default");
    assert!(b.get_traces().is_empty());
    let mut board = r.board;
    let summary = read_ses(fixture("Issue026-J2_reference.ses"), &mut board).unwrap();
    assert!(summary.wires_imported > 0);
    assert_eq!(summary.errors_encountered, 0);
    assert_eq!(board.get_traces().len(), summary.wires_imported);
}

#[test]
fn read_several_fixtures() {
    for name in [
        "Issue015-StackOverflow.dsn",
        "Issue027-zMRETestFixture.dsn",
        "Issue029-hw48na.dsn",
        "Issue103-Board-Routed.dsn",
        "Issue143-rpi_splitter.dsn",
        "Issue313-FastTest.dsn",
    ] {
        let r = read_dsn(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let b = &r.board;
        assert!(b.layer_count() >= 1, "{name}");
        assert!(b.get_outline().is_some(), "{name}");
        assert!(b.components.count() > 0, "{name}");
        assert!(b.get_pin_count() > 0, "{name}");
        assert!(b.library.padstacks.count() > 0, "{name}");
        ses_string(b, name).unwrap();
    }
}

/// Writes the dumps and session files of all DSN fixtures (or of `DSN_FILES`, separated by ':')
/// into the directory `DSN_DUMP_DIR`, for the comparison with the Java original.
#[test]
#[ignore]
fn dump_fixtures() {
    let Ok(out) = std::env::var("DSN_DUMP_DIR") else { return };
    let files: Vec<PathBuf> = match std::env::var("DSN_FILES") {
        Ok(f) => f.split(':').map(PathBuf::from).collect(),
        Err(_) => {
            let mut v: Vec<PathBuf> = std::fs::read_dir(fixtures_dir())
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().map(|e| e == "dsn").unwrap_or(false))
                .collect();
            v.sort();
            v
        }
    };
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let out_name = f.to_string_lossy().replace('/', "_");
        let res = std::panic::catch_unwind(|| read_dsn(&f));
        let text = match res {
            Ok(Ok(r)) => {
                let ses = ses_string(&r.board, &name).unwrap_or_else(|e| format!("SES ERROR {e}"));
                std::fs::write(format!("{out}/{out_name}.rust.ses"), ses).unwrap();
                std::fs::write(format!("{out}/{out_name}.rust.rules"), rules_string(&r.board, "design")).unwrap();
                dump_board(&r.board)
            }
            Ok(Err(e)) => format!("RESULT {e}\n"),
            Err(_) => "PANIC\n".to_string(),
        };
        std::fs::write(format!("{out}/{out_name}.rust.txt"), text).unwrap();
    }
}

/// Reads DSN files, applies rules files and dumps the boards (`DSN_RULES_PAIRS` = `dsn=rules:...`).
#[test]
#[ignore]
fn dump_with_rules() {
    let Ok(out) = std::env::var("DSN_DUMP_DIR") else { return };
    let Ok(pairs) = std::env::var("DSN_RULES_PAIRS") else { return };
    for pair in pairs.split(':') {
        let (dsn, rules) = pair.split_once('=').unwrap();
        let mut r = read_dsn(dsn).unwrap();
        let ok = read_rules(rules, "x", &mut r.board).is_ok();
        let text = format!("RULESREAD {}\n{}", ok, dump_board(&r.board));
        let out_name = rules.replace('/', "_");
        std::fs::write(format!("{out}/{out_name}.rust.txt"), text).unwrap();
    }
}

/// Reads DSN files, imports session files and dumps the boards (`DSN_SES_PAIRS` = `dsn=ses:...`).
#[test]
#[ignore]
fn dump_with_ses() {
    let Ok(out) = std::env::var("DSN_DUMP_DIR") else { return };
    let Ok(pairs) = std::env::var("DSN_SES_PAIRS") else { return };
    for (i, pair) in pairs.split(':').enumerate() {
        let (dsn, ses) = pair.split_once('=').unwrap();
        let mut r = read_dsn(dsn).unwrap();
        let text = match read_ses(ses, &mut r.board) {
            Ok(s) => format!(
                "SESREAD {} {} {}\n{}",
                s.wires_imported,
                s.vias_imported,
                s.errors_encountered,
                dump_board(&r.board)
            ),
            Err(e) => format!("SESERROR {e}\n"),
        };
        std::fs::write(format!("{out}/ses{i}.rust.txt"), text).unwrap();
    }
}
