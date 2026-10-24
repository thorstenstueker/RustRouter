//! Matrix of clearance values between clearance classes, per layer.

pub const CLEARANCE_SAFETY_MARGIN: i32 = 16;

#[derive(Clone, Debug)]
struct Row {
    name: String,
    /// column[class_i].layer[l]
    column: Vec<Vec<i32>>,
    max_value: Vec<i32>,
}

#[derive(Clone, Debug)]
pub struct ClearanceMatrix {
    layer_count: usize,
    max_value_on_layer: Vec<i32>,
    class_count: usize,
    row: Vec<Row>,
}

impl ClearanceMatrix {
    pub fn new(class_count: usize, layer_count: usize, names: &[&str]) -> ClearanceMatrix {
        let class_count = class_count.max(1);
        let row = (0..class_count)
            .map(|i| Row {
                name: names.get(i).map(|s| s.to_string()).unwrap_or_default(),
                column: vec![vec![0; layer_count]; class_count],
                max_value: vec![0; layer_count],
            })
            .collect();
        ClearanceMatrix {
            layer_count,
            max_value_on_layer: vec![0; layer_count],
            class_count,
            row,
        }
    }

    pub fn get_default_instance(layer_count: usize, default_value: i32) -> ClearanceMatrix {
        let mut r = ClearanceMatrix::new(2, layer_count, &["null", "default"]);
        r.set_default_value(default_value);
        r
    }

    /// Returns the index of the class with the given name (case insensitive), or -1.
    pub fn get_no(&self, name: &str) -> i32 {
        for (i, r) in self.row.iter().enumerate() {
            if r.name.eq_ignore_ascii_case(name) {
                return i as i32;
            }
        }
        -1
    }

    pub fn get_name(&self, index: i32) -> Option<&str> {
        self.row.get(index as usize).map(|r| r.name.as_str())
    }

    pub fn set_default_value(&mut self, value: i32) {
        for l in 0..self.layer_count {
            self.set_default_value_on_layer(l, value);
        }
    }

    pub fn set_default_value_on_layer(&mut self, layer: usize, value: i32) {
        for i in 1..self.class_count {
            for j in 1..self.class_count {
                self.set_value(i, j, layer, value);
            }
        }
    }

    pub fn set_value_all_layers(&mut self, i: usize, j: usize, value: i32) {
        for l in 0..self.layer_count {
            self.set_value(i, j, l, value);
        }
    }

    pub fn set_value(&mut self, class_i: usize, class_j: usize, layer: usize, value: i32) {
        let mut value = value.max(0);
        if value % 2 != 0 {
            if value == i32::MAX {
                value -= 1;
            } else {
                value += 1;
            }
        }
        let row = &mut self.row[class_j];
        row.column[class_i][layer] = value;
        row.max_value[layer] = row.max_value[layer].max(value);
        self.max_value_on_layer[layer] = self.max_value_on_layer[layer].max(value);
    }

    pub fn set_inner_value(&mut self, i: usize, j: usize, value: i32) {
        for l in 1..self.layer_count.saturating_sub(1) {
            self.set_value(i, j, l, value);
        }
    }

    #[inline]
    pub fn get_value(&self, class_i: i32, class_j: i32, layer: i32, add_safety_margin: bool) -> i32 {
        if class_i < 0
            || class_i as usize >= self.class_count
            || class_j < 0
            || class_j as usize >= self.class_count
            || layer < 0
            || layer as usize >= self.layer_count
        {
            return 0;
        }
        let v = self.row[class_j as usize].column[class_i as usize][layer as usize];
        if add_safety_margin {
            v + CLEARANCE_SAFETY_MARGIN
        } else {
            v
        }
    }

    pub fn max_value(&self, class_i: i32, layer: i32) -> i32 {
        let i = class_i.max(0).min(self.class_count as i32 - 1) as usize;
        let l = layer.max(0).min(self.layer_count as i32 - 1) as usize;
        self.row[i].max_value[l]
    }

    pub fn max_value_on_layer(&self, layer: i32) -> i32 {
        let l = layer.max(0).min(self.layer_count as i32 - 1) as usize;
        self.max_value_on_layer[l]
    }

    pub fn is_layer_dependent(&self, i: usize, j: usize) -> bool {
        let e = &self.row[j].column[i];
        e.iter().skip(1).any(|v| *v != e[0])
    }

    pub fn is_inner_layer_dependent(&self, i: usize, j: usize) -> bool {
        if self.layer_count <= 2 {
            return false;
        }
        let e = &self.row[j].column[i];
        (2..self.layer_count - 1).any(|l| e[l] != e[1])
    }

    pub fn get_class_count(&self) -> usize {
        self.class_count
    }

    pub fn get_layer_count(&self) -> usize {
        self.layer_count
    }

    #[inline]
    pub fn clearance_compensation_value(&self, class: i32, layer: i32) -> i32 {
        (self.get_value(class, class, layer, false) + 1) / 2
    }

    pub fn append_class(&mut self, name: &str) -> bool {
        if self.get_no(name) >= 0 {
            return false;
        }
        let old = self.class_count;
        self.class_count += 1;
        for r in self.row.iter_mut() {
            r.column.push(vec![0; self.layer_count]);
        }
        self.row.push(Row {
            name: name.to_string(),
            column: vec![vec![0; self.layer_count]; self.class_count],
            max_value: vec![0; self.layer_count],
        });
        for i in 0..old {
            for l in 0..self.layer_count {
                let dv = self.get_value(1, i as i32, l as i32, false);
                self.set_value(old, i, l, dv);
                self.set_value(i, old, l, dv);
            }
        }
        for l in 0..self.layer_count {
            let dv = self.get_value(1, 1, l as i32, false);
            self.set_value(old, old, l, dv);
        }
        true
    }

    pub fn remove_class(&mut self, index: usize) {
        self.row.remove(index);
        for r in self.row.iter_mut() {
            r.column.remove(index);
        }
        self.class_count -= 1;
        self.recompute_max_values();
    }

    fn recompute_max_values(&mut self) {
        self.max_value_on_layer.iter_mut().for_each(|v| *v = 0);
        for r in self.row.iter_mut() {
            for l in 0..self.layer_count {
                let m = r.column.iter().map(|e| e[l]).max().unwrap_or(0);
                r.max_value[l] = m;
                self.max_value_on_layer[l] = self.max_value_on_layer[l].max(m);
            }
        }
    }

    pub fn is_equal(&self, first: i32, second: i32) -> bool {
        if first == second {
            return true;
        }
        if first < 0 || second < 0 || first as usize >= self.class_count || second as usize >= self.class_count {
            return false;
        }
        let r1 = &self.row[first as usize];
        let r2 = &self.row[second as usize];
        (1..self.class_count).all(|i| r1.column[i] == r2.column[i])
    }
}
