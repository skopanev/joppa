//! Part of `store_state_current`: the large sections of the state rows. Each row becomes its
//! entry at once, so a read of a large workspace builds no JSON value of these sections.
use crate::model::{Notes, Requirement, Workspace};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Sections {
    objects: BTreeMap<String, Arc<crate::objects::Object>>,
    pub(super) unloaded: BTreeSet<String>,
    notes: BTreeMap<String, Arc<Notes>>,
    requirements: BTreeMap<String, Arc<Requirement>>,
    boundaries: BTreeMap<String, String>,
    titles: BTreeMap<String, Vec<String>>,
    overrides: BTreeSet<String>,
    archived: BTreeSet<String>,
}

impl Sections {
    /// Take the row of a large section. False for meta and the small sections.
    pub(super) fn take(&mut self, section: &str, key: &str, text: &str) -> Result<bool> {
        let key = key.to_owned();
        match section {
            "objects" => self
                .objects
                .insert(key, serde_json::from_str(text)?)
                .is_some(),
            "notes" => self
                .notes
                .insert(key, serde_json::from_str(text)?)
                .is_some(),
            "requirements" => self
                .requirements
                .insert(key, serde_json::from_str(text)?)
                .is_some(),
            "criterion_boundaries" => self
                .boundaries
                .insert(key, serde_json::from_str(text)?)
                .is_some(),
            "former_test_titles" => self
                .titles
                .insert(key, serde_json::from_str(text)?)
                .is_some(),
            "document_overrides" => !self.overrides.insert(key),
            "archived_objects" => !self.archived.insert(key),
            _ => return Ok(false),
        };
        Ok(true)
    }

    /// Put the sections into `state`. A meta row before migration 070 can hold a set as a list,
    /// and a row adds to a value of the meta row.
    pub(super) fn into(self, state: &mut Workspace) {
        state.objects = self.objects;
        state.unloaded_objects = self.unloaded;
        state.notes.extend(self.notes);
        state.requirements.extend(self.requirements);
        Arc::make_mut(&mut state.criterion_boundaries).extend(self.boundaries);
        Arc::make_mut(&mut state.former_test_titles).extend(self.titles);
        Arc::make_mut(&mut state.document_overrides).extend(self.overrides);
        Arc::make_mut(&mut state.archived_objects).extend(self.archived);
    }
}
