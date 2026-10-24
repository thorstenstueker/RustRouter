//! Via definitions and via rules.

use crate::library::{PadstackId, Padstacks};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViaInfoId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViaRuleId(pub usize);

#[derive(Clone, Debug, PartialEq)]
pub struct ViaInfo {
    pub name: String,
    pub padstack: PadstackId,
    pub clearance_class: i32,
    pub attach_smd_allowed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ViaInfos {
    list: Vec<ViaInfo>,
}

impl ViaInfos {
    pub fn add(&mut self, info: ViaInfo) -> Option<ViaInfoId> {
        if self.name_exists(&info.name) {
            return None;
        }
        self.list.push(info);
        Some(ViaInfoId(self.list.len() - 1))
    }

    /// Adds a via info even if the name exists (used for derived infos).
    pub fn add_unchecked(&mut self, info: ViaInfo) -> ViaInfoId {
        self.list.push(info);
        ViaInfoId(self.list.len() - 1)
    }

    pub fn count(&self) -> usize {
        self.list.len()
    }

    pub fn get(&self, id: ViaInfoId) -> &ViaInfo {
        &self.list[id.0]
    }

    pub fn get_by_name(&self, name: &str) -> Option<ViaInfoId> {
        self.list.iter().position(|v| v.name == name).map(ViaInfoId)
    }

    pub fn name_exists(&self, name: &str) -> bool {
        self.list.iter().any(|v| v.name == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViaRule {
    pub name: String,
    list: Vec<ViaInfoId>,
}

impl ViaRule {
    pub fn new(name: &str) -> ViaRule {
        ViaRule {
            name: name.to_string(),
            list: Vec::new(),
        }
    }

    pub fn append_via(&mut self, v: ViaInfoId) {
        self.list.push(v);
    }

    pub fn remove_via(&mut self, v: ViaInfoId) -> bool {
        if let Some(p) = self.list.iter().position(|x| *x == v) {
            self.list.remove(p);
            true
        } else {
            false
        }
    }

    pub fn via_count(&self) -> usize {
        self.list.len()
    }

    pub fn get_via(&self, index: usize) -> ViaInfoId {
        self.list[index]
    }

    pub fn vias(&self) -> &[ViaInfoId] {
        &self.list
    }

    pub fn contains(&self, v: ViaInfoId) -> bool {
        self.list.contains(&v)
    }

    pub fn contains_padstack(&self, infos: &ViaInfos, ps: PadstackId) -> bool {
        self.list.iter().any(|v| infos.get(*v).padstack == ps)
    }

    pub fn get_layer_range(&self, infos: &ViaInfos, padstacks: &Padstacks, from: i32, to: i32) -> Option<ViaInfoId> {
        self.list.iter().copied().find(|v| {
            let p = padstacks.get(infos.get(*v).padstack);
            p.from_layer() == from && p.to_layer() == to
        })
    }

    pub fn swap(&mut self, a: ViaInfoId, b: ViaInfoId) -> bool {
        let ia = self.list.iter().position(|x| *x == a);
        let ib = self.list.iter().position(|x| *x == b);
        match (ia, ib) {
            (Some(i), Some(j)) => {
                self.list.swap(i, j);
                true
            }
            _ => false,
        }
    }
}
