use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    pub code: String,
    pub operation: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleanup: Vec<String>,
}
impl Error {
    pub fn new(code: &str, operation: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            operation: operation.into(),
            message: message.into(),
            status: None,
            cleanup: vec![],
        }
    }
    pub fn status(operation: &str, status: u32) -> Self {
        let mut e = Self::new(
            "deviceStatus",
            operation,
            format!("Device rejected {operation} (status {status})"),
        );
        e.status = Some(status);
        e
    }
    pub fn io(operation: &str, e: impl std::fmt::Display) -> Self {
        Self::new("io", operation, e.to_string())
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.operation, self.message)
    }
}
impl std::error::Error for Error {}
