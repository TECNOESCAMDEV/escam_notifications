use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSource {
    pub id: String, // UUID
    pub csv_md5: String,
}
