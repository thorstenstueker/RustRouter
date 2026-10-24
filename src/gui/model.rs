//! A lightweight copy of the board geometry for drawing, extracted from a [`Board`]
//! (so the GUI thread never touches the board the routing thread works on).

use rustrouter::autoroute::pipeline::stats;
use rustrouter::board::{Board, ItemKind, ObstacleKind};
use rustrouter::geometry::{Area, FloatPoint, Shape, TileShape};
use rustrouter::settings::RouterSettings;

pub type P = [f32; 2];

#[derive(Clone, Debug)]
pub enum Geo {
    Polygon(Vec<P>),
    Circle(P, f32),
}

#[derive(Clone, Debug)]
pub struct DrawTrace {
    pub layer: usize,
    pub width: f32,
    pub points: Vec<P>,
}

#[derive(Clone, Debug)]
pub struct DrawPad {
    pub layer: usize,
    pub geo: Geo,
    pub is_via: bool,
}

#[derive(Clone, Debug)]
pub struct DrawArea {
    pub layer: usize,
    pub border: Vec<P>,
    /// Convex pieces for filling (conduction areas only).
    pub fill: Vec<Vec<P>>,
    pub is_keepout: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BoardStatsView {
    pub unrouted: i32,
    pub violations: i32,
    pub vias: i32,
    pub score: f64,
}

#[derive(Clone, Debug, Default)]
pub struct DrawModel {
    pub layers: Vec<String>,
    /// min x, min y, max x, max y in board coordinates.
    pub bbox: [f32; 4],
    pub outline: Vec<Vec<P>>,
    pub areas: Vec<DrawArea>,
    pub traces: Vec<DrawTrace>,
    pub pads: Vec<DrawPad>,
    pub airlines: Vec<(P, P)>,
    pub stats: Option<BoardStatsView>,
}

fn fp(p: &FloatPoint) -> P {
    [p.x as f32, p.y as f32]
}

fn shape_geo(s: &Shape) -> Option<Geo> {
    match s {
        Shape::Circle(c) => Some(Geo::Circle([c.center.x as f32, c.center.y as f32], c.radius as f32)),
        _ => {
            let pts: Vec<P> = s.corner_approx_arr().iter().map(fp).collect();
            (pts.len() >= 2).then_some(Geo::Polygon(pts))
        }
    }
}

fn tile_points(t: &TileShape) -> Vec<P> {
    t.corner_approx_arr().iter().map(fp).collect()
}

fn area_border(a: &Area) -> Vec<P> {
    a.get_border().corner_approx_arr().iter().map(fp).collect()
}

impl DrawModel {
    /// Extracts the drawing data. The airlines and statistics are expensive on large boards,
    /// so they are only computed when `with_airlines` is set.
    pub fn from_board(board: &Board, settings: &RouterSettings, with_airlines: bool) -> DrawModel {
        let layer_count = board.layer_structure.layers.len();
        let bb = &board.bounding_box;
        let mut m = DrawModel {
            layers: board.layer_structure.layers.iter().map(|l| l.name.clone()).collect(),
            bbox: [bb.ll.x as f32, bb.ll.y as f32, bb.ur.x as f32, bb.ur.y as f32],
            ..Default::default()
        };
        let mut outline_box: Option<[f32; 4]> = None;
        for item in board.items.iter() {
            match &item.kind {
                ItemKind::Trace(t) => {
                    let points: Vec<P> = t.polyline.corner_approx_arr().iter().map(fp).collect();
                    if t.layer >= 0 && (t.layer as usize) < layer_count {
                        m.traces.push(DrawTrace { layer: t.layer as usize, width: 2.0 * t.half_width as f32, points });
                    }
                }
                ItemKind::Pin(_) | ItemKind::Via(_) => {
                    let is_via = matches!(item.kind, ItemKind::Via(_));
                    let d = item.as_drill().expect("drill item");
                    for (i, s) in d.shapes.iter().enumerate() {
                        let layer = d.first_layer as usize + i;
                        if let (Some(s), true) = (s, layer < layer_count) {
                            if let Some(geo) = shape_geo(s) {
                                m.pads.push(DrawPad { layer, geo, is_via });
                            }
                        }
                    }
                }
                ItemKind::Obstacle(o) => {
                    if o.layer < 0 || o.layer as usize >= layer_count {
                        continue;
                    }
                    let is_conduction = matches!(o.kind, ObstacleKind::Conduction { .. });
                    let fill = if is_conduction { o.convex_pieces.iter().map(tile_points).collect() } else { Vec::new() };
                    m.areas.push(DrawArea {
                        layer: o.layer as usize,
                        border: area_border(&o.area),
                        fill,
                        is_keepout: !is_conduction,
                    });
                }
                ItemKind::BoardOutline(o) => {
                    for s in &o.shapes {
                        let pts: Vec<P> = s.corner_approx_arr().iter().map(fp).collect();
                        for p in &pts {
                            let b = outline_box.get_or_insert([p[0], p[1], p[0], p[1]]);
                            b[0] = b[0].min(p[0]);
                            b[1] = b[1].min(p[1]);
                            b[2] = b[2].max(p[0]);
                            b[3] = b[3].max(p[1]);
                        }
                        m.outline.push(pts);
                    }
                }
                ItemKind::ComponentOutline(_) => {}
            }
        }
        if let Some(b) = outline_box {
            m.bbox = b;
        }
        if with_airlines {
            m.airlines = stats::all_airlines(board).iter().map(|a| (fp(&a.from_corner), fp(&a.to_corner))).collect();
            let s = stats::board_stats(board, settings);
            m.stats = Some(BoardStatsView {
                unrouted: s.incomplete_count,
                violations: s.clearance_violation_count,
                vias: s.via_count,
                score: s.router_score as f64,
            });
        }
        m
    }
}
