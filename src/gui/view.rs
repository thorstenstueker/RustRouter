//! The board view: transformation between board and screen coordinates and painting of a
//! [`DrawModel`] (colors similar to the Freerouting board frame).

use crate::model::{DrawModel, Geo, P};
use eframe::egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

pub const BACKGROUND: Color32 = Color32::from_rgb(0, 0, 0);
const OUTLINE: Color32 = Color32::from_rgb(200, 200, 200);
const AIRLINE: Color32 = Color32::from_rgb(255, 255, 255);
const VIA: Color32 = Color32::from_rgb(200, 200, 0);
const KEEPOUT: Color32 = Color32::from_rgb(130, 60, 160);

/// The layer colors (top, bottom, inner layers).
const LAYER_COLORS: [(u8, u8, u8); 8] = [
    (255, 0, 0),
    (40, 90, 255),
    (0, 200, 0),
    (230, 160, 0),
    (220, 0, 220),
    (0, 200, 200),
    (255, 120, 120),
    (150, 150, 255),
];

pub fn layer_color(layer: usize, alpha: u8) -> Color32 {
    let (r, g, b) = LAYER_COLORS[layer % LAYER_COLORS.len()];
    Color32::from_rgba_unmultiplied(r, g, b, alpha)
}

/// The visible board region: the board point shown in the center of the view and the
/// scale in pixels per board unit. The board y axis points up.
#[derive(Clone, Copy, Debug)]
pub struct View {
    center: [f64; 2],
    scale: f64,
}

impl Default for View {
    fn default() -> Self {
        View { center: [0.0, 0.0], scale: 1e-3 }
    }
}

impl View {
    pub fn fit(&mut self, m: &DrawModel, rect: Rect) {
        let [x0, y0, x1, y1] = m.bbox;
        let w = ((x1 - x0) as f64).max(1.0);
        let h = ((y1 - y0) as f64).max(1.0);
        self.center = [(x0 as f64 + x1 as f64) / 2.0, (y0 as f64 + y1 as f64) / 2.0];
        self.scale = 0.95 * (rect.width() as f64 / w).min(rect.height() as f64 / h);
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.center[0] -= delta.x as f64 / self.scale;
        self.center[1] += delta.y as f64 / self.scale;
    }

    pub fn zoom_at(&mut self, rect: Rect, pos: Pos2, factor: f32) {
        let before = self.to_board(rect, pos);
        self.scale = (self.scale * factor as f64).clamp(1e-9, 1e3);
        let after = self.to_board(rect, pos);
        self.center[0] += (before[0] - after[0]) as f64;
        self.center[1] += (before[1] - after[1]) as f64;
    }

    #[inline]
    pub fn to_screen(&self, rect: Rect, p: P) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + ((p[0] as f64 - self.center[0]) * self.scale) as f32,
            c.y - ((p[1] as f64 - self.center[1]) * self.scale) as f32,
        )
    }

    pub fn to_board(&self, rect: Rect, pos: Pos2) -> P {
        let c = rect.center();
        [
            (self.center[0] + (pos.x - c.x) as f64 / self.scale) as f32,
            (self.center[1] - (pos.y - c.y) as f64 / self.scale) as f32,
        ]
    }

    #[inline]
    fn len(&self, l: f32) -> f32 {
        (l as f64 * self.scale) as f32
    }
}

fn polygon(painter: &Painter, rect: Rect, v: &View, pts: &[P], fill: Color32) {
    let s: Vec<Pos2> = pts.iter().map(|p| v.to_screen(rect, *p)).collect();
    painter.add(Shape::convex_polygon(s, fill, Stroke::NONE));
}

fn closed_line(painter: &Painter, rect: Rect, v: &View, pts: &[P], stroke: Stroke) {
    let s: Vec<Pos2> = pts.iter().map(|p| v.to_screen(rect, *p)).collect();
    painter.add(Shape::closed_line(s, stroke));
}

fn paint_layer(painter: &Painter, rect: Rect, m: &DrawModel, v: &View, layer: usize, active: bool) {
    let alpha = if active { 255 } else { 110 };
    let color = layer_color(layer, alpha);
    let visible = rect.expand(50.0);
    for a in m.areas.iter().filter(|a| a.layer == layer) {
        if a.is_keepout {
            closed_line(painter, rect, v, &a.border, Stroke::new(1.0, KEEPOUT.gamma_multiply(alpha as f32 / 255.0)));
        } else {
            for piece in &a.fill {
                polygon(painter, rect, v, piece, layer_color(layer, alpha / 4));
            }
            closed_line(painter, rect, v, &a.border, Stroke::new(1.0, color));
        }
    }
    for t in m.traces.iter().filter(|t| t.layer == layer) {
        let pts: Vec<Pos2> = t.points.iter().map(|p| v.to_screen(rect, *p)).collect();
        if !visible.intersects(Rect::from_points(&pts)) {
            continue;
        }
        let w = v.len(t.width).max(1.0);
        if w > 3.0 {
            for p in &pts {
                painter.circle_filled(*p, w / 2.0, color);
            }
        }
        painter.add(Shape::line(pts, Stroke::new(w, color)));
    }
    let pad_color = layer_color(layer, if active { 230 } else { 100 });
    for p in m.pads.iter().filter(|p| p.layer == layer && !p.is_via) {
        match &p.geo {
            Geo::Polygon(pts) => polygon(painter, rect, v, pts, pad_color),
            Geo::Circle(c, r) => {
                painter.circle_filled(v.to_screen(rect, *c), v.len(*r).max(1.0), pad_color);
            }
        }
    }
}

pub fn paint(
    painter: &Painter,
    rect: Rect,
    m: &DrawModel,
    v: &View,
    active_layer: usize,
    visible: &[bool],
    show_airlines: bool,
) {
    let painter = painter.with_clip_rect(rect);
    for o in &m.outline {
        closed_line(&painter, rect, v, o, Stroke::new(1.5, OUTLINE));
    }
    let is_visible = |l: usize| visible.get(l).copied().unwrap_or(true);
    // The inactive layers from the bottom up, the active layer on top.
    for layer in (0..m.layers.len()).rev() {
        if layer != active_layer && is_visible(layer) {
            paint_layer(&painter, rect, m, v, layer, false);
        }
    }
    if is_visible(active_layer) {
        paint_layer(&painter, rect, m, v, active_layer, true);
    }
    // Vias: one marker per via (the shape on the topmost visible layer).
    let mut last: Option<P> = None;
    for p in m.pads.iter().filter(|p| p.is_via && is_visible(p.layer)) {
        if let Geo::Circle(c, r) = &p.geo {
            if last == Some(*c) {
                continue;
            }
            last = Some(*c);
            let s = v.to_screen(rect, *c);
            let r = v.len(*r).max(1.5);
            painter.circle_filled(s, r, VIA.gamma_multiply(0.85));
            painter.circle_filled(s, r * 0.4, BACKGROUND);
        } else if let Geo::Polygon(pts) = &p.geo {
            polygon(&painter, rect, v, pts, VIA.gamma_multiply(0.85));
        }
    }
    if show_airlines {
        let stroke = Stroke::new(1.0, AIRLINE);
        for (a, b) in &m.airlines {
            painter.line_segment([v.to_screen(rect, *a), v.to_screen(rect, *b)], stroke);
        }
    }
}
