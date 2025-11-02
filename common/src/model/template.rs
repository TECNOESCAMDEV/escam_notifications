use crate::model::image::Image;

pub struct Template {
    pub id: String,
    pub text: String,
    pub images: Option<Vec<Image>>,
}