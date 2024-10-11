use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fmt::{self, Debug, Display},
};


#[derive(Clone, Debug, Hash,
         Eq, PartialEq, Ord, PartialOrd,
         Deserialize, Serialize)]
#[serde(transparent)]
pub struct EndpointId(Cow<'static, str>);

#[derive(Clone, Debug, Hash,
         Eq, PartialEq, Ord, PartialOrd,
         Deserialize, Serialize)]
#[serde(transparent)]
pub struct Namespace(Cow<'static, str>);

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
    pub source: EndpointId,

    /// Unique identifier of the `receiver` application.
    pub destination: EndpointId,

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

impl EndpointId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_broadcast(&self) -> bool {
        self.0 == Self::BROADCAST.0
    }

    pub const fn from_const(s: &'static str) -> EndpointId {
        Self(Cow::Borrowed(s))
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }

    pub const BROADCAST: EndpointId = EndpointId::from_const("*");

    pub const DEFAULT_SENDER: EndpointId = EndpointId::from_const("sender-0");
    pub const DEFAULT_RECEIVER: EndpointId = EndpointId::from_const("receiver-0");
}

impl From<&str> for EndpointId {
    fn from(s: &str) -> Self {
        Self(Cow::Owned(s.to_string()))
    }
}

impl From<String> for EndpointId {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

impl From<EndpointId> for String {
    fn from(id: EndpointId) -> String {
        id.0.into()
    }
}

impl Display for EndpointId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Namespace {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub const fn from_const(s: &'static str) -> Namespace {
        Self(Cow::Borrowed(s))
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl From<&str> for Namespace {
    fn from(s: &str) -> Self {
        Self(Cow::Owned(s.to_string()))
    }
}

impl From<String> for Namespace {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

impl From<Namespace> for String {
    fn from(id: Namespace) -> String {
        id.0.into()
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}
