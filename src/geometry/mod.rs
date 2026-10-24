//! Exact planar geometry (port of `app.freerouting.geometry.planar`).
//!
//! All board coordinates are integers. Intersections of lines may be rational points,
//! which are represented exactly with big integers.

pub mod basics;
mod circle;
mod float;
mod int_box;
mod int_octagon;
mod line;
mod line_segment;
mod point;
mod polygon;
mod polyline;
mod shape;
mod simplex;
mod tile_shape;
mod vector;

pub use basics::{Side, Signum, CRIT_DOUBLE, CRIT_INT, SQRT2};
pub use circle::Circle;
pub use float::{FloatLine, FloatPoint};
pub use int_box::IntBox;
pub use int_octagon::{FortyfiveDegreeDirection, IntOctagon};
pub use line::Line;
pub use line_segment::LineSegment;
pub use point::{IntPoint, Point, RationalPoint};
pub use polygon::{Polygon, PolygonShape};
pub use polyline::Polyline;
pub use shape::{Area, PlacementTransform, PolylineArea, Shape};
pub use simplex::Simplex;
pub use tile_shape::{BoundingDirections, RegularTileShape, TileShape};
pub use vector::{BigIntDirection, Direction, IntDirection, IntVector, RationalVector, Vector};

#[cfg(test)]
mod tests;
