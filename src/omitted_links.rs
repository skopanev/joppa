//! A live save_object merges its relations into the relations of the object: a sent key
//! replaces its title and edges, a null removes the key, and an omitted key stays. The server
//! writes the whole map into the command before the journal stores it. So a journal event
//! always names every key, and an older event without links still gives the object none.
use crate::commands::{Command, SaveObjectOp};
use crate::model::Workspace;
use crate::objects::{address, resolve_address};

/// Write the whole relations of a save: the current relations of the object with the sent
/// keys, or the sent keys on create. A save with the removed links stays as sent, and the
/// write refuses it.
pub fn keep(w: &Workspace, command: &mut Command) {
    if let Command::SaveObject(SaveObjectOp {
        kind,
        id,
        links: None,
        relations,
        ..
    }) = command
    {
        let current = w.objects.get(resolve_address(w, &address(kind, id)));
        let current = current.map(|o| o.current().relations.clone());
        let mut whole = current.unwrap_or_default();
        if let Some(patch) = relations {
            whole = crate::relations::merge(&whole, patch);
        }
        *relations = Some(whole.into_iter().map(|(k, r)| (k, Some(r))).collect());
    }
}
