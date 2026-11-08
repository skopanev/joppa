//! Part of `model`: the comments and the attachments of an object.
use super::*;

/// Всё, что человек навесил на объект сверх его собственных полей.
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct Notes {
    /// Пояснение к объекту. Для домена, капабилити и AC описание живёт в
    /// самом объекте — оно версионируется вместе с ним; сюда попадает
    /// описание тех объектов, у которых своего поля нет.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub comments: BTreeMap<String, Note>,
    #[serde(default)]
    pub attachments: BTreeMap<String, Attachment>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub actor: String,
    /// The alias of the event that wrote the comment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    /// Время записи в журнал, а не присланное клиентом.
    pub at: String,
    pub text: String,
    #[serde(default)]
    pub reply_to: Option<String>,
    #[serde(default)]
    pub withdrawn: bool,
}

/// Вложение: здесь только про файл, самого файла здесь нет.
///
/// Байты лежат в Spaces под workspaces/<воркспейс>/<отпечаток> — см. src/spaces.rs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub content_type: String,
    pub size: u64,
    pub hash: String,
    pub actor: String,
    /// The alias of the event that attached the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub at: String,
    #[serde(default)]
    pub removed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_at: Option<String>,
}
