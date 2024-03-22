use crate::{
    async_client::Result,
    types::{AppId, AppSession,
            EndpointId,
            MediaSessionId,
            MessageType, MessageTypeConst,
            NamespaceConst, SessionId},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{
    borrow::Cow,
    fmt::{self, Debug, Display},
    sync::atomic::{AtomicI32, Ordering},
};

/// i32 that represents a request_id in the Chromecast protocol.
///
/// Zero is only used in broadcast responses with no corresponding request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RequestId(i32);

pub(crate) struct RequestIdGen(AtomicI32);

impl RequestId {
    pub const BROADCAST: RequestId = RequestId(Self::BROADCAST_I32);
    const BROADCAST_I32: i32 = 0;
}

impl RequestIdGen {
    /// Some broadcasts have `request_id` 0, so skip that.
    const INITIAL_I32: i32 = RequestId::BROADCAST_I32 + 1;
}

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
    const TYPE_NAMES: &'static [MessageTypeConst];
}

pub const USER_AGENT: &str = "RustCast; https://github.com/azasypkin/rust-cast";


impl RequestId {
    pub fn inner(self) -> i32 {
        self.0
    }

    fn rpc_id_from(n: i32) -> RequestId {
        let id = RequestId(n);

        if id.is_broadcast() {
            panic!("RequestId::rpc_id_from: was broadcast = {id}");
        }

        id
    }

    pub fn is_broadcast(self) -> bool {
        self == RequestId::BROADCAST
    }

    pub fn is_rpc(self) -> bool {
        self != RequestId::BROADCAST
    }
}

impl Into<i32> for RequestId {
    fn into(self) -> i32 {
        self.0
    }
}

impl Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl RequestIdGen {
    pub(crate) fn new() -> RequestIdGen {
        RequestIdGen(AtomicI32::new(Self::INITIAL_I32))
    }

    pub(crate) fn take_next(&self) -> RequestId {
        loop {
            let id = self.0.fetch_add(1, Ordering::SeqCst);
            if id == RequestId::BROADCAST_I32 {
                // Receivers use 0 for broadcast messages, take the next value.
                continue;
            }

            return RequestId::rpc_id_from(id);
        }
    }
}

pub mod connection {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.connection";

    pub const MESSAGE_TYPE_CONNECT: MessageTypeConst = "CONNECT";
    pub const MESSAGE_TYPE_CLOSE: MessageTypeConst = "CLOSE";

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectRequest {
        pub user_agent: String,
    }

    impl RequestInner for ConnectRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_TYPE_CONNECT;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectResponse {}

    impl ResponseInner for ConnectResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[MESSAGE_TYPE_CONNECT];
    }
}

pub mod heartbeat {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.tp.heartbeat";

    pub const MESSAGE_TYPE_PING: MessageTypeConst = "PING";
    pub const MESSAGE_TYPE_PONG: MessageTypeConst = "PONG";

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Ping {}

    impl RequestInner for Ping {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_TYPE_PING;
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Pong {}

    impl RequestInner for Pong {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_TYPE_PONG;
    }
}

pub mod media {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.media";

    pub const MESSAGE_REQUEST_TYPE_GET_STATUS: MessageTypeConst = "GET_STATUS";
    pub const MESSAGE_REQUEST_TYPE_LOAD: MessageTypeConst = "LOAD";
    pub const MESSAGE_REQUEST_TYPE_PLAY: MessageTypeConst = "PLAY";
    pub const MESSAGE_REQUEST_TYPE_PAUSE: MessageTypeConst = "PAUSE";
    pub const MESSAGE_REQUEST_TYPE_STOP: MessageTypeConst = "STOP";
    pub const MESSAGE_REQUEST_TYPE_SEEK: MessageTypeConst = "SEEK";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_REMOVE: MessageTypeConst = "QUEUE_REMOVE";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_INSERT: MessageTypeConst = "QUEUE_INSERT";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_LOAD: MessageTypeConst = "QUEUE_LOAD";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEMS: MessageTypeConst = "QUEUE_GET_ITEMS";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_PREV: MessageTypeConst = "QUEUE_PREV";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_NEXT: MessageTypeConst = "QUEUE_NEXT";

