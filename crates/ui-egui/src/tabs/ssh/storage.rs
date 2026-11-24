//! LVM presentation data is prepared once when an inventory reply arrives.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Group,
    Free,
    Pv,
    Lv,
}
pub(crate) struct Node {
    pub key: String,
    pub label: String,
    pub path: String,
    pub group: String,
    pub kind: Kind,
    pub children: Vec<Node>,
}
#[derive(Default)]
pub struct State {
    pub(crate) widths: Option<[f32; 7]>,
    pub(crate) order: Vec<usize>,
    pub(crate) sort: Option<(usize, bool)>,
    pub(crate) roots: Vec<Node>,
    pub(crate) selected: Option<String>,
    pub(crate) collapsed: BTreeSet<String>,
    pub(crate) notice: Option<String>,
    pub(crate) notice_focus: bool,
}
impl State {
    pub(crate) fn sort_rows(&mut self, rows: &[Vec<String>]) {
        self.order.clear();
        self.order.extend(0..rows.len());
        if let Some((column, descending)) = self.sort {
            self.order.sort_by(|a, b| {
                let order = rows[*a].get(column).cmp(&rows[*b].get(column));
                if descending { order.reverse() } else { order }
            });
        }
    }
    pub(crate) fn replace(&mut self, detail: &str) {
        let (pvs, rest) = detail.split_once("__TOMATO_VGS__").unwrap_or((detail, ""));
        let (vgs, lvs) = rest.split_once("__TOMATO_LVS__").unwrap_or((rest, ""));
        let mut roots = Vec::new();
        let mut groups = BTreeMap::new();
        for p in fields(vgs, 3) {
            if p[0].is_empty() || groups.contains_key(p[0]) {
                continue;
            }
            groups.insert(p[0].to_owned(), roots.len());
            roots.push(Node {
                key: format!("vg:{}", p[0]),
                label: format!("VG  {}  （容量 {}，空闲 {}）", p[0], size(p[1]), size(p[2])),
                path: String::new(),
                group: p[0].into(),
                kind: Kind::Group,
                children: Vec::new(),
            });
        }
        let mut free = Node {
            key: "free-pvs".into(),
            label: "GROUP  未加入 VG 的 PV".into(),
            path: String::new(),
            group: String::new(),
            kind: Kind::Free,
            children: Vec::new(),
        };
        for p in fields(pvs, 4).filter(|p| p[0].starts_with("/dev/")) {
            let node = Node {
                key: format!("pv:{}", p[0]),
                label: format!("PV  {}  （容量 {}，空闲 {}）", p[0], size(p[2]), size(p[3])),
                path: p[0].into(),
                group: p[1].into(),
                kind: Kind::Pv,
                children: Vec::new(),
            };
            if let Some(&i) = groups.get(p[1]) {
                roots[i].children.push(node);
            } else {
                free.children.push(node);
            }
        }
        if !free.children.is_empty() {
            roots.push(free);
        }
        for p in fields(lvs, 4).filter(|p| p[0].starts_with("/dev/")) {
            if let Some(&i) = groups.get(p[1]) {
                roots[i].children.push(Node {
                    key: format!("lv:{}", p[0]),
                    label: format!("LV  {}  （容量 {}）", p[2], size(p[3])),
                    path: p[0].into(),
                    group: p[1].into(),
                    kind: Kind::Lv,
                    children: Vec::new(),
                });
            }
        }
        self.roots = roots;
        self.selected = None;
        self.collapsed.clear();
    }
    pub(crate) fn selected_node(&self) -> Option<&Node> {
        let key = self.selected.as_deref()?;
        self.roots
            .iter()
            .flat_map(|node| std::iter::once(node).chain(&node.children))
            .find(|node| node.key == key)
    }
    pub(crate) fn fail(&mut self, error: String) {
        self.notice = Some(error);
        self.notice_focus = true;
    }
}
fn fields(text: &str, count: usize) -> impl Iterator<Item = Vec<&str>> {
    text.lines().filter_map(move |line| {
        let p: Vec<_> = line.split('|').map(str::trim).collect();
        (p.len() >= count).then_some(p)
    })
}
pub(crate) fn size(raw: &str) -> String {
    let Ok(mut value) = raw.parse::<f64>() else {
        return raw.into();
    };
    if !value.is_finite() || value < 0. {
        return raw.into();
    }
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut unit = 0;
    while value >= 1024. && unit < units.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} B")
    } else {
        format!("{value:.2} {}", units[unit])
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_keeps_java_group_order_free_pvs_and_mapped_lvs() {
        let mut state = State::default();
        state.replace("/dev/sdb1||1024|1024\n/dev/sdc1|data|2048|0\n__TOMATO_VGS__\ndata|2048|0\nempty|4096|4096\n__TOMATO_LVS__\n/dev/data/lv|data|lv|2048\n/dev/missing/lv|missing|lv|2048");
        assert_eq!(state.roots.len(), 3);
        assert_eq!(state.roots[0].children.len(), 2);
        assert!(state.roots[1].children.is_empty());
        assert_eq!(state.roots[2].kind, Kind::Free);
        state.selected = Some("lv:/dev/data/lv".into());
        assert_eq!(state.selected_node().unwrap().path, "/dev/data/lv");
        state.replace("__TOMATO_VGS__\n__TOMATO_LVS__");
        assert!(state.selected_node().is_none());
        assert!(state.roots.is_empty());
        assert_eq!(size("1099511627776"), "1.00 TiB");
        assert_eq!(size("NaN"), "NaN");
    }
}
