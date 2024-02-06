use crate::types::{AppId, Namespace};
use std::fmt::Debug;

#[derive(Clone)]
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

impl Debug for CastMessagePayload {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            CastMessagePayload::String(s) => {
                f.debug_struct("CastMessagePayload::String")
                 .field("len", &s.len())
                 .finish_non_exhaustive()?;
            },
            CastMessagePayload::Binary(v) => {
                f.debug_struct("CastMessagePayload::Binary")
                 .field("len", &v.len())
                 .finish_non_exhaustive()?;
            },
        }

        Ok(())
    }
}
