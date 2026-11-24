//! Persist real backup options and selection rules without connection credentials.
use super::draft::{Draft, Options};
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Template {
    #[serde(flatten)]
    options: Options,
    #[serde(default)]
    groups: Option<[Group; 4]>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Group {
    all: bool,
    custom: bool,
    selected: Vec<String>,
}
impl Template {
    pub fn capture(draft: &Draft) -> Self {
        Self {
            options: draft.options.clone(),
            groups: Some(std::array::from_fn(|index| {
                let group = &draft.groups[index];
                Group {
                    all: group.all,
                    custom: group.custom,
                    selected: group
                        .items
                        .iter()
                        .filter(|item| item.selected)
                        .map(|item| {
                            selection_key(&item.object, draft.connection.kind() == "POSTGRESQL")
                        })
                        .collect(),
                }
            })),
        }
    }
    pub fn apply(self, draft: &mut Draft) {
        draft.options = self.options;
        draft.template_groups = self.groups;
        if !draft.loading && !draft.load_failed {
            apply_groups(draft);
        }
    }
}
pub(super) fn apply_groups(draft: &mut Draft) {
    let Some(groups) = draft.template_groups.take() else {
        return;
    };
    for (group, template) in draft.groups.iter_mut().zip(groups) {
        let names: std::collections::HashSet<_> =
            template.selected.iter().map(String::as_str).collect();
        group.all = template.all;
        group.custom = template.custom;
        for item in &mut group.items {
            item.selected = names.contains(
                selection_key(&item.object, draft.connection.kind() == "POSTGRESQL").as_str(),
            );
        }
    }
}
fn selection_key(object: &tomato_core::backup::Selection, postgres: bool) -> String {
    if postgres {
        serde_json::to_string(&(&object.table.schema, &object.table.name))
            .expect("backup name JSON")
    } else {
        object.table.name.clone()
    }
}
