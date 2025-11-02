use crate::model::image::Image;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Template {
    pub id: String,
    pub text: String,
    pub images: Option<Vec<Image>>,
}