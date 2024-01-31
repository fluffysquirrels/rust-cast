use crate::types::{AppId, Namespace};

#[derive(Debug, Clone)]
pub enum CastMessagePayload {
    String(String),
    Binary(Vec<u8>),
}

/// Messages that are exchanged between Receiver and Sender.
#[derive(Clone, Debug)]
pub struct CastMessage {
    /// A namespace is a labeled protocol. That is, messages that are exchanged throughout the
    /// Cast ecosystem utilize namespaces to identify the protocol of the message being sent.
    pub namespace: Namespace,

    /// Unique identifier of the `sender` application.
    pub source: AppId,

    /// Unique identifier of the `receiver` application.
    pub destination: AppId,

    /// Payload data attached to the message (either string or binary).
    pub payload: CastMessagePayload,
}

impl From<String> for CastMessagePayload {
    fn from(s: String) -> CastMessagePayload {
        Self::String(s)
    }
}

impl From<Vec<u8>> for CastMessagePayload {
    fn from(b: Vec<u8>) -> CastMessagePayload {
        Self::Binary(b)
    }
}
