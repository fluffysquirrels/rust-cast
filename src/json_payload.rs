use crate::types::{MessageType, MessageTypeConst, /* Namespace, */ NamespaceConst, RequestId};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fmt::Debug;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Payload<T>
{
    pub request_id: Option<RequestId>,

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

pub mod connection {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.connection";

    pub const USER_AGENT: &str = "RustCast";

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectRequest {
        pub user_agent: String,
    }

    impl RequestInner for ConnectRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "CONNECT";
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectResponse {}

    impl ResponseInner for ConnectResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "CONNECT";
    }

    const MESSAGE_TYPE_CLOSE: MessageTypeConst = "CLOSE";
}

pub mod media {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.media";

    const MESSAGE_TYPE_GET_STATUS: MessageTypeConst = "GET_STATUS";
    const MESSAGE_TYPE_LOAD: MessageTypeConst = "LOAD";
    const MESSAGE_TYPE_PLAY: MessageTypeConst = "PLAY";
    const MESSAGE_TYPE_PAUSE: MessageTypeConst = "PAUSE";
    const MESSAGE_TYPE_STOP: MessageTypeConst = "STOP";
    const MESSAGE_TYPE_SEEK: MessageTypeConst = "SEEK";
    const MESSAGE_TYPE_QUEUE_REMOVE: MessageTypeConst = "QUEUE_REMOVE";
    const MESSAGE_TYPE_QUEUE_INSERT: MessageTypeConst = "QUEUE_INSERT";
    const MESSAGE_TYPE_QUEUE_LOAD: MessageTypeConst = "QUEUE_LOAD";
    const MESSAGE_TYPE_QUEUE_GET_ITEMS: MessageTypeConst = "QUEUE_GET_ITEMS";
    const MESSAGE_TYPE_QUEUE_PREV: MessageTypeConst = "QUEUE_PREV";
    const MESSAGE_TYPE_QUEUE_NEXT: MessageTypeConst = "QUEUE_NEXT";
    const MESSAGE_TYPE_MEDIA_STATUS: MessageTypeConst = "MEDIA_STATUS";
    const MESSAGE_TYPE_LOAD_CANCELLED: MessageTypeConst = "LOAD_CANCELLED";
    const MESSAGE_TYPE_LOAD_FAILED: MessageTypeConst = "LOAD_FAILED";
    const MESSAGE_TYPE_INVALID_PLAYER_STATE: MessageTypeConst = "INVALID_PLAYER_STATE";
    const MESSAGE_TYPE_INVALID_REQUEST: MessageTypeConst = "INVALID_REQUEST";
}

pub mod heartbeat {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.heartbeat";

    const MESSAGE_TYPE_PING: MessageTypeConst = "PING";
    const MESSAGE_TYPE_PONG: MessageTypeConst = "PONG";
}

pub mod receiver {
    use crate::cast::proxies::receiver as proxies;
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.receiver";

    #[derive(Debug, Serialize)]
    pub struct StatusRequest {}

    impl RequestInner for StatusRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "GET_STATUS";
    }

    #[derive(Debug, Deserialize)]
    pub struct StatusResponse {
        pub status: proxies::Status,
    }

    impl ResponseInner for StatusResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = "RECEIVER_STATUS";
    }
}
