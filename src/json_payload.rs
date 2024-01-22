use crate::types::{MessageType, MessageTypeConst, Namespace, NamespaceConst, RequestId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payload<T>
{
    pub request_id: RequestId,

    #[serde(rename = "type")]
    pub typ: MessageType,

    #[serde(flatten)]
    pub inner: T,
}

pub type PayloadDyn = Payload<serde_json::Value>;

pub trait RequestInner: Debug + Serialize
{
    const CHANNEL_NAMESPACE: NamespaceConst;
    const TYPE_NAME: MessageTypeConst;
}

pub trait ResponseInner: Debug + DeserializeOwned
{
    const CHANNEL_NAMESPACE: NamespaceConst;
    const TYPE_NAME: MessageTypeConst;
}

pub mod receiver {
    use crate::cast::proxies::receiver as proxies;
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.receiver";

    #[derive(Debug, Serialize)]
    pub struct StatusRequest {}

    impl RequestInner for StatusRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;

        // pub const TYPE_NAME: MessageTypeConst = "GET_STATUS";
        const TYPE_NAME: MessageTypeConst = "GET_STATUS_spoiler";
    }

    #[derive(Debug, Deserialize)]
    pub struct StatusResponse {
        pub status: proxies::Status,
    }

    impl ResponseInner for StatusResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;

        // pub const TYPE_NAME: MessageTypeConst = "RECEIVER_STATUS";
        const TYPE_NAME: MessageTypeConst = "RECEIVER_STATUS_spoiler";
    }
}
