use serde::Deserialize;

#[derive(Deserialize)]
/// Request payload for the CSV verification endpoint.
/// Contains the template identifier (uuid) to verify.
pub struct VerifyCsvRequest {
    pub uuid: String,
}