    pub const MESSAGE_RESPONSE_TYPE_MEDIA_STATUS: MessageTypeConst = "MEDIA_STATUS";
    pub const MESSAGE_RESPONSE_TYPE_LOAD_CANCELLED: MessageTypeConst = "LOAD_CANCELLED";
    pub const MESSAGE_RESPONSE_TYPE_LOAD_FAILED: MessageTypeConst = "LOAD_FAILED";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE: MessageTypeConst
        = "INVALID_PLAYER_STATE";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: MessageTypeConst = "INVALID_REQUEST";

    mod shared {
        use super::*;

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Status {
            #[serde(rename = "status")]
            pub entries: Vec<StatusEntry>,
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct StatusEntry {
            pub media_session_id: i32,

            pub media: Option<Media>,

            pub playback_rate: f32,
            pub player_state: String,
            pub idle_reason: Option<String>,
            pub current_time: Option<f32>,
            pub current_item_id: Option<i32>,
            pub supported_media_commands: u32,
            pub items: Option<Vec<Item>>,

            pub repeat_mode: Option<String>,

            // This field seems invalid. Volume level is always 1.0 in testing.
            // pub volume: crate::payload::receiver::Volume,
        }

        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Media {
            pub content_id: String,

            #[serde(default)]
            pub stream_type: String,

            pub content_type: String,

            pub metadata: Option<Metadata>,

            pub duration: Option<f32>,

            pub content_url: Option<String>,

            #[serde(default)]
            pub custom_data: CustomData,
        }

        #[serde_with::serde_as]
        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Item {
            pub item_id: i32,

            pub media: Option<Media>,

            // `auto_play` seems to sometimes be serialised as a JSON string, like `"false"`.
            // Support deserialising from a bool or string, serialise as a bool.
            #[serde_as(as = "serde_with::PickFirst<(_, serde_with::DisplayFromStr)>")]

            #[serde(rename = "autoplay")]
            pub auto_play: bool,

            #[serde(default)]
            pub custom_data: CustomData,
        }

        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Metadata {
            pub metadata_type: u32,
            pub title: Option<String>,
            pub series_title: Option<String>,
            pub album_name: Option<String>,
            pub subtitle: Option<String>,
            pub album_artist: Option<String>,
            pub artist: Option<String>,
            pub composer: Option<String>,

            #[serde(default)]
            pub images: Vec<Image>,

            pub release_date: Option<String>,
            pub original_air_date: Option<String>,
            pub creation_date_time: Option<String>,
            pub studio: Option<String>,
            pub location: Option<String>,
            pub latitude: Option<f64>,
            pub longitude: Option<f64>,
            pub season: Option<u32>,
            pub episode: Option<u32>,
            pub track_number: Option<u32>,
            pub disc_number: Option<u32>,
            pub width: Option<u32>,
            pub height: Option<u32>,
        }

        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        pub struct Image {
            pub url: String,
            pub width: Option<u32>,
            pub height: Option<u32>,
        }

        #[derive(Debug, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct MediaRequestCommon {
            pub custom_data: CustomData,
            pub media_session_id: MediaSessionId,
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        pub struct CustomData(pub serde_json::Value);

        impl Default for CustomData {
            fn default() -> CustomData {
                CustomData::new()
            }
        }

        impl CustomData {
            pub fn new() -> CustomData {
                CustomData(serde_json::Value::Null)
            }
        }
    }
    pub use self::shared::*;

    pub mod small_debug {
        use crate::util::fmt::DebugInline;
        use super::*;

