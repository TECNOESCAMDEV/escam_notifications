use crate::model::image::Image;

/// Represents the core content and structure of a template.
///
/// This struct is the primary Data Transfer Object (DTO) for all operations related
/// to a template's creative content. It encapsulates the template's unique identifier,
/// its main text body, and any associated images. It is used consistently across the
/// frontend and backend to ensure a common understanding of a template's structure.
///
/// ## Backend Usage:
/// - **`POST /api/templates/save`**: The `save` service receives this entire struct as a
///   JSON payload from the frontend. It uses the `id` to identify the template and
///   updates the `text` content in the `templates` table. It also synchronizes the
///   `images` list with the `images` table, adding, updating, or removing images as needed.
/// - **`GET /api/templates/{template_id}`**: The `get` service constructs this struct by
///   fetching the template's `text` and all its associated `images` from the database.
///   It is then serialized to JSON and sent to the frontend.
/// - **`GET /api/templates/pdf/{template_id}`**: The `pdf` generation service uses the
///   `id` to fetch the `text` and `images`. The `text` is parsed for layout and content
///   (including `[img:...]` tags), and the `images` data is used to embed the actual
///   visuals into the resulting PDF.
///
/// ## Frontend Usage:
/// - The frontend holds the application's state using this model. When a user edits a
///   template's text or manages its images, it is modifying a local instance of this struct.
/// - On saving, the frontend serializes its local `Template` object and sends it to the
///   backend's `save` endpoint.
/// - When loading a template, it receives this object from the backend and uses it to
///   populate the editor and image galleries.
///
/// ## Distinction from `DataSource`:
/// It is important to note that this `Template` struct is intentionally separate from the
/// `DataSource` model. `Template` deals exclusively with the *static content* and *layout*
/// of the document (the "what it looks like"). In contrast, `DataSource` (`common::model::datasource`)
/// deals with the *dynamic data* that can be merged into the template (the "what it's filled with"),
/// such as CSV column information and verification status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Template {
    /// A unique identifier for the template, typically a UUID. This is used as the
    /// primary key in the database and as the reference in API routes.
    pub id: String,
    /// The main body of the template. This string can contain plain text, Markdown-like
    /// styling (`*` for italic, `**` for bold), and special tags like `[img:image_id]`
    /// to reference an image or `{{placeholder_name}}` for data merging.
    pub text: String,
    /// An optional list of images associated with the template.
    /// - When sending to the backend (`save`), this list represents the complete set of
    ///   images that should be associated with the template.
    /// - When receiving from the backend (`get`), it contains all images currently linked
    ///   to the template in the database.
    /// It is `None` if no images are associated.
    pub images: Option<Vec<Image>>,
}
