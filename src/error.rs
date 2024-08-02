#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid YAML in TM2Source.")]
    TM2Source(#[from] serde_yaml::Error)
}