        pub struct MediaStatus<'a>(pub &'a super::Status);
        pub struct MediaStatusEntries<'a>(pub &'a [super::StatusEntry]);
        pub struct MediaStatusEntry<'a>(pub &'a super::StatusEntry);
        pub struct Media<'a>(pub &'a super::Media);
        pub struct Metadata<'a>(pub &'a super::Metadata);

        impl<'a> Debug for MediaStatus<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct("media::Status")
                    .field("entries", &MediaStatusEntries(&self.0.entries))
                    .finish()
            }
        }

        impl<'a> Debug for MediaStatusEntries<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let mut d = f.debug_list();
                for item in self.0 {
                    d.entry(&MediaStatusEntry(item));
                }
                d.finish()
            }
        }

        impl<'a> Debug for MediaStatusEntry<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                // `DebugInline` is used to override Formatter's `alternate` setting
                // (i.e. using `{:#?}`) to remove unnecessary whitespace.

                f.debug_struct("MediaStatusEntry")
                    .field("player_state", &self.0.player_state)
                    .field("current_time",
                           &DebugInline(&format!("{:?}", &self.0.current_time)))
                    .field("media", &self.0.media.as_ref().map(|m| Media(m)))
                    .field("idle_reason",
                           &DebugInline(&format!("{:?}", &self.0.idle_reason)))
                    .field("media_session_id", &self.0.media_session_id)
                    .field("current_item_id",
                           &DebugInline(&format!("{:?}", &self.0.current_item_id)))
                    .field("repeat_mode", &DebugInline(&format!("{:?}", &self.0.repeat_mode)))
                // .field("items", &self.0.items)

                // Volume level seems to be always 1.0, no point showing it.
                // .field("volume", &Volume(&self.0.volume))

                    .finish()
                // .finish_non_exhaustive()
            }
        }

        impl<'a> Debug for Media<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct("Media")
                    .field("content_id", &self.0.content_id)
                    .field("content_url", &self.0.content_url)
                    .field("stream_type", &self.0.stream_type)
                    .field("content_type", &self.0.content_type)
                    .field("duration",
                           // Override formatter's `alternate` setting (i.e. using `{:#?}`)
                           // to remove unnecessary whitespace.
                           &DebugInline(&format!("{:?}", &self.0.duration)))
                    .field("metadata", &self.0.metadata.as_ref().map(|m| Metadata(m)))
                    .finish()
                // .finish_non_exhaustive()
            }
        }

        impl<'a> Debug for Metadata<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let mut s = f.debug_struct("Metadata");

                opt_field(&mut s, "title", &self.0.title);
                opt_field(&mut s, "series_title", &self.0.series_title);
                opt_field(&mut s, "subtitle", &self.0.subtitle);
                opt_field(&mut s, "season", &self.0.season);
                opt_field(&mut s, "episode", &self.0.episode);

                s.finish()
                // s.finish_non_exhaustive()
            }
        }

        fn opt_field<'a, 'b: 'a>(debug_struct: &mut std::fmt::DebugStruct<'a, 'b>,
                                 name: &str, value: &Option<impl Debug>)
        {
            let Some(ref value) = value else {
                return;
            };

            debug_struct.field(name, value);
        }
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoadRequest {
        pub session_id: SessionId,

        pub media: Media,
        pub current_time: f64,

        #[serde(default)]
        pub custom_data: CustomData,
        pub autoplay: bool,
        pub preload_time: f64,
    }

    impl RequestInner for LoadRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_LOAD;
    }


    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum LoadResponse {
        #[serde(rename = "MEDIA_STATUS")]
        Ok(Status),

        #[serde(rename = "LOAD_CANCELLED")]
        LoadCancelled,

        #[serde(rename = "LOAD_FAILED")]
        LoadFailed,

        #[serde(rename = "INVALID_PLAYER_STATE")]
        InvalidPlayerState,

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest { reason: String },
    }

    impl ResponseInner for LoadResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_MEDIA_STATUS,
            MESSAGE_RESPONSE_TYPE_LOAD_CANCELLED,
            MESSAGE_RESPONSE_TYPE_LOAD_FAILED,
            MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusRequest {
        pub media_session_id: Option<MediaSessionId>,
    }

    impl RequestInner for GetStatusRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_GET_STATUS;
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum GetStatusResponse {
        #[serde(rename = "MEDIA_STATUS")]
        Ok(Status),

        #[serde(rename = "INVALID_PLAYER_STATE")]
        InvalidPlayerState,

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest { reason: String },
    }

    impl ResponseInner for GetStatusResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_MEDIA_STATUS,
            MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }



    macro_rules! simple_media_request {
        ($name: ident, $msg_type_name: path) => {
            #[derive(Debug, Serialize)]
            pub struct $name(pub MediaRequestCommon);

            impl RequestInner for $name {
                const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
                const TYPE_NAME: MessageTypeConst = $msg_type_name;
            }
        };
    }

    simple_media_request!(PlayRequest,  MESSAGE_REQUEST_TYPE_PLAY);
    simple_media_request!(PauseRequest, MESSAGE_REQUEST_TYPE_PAUSE);
    simple_media_request!(StopRequest,  MESSAGE_REQUEST_TYPE_STOP);
}

