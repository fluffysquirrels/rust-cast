/// Proxy classes for the `connection` channel.
pub mod connection {
    use serde::Serialize;

    #[derive(Serialize, Debug)]
    pub struct ConnectionRequest {
        #[serde(rename = "type")]
        pub typ: String,
        #[serde(rename = "userAgent")]
        pub user_agent: String,
    }
}

/// Proxy classes for the `heartbeat` channel.
pub mod heartbeat {
    use serde::Serialize;

    #[derive(Serialize, Debug)]
    pub struct HeartBeatRequest {
        #[serde(rename = "type")]
        pub typ: String,
    }
}

/// Proxy classes for the `media` channel.
pub mod media {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Debug)]
    pub struct GetStatusRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "mediaSessionId", skip_serializing_if = "Option::is_none")]
        pub media_session_id: Option<i32>,
    }

    #[derive(Serialize, Debug)]
    pub struct MediaRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "sessionId")]
        pub session_id: String,

        #[serde(rename = "type")]
        pub typ: String,

        pub media: Media,

        #[serde(rename = "currentTime")]
        pub current_time: f64,

        #[serde(rename = "customData")]
        pub custom_data: CustomData,

        pub autoplay: bool,

        #[serde(rename = "preloadTime")]
        pub preload_time: f64,
    }

    #[derive(Serialize, Debug)]
    pub struct PlaybackGenericRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "mediaSessionId")]
        pub media_session_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "customData")]
        pub custom_data: CustomData,
    }

    #[derive(Serialize, Debug)]
    pub struct PlaybackQueueRemoveRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "mediaSessionId")]
        pub media_session_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "itemIds")]
        pub item_ids: Vec<i32>,
    }

    #[derive(Serialize, Debug)]
    pub struct PlaybackQueueInsertRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "mediaSessionId")]
        pub media_session_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "items")]
        pub items: Vec<QueueItem>,

        #[serde(rename = "currentItemIndex", skip_serializing_if = "Option::is_none")]
        pub current_item_index: Option<i32>,

        #[serde(rename = "insertBefore", skip_serializing_if = "Option::is_none")]
        pub insert_before: Option<i32>,
    }

    #[derive(Serialize, Debug)]
    pub struct PlaybackQueueLoadRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "items")]
        pub items: Vec<QueueItem>,

        #[serde(rename = "startIndex", skip_serializing_if = "Option::is_none")]
        pub start_index: Option<i32>,

        #[serde(rename = "repeatMode", skip_serializing_if = "Option::is_none")]
        pub repeat_mode: Option<String>,
    }

    #[derive(Serialize, Debug)]
    pub struct PlaybackSeekRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "mediaSessionId")]
        pub media_session_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "resumeState")]
        pub resume_state: Option<String>,

        #[serde(rename = "currentTime")]
        pub current_time: Option<f32>,

        #[serde(rename = "customData")]
        pub custom_data: CustomData,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueueItem {
        pub media: Media,

        #[serde(rename = "autoplay")]
        pub auto_play: bool,

        #[serde(rename = "startTime")]
        pub start_time: f32,

        #[serde(rename = "customData")]
        pub custom_data: CustomData,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Item {
        pub item_id: i32,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub media: Option<Media>,

        #[serde(rename = "autoplay")]
        pub auto_play: bool,

        #[serde(default)]
        pub custom_data: CustomData,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Media {
        pub content_id: String,

        #[serde(default)]
        pub stream_type: String,

        pub content_type: String,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub metadata: Option<Metadata>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub duration: Option<f32>,

        pub content_url: Option<String>,

        #[serde(default)]
        pub custom_data: CustomData,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Metadata {
        pub metadata_type: u32,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub title: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub series_title: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub album_name: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub subtitle: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub album_artist: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub artist: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub composer: Option<String>,

        #[serde(default)]
        pub images: Vec<Image>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub release_date: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub original_air_date: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub creation_date_time: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub studio: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub location: Option<String>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub latitude: Option<f64>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub longitude: Option<f64>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub season: Option<u32>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub episode: Option<u32>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub track_number: Option<u32>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub disc_number: Option<u32>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub width: Option<u32>,

        #[serde(skip_serializing_if = "Option::is_none")]
        pub height: Option<u32>,
    }

    impl Metadata {
        pub fn new(metadata_type: u32) -> Metadata {
            Metadata {
                metadata_type,
                title: None,
                series_title: None,
                album_name: None,
                subtitle: None,
                album_artist: None,
                artist: None,
                composer: None,
                images: Vec::new(),
                release_date: None,
                original_air_date: None,
                creation_date_time: None,
                studio: None,
                location: None,
                latitude: None,
                longitude: None,
                season: None,
                episode: None,
                track_number: None,
                disc_number: None,
                width: None,
                height: None,
            }
        }
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct Image {
        pub url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub width: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub height: Option<u32>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct CustomData(pub serde_json::Value);

//    pub struct CustomData {
//        #[serde(skip_serializing_if = "Option::is_none")]
//        pub queue: Option<bool>,
//    }

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

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Status {
        pub media_session_id: i32,

        pub media: Option<Media>,

        pub playback_rate: f32,
        pub player_state: String,
        pub idle_reason: Option<String>,
        pub current_time: Option<f32>,
        pub current_item_id: Option<i32>,
        pub supported_media_commands: u32,
        pub items: Option<Vec<Item>>,
    }

    #[derive(Deserialize, Debug)]
    pub struct StatusReply {
        #[serde(rename = "requestId", default)]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        pub status: Vec<Status>,
    }

    #[derive(Deserialize, Debug)]
    pub struct LoadCancelledReply {
        #[serde(rename = "requestId")]
        pub request_id: i32,
    }

    #[derive(Deserialize, Debug)]
    pub struct LoadFailedReply {
        #[serde(rename = "requestId")]
        pub request_id: i32,
    }

    #[derive(Deserialize, Debug)]
    pub struct InvalidPlayerStateReply {
        #[serde(rename = "requestId")]
        pub request_id: i32,
    }

    #[derive(Deserialize, Debug)]
    pub struct InvalidRequestReply {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        pub reason: Option<String>,
    }
}

/// Proxy classes for the `receiver` channel.
pub mod receiver {
    use serde::{Deserialize, Serialize};
    use std::borrow::Cow;

    #[derive(Serialize, Debug)]
    pub struct AppLaunchRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "appId")]
        pub app_id: String,
    }

    #[derive(Serialize, Debug)]
    pub struct AppStopRequest<'a> {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        #[serde(rename = "sessionId")]
        pub session_id: Cow<'a, str>,
    }

    #[derive(Serialize, Debug)]
    pub struct GetStatusRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,
    }

    #[derive(Serialize, Debug)]
    pub struct SetVolumeRequest {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        pub volume: Volume,
    }

    #[derive(Deserialize, Debug)]
    pub struct StatusReply {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

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

    #[derive(Clone, Deserialize, Debug, Eq, PartialEq)]
    pub struct AppNamespace {
        pub name: String,
    }

    impl From<&str> for AppNamespace {
        fn from(s: &str) -> AppNamespace {
            AppNamespace::from(s.to_string())
        }
    }

    impl From<String> for AppNamespace {
        fn from(s: String) -> AppNamespace {
            AppNamespace { name: s }
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

    #[derive(Deserialize, Debug)]
    #[serde(rename_all = "camelCase")]
    pub struct LaunchErrorReply {
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        pub reason: Option<String>,
    }

    #[derive(Deserialize, Debug)]
    pub struct InvalidRequestReply {
        #[serde(rename = "requestId")]
        pub request_id: i32,

        #[serde(rename = "type")]
        pub typ: String,

        pub reason: Option<String>,
    }


    use crate::{
        async_client::Result,
        types::{AppSession, EndpointId},
    };

    impl Application {
        pub fn to_app_session(&self, receiver_destination_id: EndpointId)
        -> Result<AppSession> {
            Ok(AppSession {
                receiver_destination_id,
                app_destination_id: self.transport_id.clone(),
                session_id: self.session_id.clone(),
            })
        }
    }
}
