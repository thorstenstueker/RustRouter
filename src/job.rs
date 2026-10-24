//! Loading a design for a routing job (port of the headless parts of
//! `HeadlessBoardManager.loadFromSpecctraDsn` and the DSN settings source).

use crate::board::structure::Unit;
use crate::board::{Board, ItemKind};
use crate::geometry::basics::java_round;
use crate::geometry::{FloatLine, FloatPoint, Shape};
use crate::io::specctra::{DsnAutorouteSettings, DsnReadResult};
use crate::rules::ItemClass;
use crate::settings::RouterSettings;

const BOARD_EDGE_CLEARANCE_CLASS_NAME: &str = "board_edge";
const HOLE_EDGE_CLEARANCE_CLASS_NAME: &str = "hole_edge";
const DEFAULT_COPPER_TO_EDGE_CLEARANCE_UM: f64 = 250.0;

/// Applies the `(autoroute_settings ...)` of a DSN file (`DsnFileSettings`, merged over the defaults).
pub fn apply_dsn_autoroute_settings(settings: &mut RouterSettings, dsn: &DsnAutorouteSettings, layer_count: usize) {
    if settings.layer_count() != layer_count {
        settings.set_layer_count(layer_count);
    }
    settings.autorouter.enabled = dsn.run_router;
    settings.optimizer.enabled = dsn.run_optimizer;
    if let Some(v) = dsn.vias_allowed {
        settings.vias_allowed = v;
    }
    if let Some(v) = dsn.via_costs {
        settings.set_via_costs(v);
    }
    if let Some(v) = dsn.plane_via_costs {
        settings.set_plane_via_costs(v);
    }
    if let Some(v) = dsn.start_ripup_costs {
        settings.set_start_ripup_costs(v);
    }
    for (i, l) in dsn.layers.iter().enumerate().take(layer_count) {
        if let Some(a) = l.active {
            settings.set_layer_active(i, a);
        }
        if let Some(h) = l.preferred_direction_is_horizontal {
            settings.set_preferred_direction_is_horizontal(i, h);
        }
        if let Some(c) = l.preferred_direction_trace_costs {
            settings.set_preferred_direction_trace_costs(i, c);
        }
        if let Some(c) = l.against_preferred_direction_trace_costs {
            settings.set_against_preferred_direction_trace_costs(i, c);
        }
    }
}

fn physical_to_board(board: &Board, value: f64, unit: Unit) -> f64 {
    let c = &board.communication;
    match &c.coordinate_transform {
        Some(t) => Unit::scale(value * t.dsn_to_board(1.0), unit, c.unit),
        None => Unit::scale(value * c.resolution as f64, unit, c.unit),
    }
}

/// `BoardOutline.minimumPinGap`
fn minimum_pin_gap(board: &Board) -> f64 {
    let Some(outline_item) = board.get_outline() else {
        return f64::INFINITY;
    };
    let ItemKind::BoardOutline(outline) = &outline_item.kind else {
        return f64::INFINITY;
    };
    let distance_to_outline = |p: &FloatPoint| -> f64 {
        if !outline.shapes.iter().any(|s| s.contains_float(p)) {
            return 0.0;
        }
        let mut r = f64::INFINITY;
        for s in &outline.shapes {
            let n = s.border_line_count();
            for i in 0..n {
                let from = s.corner(i).to_float();
                let to = s.corner((i + 1) % n).to_float();
                r = r.min(FloatLine::new(from, to).segment_distance(p));
            }
        }
        r
    };
    let mut result = f64::INFINITY;
    for pin in board.get_pins() {
        let d = board.item(pin).as_drill().unwrap();
        for s in d.shapes.iter().flatten() {
            let mut gap = f64::INFINITY;
            if !s.is_empty() && s.is_bounded() {
                match s {
                    Shape::Circle(_) => {
                        let b = s.bounding_box();
                        for c in 0..4 {
                            gap = gap.min(distance_to_outline(&b.corner(c).to_float()));
                        }
                    }
                    _ => {
                        for c in 0..s.border_line_count() {
                            gap = gap.min(distance_to_outline(&s.corner_approx(c)));
                        }
                    }
                }
            }
            result = result.min(gap);
            if result <= 0.0 {
                return 0.0;
            }
        }
    }
    result
}

fn apply_copper_to_edge_clearance_override(board: &mut Board, settings: &RouterSettings) {
    let um = settings.copper_to_edge_clearance_um;
    if um < 0.0 {
        log::warn!("Ignoring router.copper_to_edge_clearance_um because it is negative: {um}");
        return;
    }
    let Some(outline_id) = board.items.outline_id() else {
        log::warn!("Ignoring router.copper_to_edge_clearance_um because the board outline is unavailable.");
        return;
    };
    let default_area_class = board.rules.get_default_net_class().default_item_clearance_classes.get(ItemClass::Area);
    let uses_fallback_outline_class = board.item(outline_id).clearance_class == default_area_class;
    let uses_default_value = (um - DEFAULT_COPPER_TO_EDGE_CLEARANCE_UM).abs() < 1e-9;
    if uses_default_value && !uses_fallback_outline_class {
        return;
    }
    let mut value = java_round(physical_to_board(board, um, Unit::Um));
    if uses_default_value {
        let gap = minimum_pin_gap(board);
        if gap < value as f64 {
            value = gap.floor().max(0.0) as i32;
        }
    }
    let lc = board.layer_count() as usize;
    let m = &mut board.rules.clearance_matrix;
    if m.get_no(BOARD_EDGE_CLEARANCE_CLASS_NAME) < 0 {
        m.append_class(BOARD_EDGE_CLEARANCE_CLASS_NAME);
    }
    let edge = m.get_no(BOARD_EDGE_CLEARANCE_CLASS_NAME);
    if edge < 0 {
        return;
    }
    let edge = edge as usize;
    for layer in 0..lc {
        for c in 1..m.get_class_count() {
            m.set_value(edge, c, layer, value);
            m.set_value(c, edge, layer, value);
        }
    }
    board.remove_from_trees(outline_id);
    board.item_mut(outline_id).clearance_class = edge as i32;
    board.insert_into_trees(outline_id);
}

