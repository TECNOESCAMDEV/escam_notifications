#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Image {
    pub id: String,
    pub base64: String,
}