pub mod receiver {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.receiver";
    pub const CHANNEL_NAMESPACE_TYPED: AppNamespace =
        AppNamespace::from_const("urn:x-cast:com.google.cast.receiver");

    pub const MESSAGE_REQUEST_TYPE_LAUNCH: &str = "LAUNCH";
    pub const MESSAGE_REQUEST_TYPE_STOP: &str = "STOP";
    pub const MESSAGE_REQUEST_TYPE_GET_STATUS: &str = "GET_STATUS";
    pub const MESSAGE_REQUEST_TYPE_SET_VOLUME: &str = "SET_VOLUME";

    pub const MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS: &str = "RECEIVER_STATUS";
    pub const MESSAGE_RESPONSE_TYPE_LAUNCH_ERROR: &str = "LAUNCH_ERROR";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: &str = "INVALID_REQUEST";

    mod shared {
        use super::*;

        #[derive(Clone, Deserialize, Debug)]
        #[serde(rename_all = "camelCase")]
        pub struct StatusWrapper {
            pub status: Status,
        }

        #[derive(Clone, Deserialize, Debug)]
        #[serde(rename_all = "camelCase")]
        pub struct Status {
            #[serde(default)]
            pub applications: Vec<Application>,

            #[serde(default)]
            pub is_active_input: bool,

            #[serde(default)]
            pub is_stand_by: bool,

            /// Volume parameters of the currently active cast device.
            pub volume: Volume,

            // pub user_eq: ??,
        }

        #[derive(Clone, Deserialize, Debug)]
        #[serde(rename_all = "camelCase")]
        pub struct Application {
            pub app_id: String,
            pub session_id: String,
            pub transport_id: String,

            #[serde(default)]
            pub namespaces: Vec<AppNamespace>,
            pub display_name: String,
            pub status_text: String,
            pub app_type: String,
            pub icon_url: String,
            pub is_idle_screen: bool,
            pub launched_from_cloud: bool,
            pub universal_app_id: String,
        }

        impl Application {
            pub fn has_namespace(&self, ns: &str) -> bool {
                self.namespaces.iter().any(|app_ns| app_ns == ns)
            }

            pub fn to_app_session(&self, receiver_destination_id: EndpointId)
                                  -> Result<AppSession> {
                Ok(AppSession {
                    receiver_destination_id,
                    app_destination_id: self.transport_id.clone(),
                    session_id: self.session_id.clone(),
                })
            }
        }

        #[derive(Clone, Deserialize, Debug, Eq, PartialEq)]
        #[serde(rename_all = "camelCase")]
        pub struct AppNamespace {
            pub name: Cow<'static, str>,
        }

        impl AppNamespace {
            pub const fn from_const(s: &'static str) -> AppNamespace {
                AppNamespace {
                    name: Cow::Borrowed(s),
                }
            }
        }

        impl From<&str> for AppNamespace {
            fn from(s: &str) -> AppNamespace {
                AppNamespace::from(s.to_string())
            }
        }

        impl From<String> for AppNamespace {
            fn from(s: String) -> AppNamespace {
                AppNamespace { name: s.into() }
            }
        }

        impl PartialEq<str> for AppNamespace {
            fn eq(&self, other: &str) -> bool {
                self.name == other
            }
        }

        impl PartialEq<AppNamespace> for str {
            fn eq(&self, other: &AppNamespace) -> bool {
                self == other.name
            }
        }

