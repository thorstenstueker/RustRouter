//! `Connection`: a sequence of routable items between two contact points.

use super::engine::AutorouteEngine;
use crate::board::{Board, ItemId};
use crate::geometry::Point;
use std::collections::BTreeSet;

const DETOUR_ADD: f64 = 100.0;
const DETOUR_ITEM_COST: f64 = 0.1;

#[derive(Clone, Debug)]
pub struct Connection {
    pub start_point: Option<Point>,
    pub start_layer: i32,
    pub end_point: Option<Point>,
    pub end_layer: i32,
    pub item_list: BTreeSet<ItemId>,
}

impl Connection {
    /// `Connection.get(item)`; the result is cached in the item infos of the engine.
    pub fn get(eng: &mut AutorouteEngine, board: &Board, item: ItemId) -> Option<Connection> {
        if !board.item(item).is_routable() {
            return None;
        }
        if let Some(c) = eng.item_info.get(&item).and_then(|i| i.precalculated_connection.clone()) {
            return Some(c);
        }
        let lc = board.layer_count();
        let contacts = board.normal_contacts(item);
        let mut connection_items = BTreeSet::new();
        connection_items.insert(item);
        let mut start_point: Option<Point> = None;
        let mut start_layer = 0;
        let mut end_point: Option<Point> = None;
        let mut end_layer = 0;
        for c in contacts {
            let mut current_item = c;
            let Some(mut prev_contact_point) = board.normal_contact_point(item, current_item) else {
                continue;
            };
            let mut prev_contact_layer = board.item(item).first_common_layer(board.item(current_item), lc);
            let mut fork_found = false;
            if board.item(item).is_trace() && board.trace_contacts_at(item, &prev_contact_point, false).len() != 1 {
                fork_found = true;
            }
            let mut ring_item = current_item;
            let mut ring_point = prev_contact_point.clone();
            let mut ring_layer = prev_contact_layer;
            let mut ring_steps: i64 = 0;
            let mut ring_mark: i64 = 1;
            loop {
                ring_steps += 1;
                if ring_steps > 1 && current_item == ring_item && prev_contact_layer == ring_layer && prev_contact_point == ring_point {
                    break;
                }
                if ring_steps == ring_mark {
                    ring_item = current_item;
                    ring_point = prev_contact_point.clone();
                    ring_layer = prev_contact_layer;
                    ring_mark *= 2;
                }
                if !board.item(current_item).is_routable() || fork_found {
                    match &start_point {
                        None => {
                            start_point = Some(prev_contact_point.clone());
                            start_layer = prev_contact_layer;
                        }
                        Some(sp) => {
                            if prev_contact_point != *sp {
                                end_point = Some(prev_contact_point.clone());
                                end_layer = prev_contact_layer;
                            }
                        }
                    }
                    break;
                }
                connection_items.insert(current_item);
                let current_contacts = board.normal_contacts(current_item);
                let mut next_contact_point: Option<Point> = None;
                let mut next_contact_layer = -1;
                let mut next_contact: Option<ItemId> = None;
                for tmp in current_contacts {
                    let tmp_layer = board.item(current_item).first_common_layer(board.item(tmp), lc);
                    if tmp_layer >= 0 {
                        let Some(tmp_point) = board.normal_contact_point(current_item, tmp) else {
                            fork_found = true;
                            break;
                        };
                        if prev_contact_layer != tmp_layer || prev_contact_point != tmp_point {
                            next_contact_point = Some(tmp_point);
                            next_contact_layer = tmp_layer;
                            if next_contact.is_some() {
                                fork_found = true;
                                break;
                            }
                            next_contact = Some(tmp);
                        }
                    }
                }
                let Some(nc) = next_contact else {
                    break;
                };
                current_item = nc;
                prev_contact_point = next_contact_point.unwrap();
                prev_contact_layer = next_contact_layer;
            }
        }
        let result = Connection {
            start_point,
            start_layer,
            end_point,
            end_layer,
            item_list: connection_items,
        };
        for id in &result.item_list {
            eng.item_info_mut(*id).precalculated_connection = Some(result.clone());
        }
        Some(result)
    }

    pub fn trace_length(&self, board: &Board) -> f64 {
        self.item_list
            .iter()
            .filter_map(|id| board.item(*id).as_trace())
            .fold(0.0, |s, t| s + t.get_length())
    }

    pub fn get_detour(&self, board: &Board) -> f64 {
        let (Some(s), Some(e)) = (&self.start_point, &self.end_point) else {
            return i32::MAX as f64;
        };
        let min_trace_length = s.to_float().distance(&e.to_float());
        (self.trace_length(board) + DETOUR_ADD) / (min_trace_length + DETOUR_ADD)
            + DETOUR_ITEM_COST * (self.item_list.len() as f64 - 1.0)
    }
}
