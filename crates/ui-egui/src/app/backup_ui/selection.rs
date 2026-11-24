//! Mirrors the original BackupDialog check tree without retaining duplicate names.
use tomato_core::backup::{Kind, Selection};

pub(crate) struct Group {
    pub kind: Kind,
    pub all: bool,
    pub custom: bool,
    pub expanded: bool,
    pub custom_expanded: bool,
    pub items: Vec<Item>,
}
pub(crate) struct Item {
    pub object: Selection,
    pub selected: bool,
}
impl Group {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            all: false,
            custom: false,
            expanded: true,
            custom_expanded: true,
            items: Vec::new(),
        }
    }
    pub fn selected_count(&self) -> usize {
        self.items.iter().filter(|item| item.selected).count()
    }
    pub fn indeterminate(&self) -> bool {
        let count = self.selected_count();
        count > 0 && count < self.items.len()
    }
    pub fn set_all(&mut self, selected: bool) {
        if self.all == selected {
            return;
        }
        self.all = selected;
        if selected {
            self.custom = false;
            for item in &mut self.items {
                item.selected = false;
            }
        }
    }
    pub fn set_custom(&mut self, selected: bool) {
        if self.custom == selected {
            return;
        }
        self.custom = selected;
        if selected {
            self.all = false;
        }
        for item in &mut self.items {
            item.selected = selected;
        }
    }
    pub fn set_item(&mut self, index: usize, selected: bool) {
        if let Some(item) = self.items.get_mut(index) {
            item.selected = selected;
        }
        // Java sets custom=true only when every child is selected. In particular,
        // partial selection does not clear a previously selected parent checkbox.
        if !self.items.is_empty() && self.selected_count() == self.items.len() {
            self.set_custom(true);
        }
    }
    pub fn selected(&self) -> impl Iterator<Item = &Selection> {
        self.items
            .iter()
            .filter(|item| self.all || item.selected)
            .map(|item| &item.object)
    }
    pub fn icon(&self) -> &'static str {
        match self.kind {
            Kind::Table => "images/connect/table.png",
            Kind::View => "images/connect/view.png",
            Kind::Function => "images/connect/function.png",
            Kind::Event => "images/connect/event.png",
        }
    }
}

pub(super) fn groups(objects: Vec<Selection>) -> [Group; 4] {
    let mut groups = [Kind::Table, Kind::View, Kind::Function, Kind::Event].map(Group::new);
    for object in objects {
        let index = match object.kind {
            Kind::Table => 0,
            Kind::View => 1,
            Kind::Function => 2,
            Kind::Event => 3,
        };
        groups[index].items.push(Item {
            object,
            selected: false,
        });
    }
    groups
}