        /// Structure that describes possible cast device volume options.
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Volume {
            /// Volume level.
            pub level: Option<f32>,
            /// Mute/unmute state.
            pub muted: Option<bool>,

            pub control_type: Option<String>,
            pub step_interval: Option<f32>,
        }
    }
    pub use self::shared::*;

    pub mod small_debug {
        use super::*;

        pub struct ReceiverStatus<'a>(pub &'a super::Status);
        pub struct Applications<'a>(pub &'a [super::Application]);
        pub struct Application<'a>(pub &'a super::Application);
        pub struct Volume<'a>(pub &'a super::Volume);

        impl<'a> Debug for ReceiverStatus<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct("receiver::Status")
                    .field("applications", &Applications(&self.0.applications))
                    .field("volume", &Volume(&self.0.volume))
                    .finish()
                // .finish_non_exhaustive()
            }
        }

        impl<'a> Debug for Applications<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let mut d = f.debug_list();
                for item in self.0 {
                    d.entry(&Application(item));
                }
                d.finish()
            }
        }

        impl<'a> Debug for Application<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct("Application")
                // .field("app_id", &self.0.app_id)
                    .field("session_id", &self.0.session_id)
                // .field("transport_id", &self.0.transport_id)
                    .field("display_name", &self.0.display_name)
                    .field("status_text", &self.0.status_text)
                // .field("namespaces", &todo)
                    .finish()
                // .finish_non_exhaustive()
            }
        }

        impl<'a> Debug for Volume<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "Volume {{ level: {level}, muted: {muted} }}",
                       level = match self.0.level {
                           None => "None".to_string(),
                           Some(l) => format!("{l:.2}"),
                       },
                       muted = match self.0.muted {
                           None => "None".to_string(),
                           Some(m) => format!("{m}"),
                       })
            }
        }
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusRequest {}

    impl RequestInner for GetStatusRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_GET_STATUS;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusResponse(pub StatusWrapper);

    impl ResponseInner for GetStatusResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS];
    }


    // TODO: Decide: split operations into modules?

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LaunchRequest {
        pub app_id: AppId,
    }

    impl RequestInner for LaunchRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_LAUNCH;
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum LaunchResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        Ok(StatusWrapper),

        #[serde(rename = "LAUNCH_ERROR")]
        Error {
            reason: String,
        },

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest {
            reason: String,
        },
    }

    impl ResponseInner for LaunchResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
            MESSAGE_RESPONSE_TYPE_LAUNCH_ERROR,
            MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS,
        ];
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct StopRequest {
        pub session_id: SessionId,
    }

    impl RequestInner for StopRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_STOP;
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum StopResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        Ok(StatusWrapper),

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest {
            reason: String,
        },
    }

    impl ResponseInner for StopResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SetVolumeRequest {
        pub volume: Volume,
    }

    impl RequestInner for SetVolumeRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_SET_VOLUME;
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type",
            rename_all = "camelCase")]
    pub enum SetVolumeResponse {
        #[serde(rename = "RECEIVER_STATUS")]
        Ok(StatusWrapper),

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest {
            reason: String,
        },
    }

    impl ResponseInner for SetVolumeResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn request_id_gen_default() {
        let gen = RequestIdGen::new();
        assert_eq!(gen.take_next().0, 1);
        assert_eq!(gen.take_next().0, 2);
        assert_eq!(gen.take_next().0, 3);
    }

    #[test]
    fn request_id_gen_overflow() {
        let gen = RequestIdGen(AtomicI32::new(i32::MAX - 1));
        assert_eq!(gen.take_next().0, i32::MAX - 1);
        assert_eq!(gen.take_next().0, i32::MAX);
        assert_eq!(gen.take_next().0, i32::MIN);
        assert_eq!(gen.take_next().0, i32::MIN + 1);
    }

    #[test]
    fn request_id_gen_skip_0() {
        let gen = RequestIdGen(AtomicI32::new(-1));
        assert_eq!(gen.take_next().0, -1);
        assert_eq!(gen.take_next().0,  1);
        assert_eq!(gen.take_next().0,  2);
    }
}
