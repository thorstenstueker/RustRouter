//! Optimization of traces and vias (port of `app.freerouting.board.optimize` and the
//! shove related classes of `app.freerouting.board.actions`).

pub mod shove;
pub mod trace_tightener;
pub mod via_optimizer;

pub use trace_tightener::{ExpansionCostFactor, TraceTightener};
