//! Area of the board changed by an action, used to restrict the optimization.

use crate::geometry::{FloatPoint, IntBox, IntOctagon, TileShape};

#[derive(Clone, Copy, Debug)]
struct MutableOctagon {
    lx: f64,
    ly: f64,
    rx: f64,
    uy: f64,
    ulx: f64,
    lrx: f64,
    llx: f64,
    urx: f64,
}

impl MutableOctagon {
    fn empty() -> Self {
        let max = i32::MAX as f64;
        let min = i32::MIN as f64;
        MutableOctagon {
            lx: max,
            ly: max,
            rx: min,
            uy: min,
            ulx: max,
            lrx: min,
            llx: max,
            urx: min,
        }
    }

    fn to_int(&self) -> IntOctagon {
        if self.rx < self.lx || self.uy < self.ly || self.lrx < self.ulx || self.urx < self.llx {
            return IntOctagon::EMPTY;
        }
        IntOctagon::new(
            self.lx.floor() as i32,
            self.ly.floor() as i32,
            self.rx.ceil() as i32,
            self.uy.ceil() as i32,
            self.ulx.floor() as i32,
            self.lrx.ceil() as i32,
            self.llx.floor() as i32,
            self.urx.ceil() as i32,
        )
    }
}

#[derive(Clone, Debug)]
pub struct ChangedArea {
    arr: Vec<MutableOctagon>,
}

impl ChangedArea {
    pub fn new(layer_count: usize) -> ChangedArea {
        ChangedArea {
            arr: vec![MutableOctagon::empty(); layer_count],
        }
    }

    pub fn join(&mut self, p: &FloatPoint, layer: i32) {
        let c = &mut self.arr[layer as usize];
        c.lx = p.x.min(c.lx);
        c.ly = p.y.min(c.ly);
        c.rx = c.rx.max(p.x);
        c.uy = c.uy.max(p.y);
        let t = p.x - p.y;
        c.ulx = c.ulx.min(t);
        c.lrx = c.lrx.max(t);
        let t = p.x + p.y;
        c.llx = c.llx.min(t);
        c.urx = c.urx.max(t);
    }

    pub fn join_shape(&mut self, shape: &TileShape, layer: i32) {
        for i in 0..shape.border_line_count() {
            self.join(&shape.corner_approx(i), layer);
        }
    }

    pub fn get_area(&self, layer: i32) -> IntOctagon {
        self.arr[layer as usize].to_int()
    }

    pub fn surrounding_box(&self) -> IntBox {
        let mut llx = i32::MAX;
        let mut lly = i32::MAX;
        let mut urx = i32::MIN;
        let mut ury = i32::MIN;
        for c in &self.arr {
            llx = llx.min(c.lx.floor() as i32);
            lly = lly.min(c.ly.floor() as i32);
            urx = urx.max(c.rx.ceil() as i32);
            ury = ury.max(c.uy.ceil() as i32);
        }
        if llx > urx || lly > ury {
            return IntBox::EMPTY;
        }
        IntBox::new(llx, lly, urx, ury)
    }

    pub fn set_empty(&mut self, layer: i32) {
        self.arr[layer as usize] = MutableOctagon::empty();
    }

    pub fn layer_count(&self) -> usize {
        self.arr.len()
    }
}
