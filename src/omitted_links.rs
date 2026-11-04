//! A live save_object without links keeps the links of the object: the server writes them
//! into the command before the journal stores it. So a journal event always names its
//! links, and an older event without links still gives the object none.
use crate::commands::{Command, SaveObjectOp};
use crate::model::Workspace;
use crate::objects::{address, resolve_address};

/// Fill the omitted links of a save with the current links of the object, or none on create.
pub fn keep(w: &Workspace, command: &mut Command) {
    if let Command::SaveObject(SaveObjectOp {
        kind, id, links, ..
    }) = command
        && links.is_none()
    {
        let current = w.objects.get(resolve_address(w, &address(kind, id)));
        *links = Some(
            current
                .map(|o| o.current().links.clone())
                .unwrap_or_default(),
        );
    }
}