fn apply_hole_clearance_override(board: &mut Board, settings: &RouterSettings) {
    let um = settings.hole_clearance_um;
    if um < 0.0 {
        log::warn!("Ignoring router.hole_clearance_um because it is negative: {um}");
        return;
    }
    let value = java_round(physical_to_board(board, um, Unit::Um));
    let changed = value != board.rules.get_hole_clearance();
    board.rules.set_hole_clearance(value);
    let mut reclassified = 0;
    if value > 0 {
        let keepouts: Vec<_> = board
            .items
            .iter()
            .filter(|i| {
                matches!(&i.kind, ItemKind::Obstacle(o) if matches!(o.kind, crate::board::ObstacleKind::Obstacle) && i.component_id > 0 && matches!(o.area, crate::geometry::Area::Shape(Shape::Circle(_))))
            })
            .map(|i| i.id)
            .collect();
        if !keepouts.is_empty() {
            let lc = board.layer_count() as usize;
            let m = &mut board.rules.clearance_matrix;
            if m.get_no(HOLE_EDGE_CLEARANCE_CLASS_NAME) < 0 {
                m.append_class(HOLE_EDGE_CLEARANCE_CLASS_NAME);
            }
            let hole = m.get_no(HOLE_EDGE_CLEARANCE_CLASS_NAME);
            if hole >= 0 {
                let default_area = board.rules.get_default_net_class().default_item_clearance_classes.get(ItemClass::Area);
                let m = &mut board.rules.clearance_matrix;
                for layer in 0..lc {
                    for c in 1..m.get_class_count() {
                        let v = value.max(m.get_value(default_area, c as i32, layer as i32, false));
                        m.set_value(hole as usize, c, layer, v);
                        m.set_value(c, hole as usize, layer, v);
                    }
                }
                for k in keepouts {
                    if board.item(k).clearance_class != hole {
                        board.item_mut(k).clearance_class = hole;
                        reclassified += 1;
                    }
                }
            }
        }
    }
    if changed || reclassified > 0 {
        board.reinsert_tree_items();
    }
}

/// Prepares a loaded board and the router settings for routing
/// (`applyRouterSettingsForLoadedBoard`, `applyImmediatePostLoadProcessing` and the
/// deferred clearance baseline of `HeadlessBoardManager`).
pub fn prepare_loaded_board(dsn: &DsnReadResult, board: &mut Board, settings: &mut RouterSettings) {
    let layer_count = board.layer_count() as usize;
    if let Some(a) = &dsn.autoroute_settings {
        apply_dsn_autoroute_settings(settings, a, layer_count);
    }
    if settings.layer_count() != layer_count {
        settings.set_layer_count(layer_count);
    }
    settings.apply_board_specific_optimizations(board);
    settings.apply_net_class_exclusions(board);
    apply_copper_to_edge_clearance_override(board, settings);
    apply_hole_clearance_override(board, settings);
    for name in &settings.plane_nets {
        if name.trim().is_empty() {
            continue;
        }
        let nos: Vec<i32> = board.rules.nets.get_by_name(name.trim()).iter().map(|n| n.net_number).collect();
        for no in nos {
            if let Some(n) = board.rules.nets.get_mut(no) {
                if !n.contains_plane() {
                    n.set_contains_plane(true);
                    log::info!("Configured net '{}' as a power plane net via router.plane_nets.", n.name);
                }
            }
        }
    }
    board.change_plane_as_obstacle(settings.plane_as_obstacle);
    if settings.clearance_tolerance_um.is_finite() && settings.clearance_tolerance_um >= 0.0 {
        board.rules.clearance_tolerance_um = settings.clearance_tolerance_um;
    }
    board.expand_bounding_box_to_include_all_items();
    board.reduce_nets_of_route_items();
    let (assigned, tie_pads) = board.connect_netless_pads(false, settings.fix_kicad_footprints);
    if assigned > 0 {
        log::info!(
            "{} netless pad(s) overlapping a netted pad take that pad's net{}.",
            assigned,
            if tie_pads > 0 { format!(" ({tie_pads} tie pad(s) carry several nets)") } else { String::new() }
        );
    }
    let violations = crate::drc::all_clearance_violations(board);
    board.pre_existing_clearance_violations_count = violations.len();
    board.clearance_baseline = Some(std::sync::Arc::new(crate::drc::ClearanceBaseline::new(board, &violations)));
    if !violations.is_empty() {
        log::warn!("The board has {} pre-existing clearance violation(s).", violations.len());
    }
}
