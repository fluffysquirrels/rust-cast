use anyhow::{bail, format_err};
use crate::{
    async_client::Result,
    types::{AppId, AppSession,
            EndpointId,
            MediaSessionId,
            MessageType, MessageTypeConst,
            NamespaceConst, AppSessionId},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{
    borrow::Cow,
    fmt::{self, Debug, Display},
    str::FromStr,
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

// TODO: Update USER_AGENT?
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

/// Messages and types for the media namespace, as used by the Default Media Receiver app.
///
/// Reference: <https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages>
pub mod media {
    use super::*;

    pub const CHANNEL_NAMESPACE: NamespaceConst = "urn:x-cast:com.google.cast.media";

    pub const MESSAGE_REQUEST_TYPE_GET_STATUS: MessageTypeConst = "GET_STATUS";
    pub const MESSAGE_REQUEST_TYPE_LOAD: MessageTypeConst = "LOAD";
    pub const MESSAGE_REQUEST_TYPE_PLAY: MessageTypeConst = "PLAY";
    pub const MESSAGE_REQUEST_TYPE_PAUSE: MessageTypeConst = "PAUSE";
    pub const MESSAGE_REQUEST_TYPE_STOP: MessageTypeConst = "STOP";
    pub const MESSAGE_REQUEST_TYPE_SEEK: MessageTypeConst = "SEEK";
    pub const MESSAGE_REQUEST_TYPE_EDIT_TRACKS_INFO: MessageTypeConst = "EDIT_TRACKS_INFO";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEM_IDS: MessageTypeConst = "QUEUE_GET_ITEM_IDS";
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEM_RANGE: MessageTypeConst
        = "QUEUE_GET_ITEM_RANGE";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEMS: MessageTypeConst = "QUEUE_GET_ITEMS";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_INSERT: MessageTypeConst = "QUEUE_INSERT";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_LOAD: MessageTypeConst = "QUEUE_LOAD";

    // Already supported by `QUEUE_UPDATE` with QueueUpdateRequestArgs::next()
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_NEXT: MessageTypeConst = "QUEUE_NEXT";

    // Already supported by `QUEUE_UPDATE` with QueueUpdateRequestArgs::prev()
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_PREV: MessageTypeConst = "QUEUE_PREV";

    pub const MESSAGE_REQUEST_TYPE_QUEUE_REMOVE: MessageTypeConst = "QUEUE_REMOVE";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_REORDER: MessageTypeConst = "QUEUE_REORDER";
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_SHUFFLE: MessageTypeConst = "QUEUE_SHUFFLE";
    pub const MESSAGE_REQUEST_TYPE_QUEUE_UPDATE: MessageTypeConst = "QUEUE_UPDATE";

    pub const MESSAGE_REQUEST_TYPE_SET_PLAYBACK_RATE: MessageTypeConst = "SET_PLAYBACK_RATE";

    pub const MESSAGE_RESPONSE_TYPE_MEDIA_STATUS: MessageTypeConst = "MEDIA_STATUS";
    pub const MESSAGE_RESPONSE_TYPE_LOAD_CANCELLED: MessageTypeConst = "LOAD_CANCELLED";
    pub const MESSAGE_RESPONSE_TYPE_LOAD_FAILED: MessageTypeConst = "LOAD_FAILED";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE: MessageTypeConst
        = "INVALID_PLAYER_STATE";
    pub const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: MessageTypeConst = "INVALID_REQUEST";
    pub const MESSAGE_RESPONSE_TYPE_QUEUE_ITEM_IDS: MessageTypeConst = "QUEUE_ITEM_IDS";
    pub const MESSAGE_RESPONSE_TYPE_QUEUE_ITEMS: MessageTypeConst = "QUEUE_ITEMS";

    mod shared {
        use super::*;

        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        pub struct Image {
            pub url: String,
            pub width: Option<u32>,
            pub height: Option<u32>,
        }

        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Media {
            /// Typically a URL for the content.
            pub content_id: String,
            pub content_type: MimeType,

            /// If missing, `content_id` is used as a URL.
            pub content_url: Option<String>,

            #[serde(default)]
            pub custom_data: CustomData,

            pub duration: Option<Seconds>,
            pub media_category: Option<MediaCategory>,
            pub metadata: Option<Metadata>,
            pub stream_type: Option<StreamType>,
            pub text_track_style: Option<TextTrackStyle>,
            pub tracks: Option<Vec<Track>>,
        }

        impl Media {
            pub fn from_url(url: impl Into<String>) -> Media {
                Self::from_content_id(url)
            }

            pub fn from_content_id(content_id: impl Into<String>) -> Media {
                Media {
                    content_id: content_id.into(),
                    content_type: MimeType::default(),
                    content_url: None,
                    custom_data: CustomData::default(),
                    duration: None,
                    media_category: None,
                    metadata: None,
                    stream_type: None,
                    text_track_style: None,
                    tracks: None,
                }
            }

            pub fn with_track(mut self, track: Track) -> Result<Media> {
                match &mut self.tracks {
                    None => self.tracks = Some(vec![
                        Track {
                            track_id: track.track_id.or(Some(TRACK_ID_FIRST)),
                            .. track
                        }
                    ]),
                    Some(tracks) => {
                        let max_id = tracks.iter()
                                           .flat_map(|t| t.track_id)
                                           .max()
                                           .unwrap_or(0);
                        let next_id = max_id.checked_add(1)
                            .ok_or_else(|| format_err!(
                                "with_track: \
                                 TrackID overflowed while calculating 1 + current maximum."))?;
                        tracks.push(
                            Track {
                                track_id: Some(next_id),
                                .. track
                            });
                    },
                };

                Ok(self)
            }
        }

        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Metadata {
            pub metadata_type: u32,

            pub album_artist: Option<String>,
            pub album_name: Option<String>,
            pub artist: Option<String>,
            pub composer: Option<String>,
            pub creation_date_time: Option<String>,
            pub disc_number: Option<u32>,
            pub episode: Option<u32>,
            pub height: Option<u32>,

            #[serde(default)]
            pub images: Vec<Image>,

            pub latitude: Option<f64>,
            pub location: Option<String>,
            pub longitude: Option<f64>,
            pub original_air_date: Option<String>,
            pub release_date: Option<String>,
            pub season: Option<u32>,
            pub series_title: Option<String>,
            pub studio: Option<String>,
            pub subtitle: Option<String>,
            pub title: Option<String>,
            pub track_number: Option<u32>,
            pub width: Option<u32>,
        }

        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct QueueData {
            pub description: Option<String>,
            pub items: Option<Vec<QueueItem>>,
            pub id: Option<String>,
            pub name: Option<String>,
            pub repeat_mode: Option<RepeatMode>,
            pub shuffle: Option<bool>,
            pub start_index: Option<u32>,
            pub start_time: Option<Seconds>,
        }

        #[serde_with::serde_as]
        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct QueueItem {
            pub active_track_ids: Option<Vec<TrackId>>,

            // Sometimes in testing this was wrapped by the Chromecast in a JSON string.
            // TODO: Custom deserialise approach. E.g.:
            //       1. Function with `serde_as`
            //       2. struct OptionBoolJson(serde_json::Value),
            //          implementing TryInto<Option<bool>>
            // For now, just ignore it on error.
            #[serde_as(deserialize_as = "serde_with::DefaultOnError")]
            pub autoplay: Option<bool>,

            #[serde(default)]
            pub custom_data: CustomData,

            /// Must be missing for load requests, is assigned by the
            /// receiver, then must be present for further requests,
            /// and will be present for responses.
            pub item_id: Option<ItemId>,
            pub media: Option<Media>,

            /// Used to track original order of an item in the queue to undo shuffle.
            /// [Documentation](https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages.QueueItem#orderId)
            pub order_id: Option<u32>,

            pub playback_duration: Option<Seconds>,
            pub preload_time: Option<Seconds>,
            pub start_time: Option<Seconds>,
        }

        impl Default for QueueItem {
            fn default() -> QueueItem {
                QueueItem {
                    active_track_ids: None,
                    autoplay: None,
                    custom_data: CustomData::default(),
                    item_id: None,
                    media: None,
                    order_id: None,
                    playback_duration: None,
                    preload_time: None,
                    start_time: None,
                }
            }
        }

        impl QueueItem {
            pub fn from_url(url: &str) -> QueueItem {
                QueueItem {
                    media: Some(Media::from_url(url)),
                    .. QueueItem::default()
                }
            }
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Status {
            #[serde(rename = "status")]
            pub entries: Vec<StatusEntry>,
        }

        impl Status {
            pub fn try_find_media_session_id(&self) -> Result<MediaSessionId> {
                let Some(media_session_id) =
                    self.entries.first()
                        .map(|s| s.media_session_id) else
                {
                    bail!("No media status entry\n\
                           _ media_status = {self:#?}");
                };

                Ok(media_session_id)
            }
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct StatusEntry {
            pub media_session_id: MediaSessionId,

            pub active_track_ids: Option<Vec<TrackId>>,
            pub current_item_id: Option<ItemId>,
            pub current_time: Option<Seconds>,
            pub idle_reason: Option<IdleReason>,
            pub items: Option<Vec<QueueItem>>,
            pub loading_item_id: Option<ItemId>,
            pub media: Option<Media>,
            pub playback_rate: f64,
            pub player_state: PlayerState,
            pub preloaded_item_id: Option<ItemId>,
            pub queue_data: Option<QueueData>,
            pub repeat_mode: Option<RepeatMode>,

            /// Bit field.
            /// * `1` `Pause`;
            /// * `2` `Seek`;
            /// * `4` `Stream volume`;
            /// * `8` `Stream mute`;
            /// * `16` `Skip forward`;
            /// * `32` `Skip backward`;
            /// * `1 << 12` `Unknown`;
            /// * `1 << 13` `Unknown`;
            /// * `1 << 18` `Unknown`.
            // TODO: Replace this with a bitfield type.
            // Crate [modular-bitfield](https://docs.rs/modular-bitfield)
            // looks excellent.
            pub supported_media_commands: u32,

            // This field seems invalid. Volume level is always 1.0 in testing.
            // pub volume: crate::payload::receiver::Volume,
        }

        #[serde_with::serde_as]
        #[skip_serializing_none]
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct TextTrackStyle {
            pub background_color: Option<Color>,

            #[serde(default)]
            pub custom_data: CustomData,

            pub edge_color: Option<Color>,

            pub edge_type: Option<TextTrackEdgeType>,

            pub font_family: Option<String>,

            pub font_generic_family: Option<FontGenericFamily>,

            /// Default scaling is 1.0.
            #[serde_as(as = "serde_with::PickFirst<(_, FontScale)>")]
            #[serde(default)]
            pub font_scale: Option<f64>,

            pub font_style: Option<FontStyle>,
            pub foreground_color: Option<Color>,
            pub window_color: Option<Color>,

            /// Rounded corner radius absolute value in pixels (px).
            /// This value will be ignored if window_type is not RoundedCorners.
            pub window_rounded_corner_radius: Option<f64>,

            pub window_type: Option<TextTrackWindowType>,
        }

        serde_with::serde_conv!(
            FontScale,
            Option<f64>,
            |fs: &Option<f64>| *fs,
            |ser: Option<String>| -> Result<Option<f64>, std::num::ParseFloatError> {
                ser.map(|s| f64::from_str(&s)).transpose()
            }
        );

        impl TextTrackStyle {
            pub fn empty() -> TextTrackStyle {
                TextTrackStyle {
                    background_color: None,
                    custom_data: CustomData::default(),
                    edge_color: None,
                    edge_type: None,
                    font_family: None,
                    font_generic_family: None,
                    font_scale: None,
                    font_style: None,
                    foreground_color: None,
                    window_color: None,
                    window_rounded_corner_radius: None,
                    window_type: None,
                }
            }

            pub fn basic() -> TextTrackStyle {
                TextTrackStyle {
                    background_color: Some("#00000099".to_string()),
                    font_family: Some("Droid Sans".to_string()),
                    font_generic_family: Some(FontGenericFamily::SansSerif),
                    font_scale: Some(1.2),
                    font_style: Some(FontStyle::Normal),
                    foreground_color: Some("#ffff00ff".to_string()),

                    .. TextTrackStyle::empty()
                }
            }
        }

        #[skip_serializing_none]
        #[derive(Clone, Debug, Default, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Track {
            #[serde(default)]
            pub custom_data: CustomData,
            pub is_inband: Option<bool>,
            pub language: Option<String>,
            pub name: Option<String>,
            pub subtype: Option<TextTrackType>,

            /// Typically a URL for the track.
            pub track_content_id: Option<String>,

            pub track_content_type: Option<MimeType>,

            /// Must be set before serialising.
            ///
            /// See: `Media.with_track`.
            pub track_id: Option<TrackId>,

            #[serde(rename = "type")]
            pub track_type: Option<TrackType>,
        }

        impl Track {
            pub fn vtt_subtitles_from_url(url: &str) -> Track {
                Track {
                    subtype: Some(TextTrackType::Subtitles),
                    track_content_id: Some(url.to_string()),
                    track_content_type: Some(MimeType::from(MIME_TEXT_VTT)),
                    track_type: Some(TrackType::Text),
                    .. Track::default()
                }
            }
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum FontGenericFamily {
            SansSerif,
            MonospacedSansSerif,
            Serif,
            MonospacedSerif,
            Casual,
            Cursive,
            SmallCapitals,

            #[serde(untagged, skip_serializing)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum FontStyle {
            Normal,
            Bold,
            BoldItalic,
            Italic,

            #[serde(untagged, skip_serializing)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum IdleReason {
            Cancelled,
            Interrupted,
            Finished,

            #[serde(untagged, skip_serializing)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum MediaCategory {
            Audio,
            Video,
            Image,

            #[serde(untagged, skip_serializing)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum PlayerState {
            Idle,
            Playing,
            Paused,
            Buffering,

            #[serde(untagged, skip_serializing)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum RepeatMode {
            #[serde(rename = "REPEAT_OFF")]
            Off,

            #[serde(rename = "REPEAT_ALL")]
            All,

            #[serde(rename = "REPEAT_ALL_AND_SHUFFLE")]
            AllAndShuffle,

            #[serde(rename = "REPEAT_SINGLE")]
            Single,

            #[serde(untagged, skip_serializing)]
            #[clap(skip)]
            Unknown(String),
        }

        impl Default for RepeatMode {
            fn default() -> RepeatMode {
                RepeatMode::Off
            }
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum StreamType {
            Buffered,
            Live,
            Other,

            #[serde(untagged, skip_serializing)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum TextTrackEdgeType {
            None,
            Outline,
            DropShadow,
            Raised,
            Depressed,

            #[serde(untagged, skip_serializing)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum TextTrackType {
            Captions,
            Chapters,
            Descriptions,
            Metadata,
            Subtitles,

            #[serde(untagged, skip_serializing)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum TextTrackWindowType {
            None,
            Normal,
            RoundedCorners,

            #[serde(untagged, skip_serializing)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum TrackType {
            Audio,
            Video,
            Text,

            #[serde(untagged, skip_serializing)]
            Unknown(String),
        }

        #[derive(Debug, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct MediaRequestCommon {
            pub custom_data: CustomData,
            pub media_session_id: MediaSessionId,
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct CustomData(pub serde_json::Value);

        impl Default for CustomData {
            fn default() -> CustomData {
                CustomData::new()
            }
        }

        impl<T> From<T> for CustomData
        where T: Into<serde_json::Value>
        {
            fn from(v: T) -> CustomData {
                CustomData(v.into())
            }
        }

        impl CustomData {
            pub fn new() -> CustomData {
                CustomData(serde_json::Value::Null)
            }
        }

        /// Expected format: "#RRGGBBAA".
        // TODO: Switch to a strongly typed version.
        pub type Color = String;

        pub type ItemId = i32;
        pub type Seconds = f64;

        pub type TrackId = i32;
        pub const TRACK_ID_FIRST: TrackId = 1;

        pub type MimeType = String;
        pub const MIME_TEXT_VTT: &str = "text/vtt";
    }
    pub use self::shared::*;

    pub mod small_debug {
        use crate::util::fmt::{DebugNoAlternate, opt_field};
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
                f.debug_struct("MediaStatusEntry")
                    .field("player_state", &self.0.player_state)
                    .field("current_time",
                           &DebugNoAlternate(&self.0.current_time))
                    .field("media", &self.0.media.as_ref().map(|m| Media(m)))
                    .field("idle_reason",
                           &DebugNoAlternate(&self.0.idle_reason))
                    .field("media_session_id", &self.0.media_session_id)
                    .field("current_item_id",
                           &DebugNoAlternate(&self.0.current_item_id))
                    .field("repeat_mode",
                           &DebugNoAlternate(&self.0.repeat_mode))
                    // .field("items", &self.0.items)

                    // Volume level seems to be always 1.0, no point showing it.
                    // .field("volume", &Volume(&self.0.volume))

                    .finish()
            }
        }

        impl<'a> Debug for Media<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct("Media")
                    .field("content_id", &self.0.content_id)
                    .field("content_url", &self.0.content_url)
                    .field("stream_type",
                           &DebugNoAlternate(&self.0.stream_type))
                    .field("content_type", &self.0.content_type)
                    .field("duration",
                           &DebugNoAlternate(&self.0.duration))
                    .field("metadata", &self.0.metadata.as_ref().map(|m| Metadata(m)))
                    .finish()
            }
        }

        impl<'a> Debug for Metadata<'a> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                let mut s = f.debug_struct("Metadata");

                opt_field(&mut s, "artist", &self.0.artist);
                opt_field(&mut s, "album_name", &self.0.album_name);
                opt_field(&mut s, "title", &self.0.title);

                opt_field(&mut s, "series_title", &self.0.series_title);
                opt_field(&mut s, "subtitle", &self.0.subtitle);
                opt_field(&mut s, "season", &self.0.season);
                opt_field(&mut s, "episode", &self.0.episode);

                s.finish()
            }
        }
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



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoadRequest {
        #[serde(flatten)]
        pub args: LoadRequestArgs,

        #[serde(rename = "sessionId")]
        pub app_session_id: AppSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoadRequestArgs {
        pub active_track_ids: Option<Vec<TrackId>>,
        pub autoplay: Option<bool>,
        pub current_time: Option<Seconds>,
        pub custom_data: CustomData,
        pub media: Media,
        pub playback_rate: Option<f64>,
        pub queue_data: Option<QueueData>,
    }

    impl RequestInner for LoadRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_LOAD;
    }

    impl LoadRequestArgs {
        pub fn from_url(url: &str) -> LoadRequestArgs {
            LoadRequestArgs {
                media: Media::from_url(url),
                active_track_ids: None,
                autoplay: Some(true),
                current_time: None,
                custom_data: CustomData::default(),
                playback_rate: None,
                queue_data: None,
            }
        }
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



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct EditTracksInfoRequest {
        #[serde(flatten)]
        pub args: EditTracksInfoRequestArgs,
        pub media_session_id: MediaSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct EditTracksInfoRequestArgs {
        pub active_track_ids: Option<Vec<TrackId>>,
        pub text_track_style: Option<TextTrackStyle>,
    }

    impl RequestInner for EditTracksInfoRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_EDIT_TRACKS_INFO;
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



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueGetItemsRequest {
        #[serde(flatten)]
        pub args: QueueGetItemsRequestArgs,

        pub media_session_id: MediaSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueGetItemsRequestArgs {
        pub custom_data: CustomData,
        pub item_ids: Vec<ItemId>,
    }

    impl RequestInner for QueueGetItemsRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEMS;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueGetItemsResponse {
        pub items: Vec<QueueItem>,
    }

    impl ResponseInner for QueueGetItemsResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_QUEUE_ITEMS,
        ];
    }

    simple_media_request!(QueueGetItemIdsRequest, MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEM_IDS);

    #[derive(Debug, Deserialize)]
    #[serde(tag = "type", rename_all = "camelCase")]
    pub enum QueueGetItemIdsResponse {
        #[serde(rename = "QUEUE_ITEM_IDS", rename_all = "camelCase")]
        Ok {
            item_ids: Vec<ItemId>,
        },

        #[serde(rename = "INVALID_PLAYER_STATE")]
        InvalidPlayerState,

        #[serde(rename = "INVALID_REQUEST")]
        InvalidRequest { reason: String },
    }

    impl ResponseInner for QueueGetItemIdsResponse {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageTypeConst] = &[
            MESSAGE_RESPONSE_TYPE_QUEUE_ITEM_IDS,
            MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueInsertRequest {
        #[serde(flatten)]
        pub args: QueueInsertRequestArgs,

        pub media_session_id: MediaSessionId,

        #[serde(rename = "sessionId")]
        pub app_session_id: AppSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueInsertRequestArgs {
        pub custom_data: CustomData,

        /// When None, insert the items to the end of the queue.
        pub insert_before: Option<ItemId>,
        pub items: Vec<QueueItem>,
    }

    impl RequestInner for QueueInsertRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_QUEUE_INSERT;
    }



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueLoadRequest {
        #[serde(flatten)]
        pub args: QueueLoadRequestArgs,

        #[serde(rename = "sessionId")]
        pub app_session_id: AppSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueLoadRequestArgs {
        pub current_time: Option<Seconds>,
        pub custom_data: CustomData,
        pub items: Vec<QueueItem>,
        pub repeat_mode: Option<RepeatMode>,

        /// Treated as 0 if None.
        pub start_index: Option<u32>,
    }

    impl RequestInner for QueueLoadRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_QUEUE_LOAD;
    }

    impl QueueLoadRequestArgs {
        pub fn from_items(items: Vec<QueueItem>) -> QueueLoadRequestArgs {
            QueueLoadRequestArgs {
                current_time: None,
                custom_data: CustomData::default(),
                items,
                repeat_mode: None,
                start_index: None,
            }
        }
    }



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueRemoveRequest {
        #[serde(flatten)]
        pub args: QueueRemoveRequestArgs,
        pub media_session_id: MediaSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueRemoveRequestArgs {
        pub custom_data: CustomData,

        pub current_item_id: Option<ItemId>,
        pub current_time: Option<Seconds>,
        pub item_ids: Vec<ItemId>,
    }

    impl RequestInner for QueueRemoveRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_QUEUE_REMOVE;
    }



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueReorderRequest {
        #[serde(flatten)]
        pub args: QueueReorderRequestArgs,
        pub media_session_id: MediaSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueReorderRequestArgs {
        pub custom_data: CustomData,

        pub current_item_id: Option<ItemId>,
        pub current_time: Option<Seconds>,

        /// When None, re-order the items to the end of the queue.
        pub insert_before: Option<ItemId>,

        /// ID's of the items to be reordered, in the new order.
        /// Other items will keep their existing order.
        pub item_ids: Vec<ItemId>,
    }

    impl RequestInner for QueueReorderRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_QUEUE_REORDER;
    }



    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueUpdateRequest {
        #[serde(flatten)]
        pub args: QueueUpdateRequestArgs,

        pub media_session_id: MediaSessionId,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueUpdateRequestArgs {
        pub current_item_id: Option<ItemId>,
        pub current_time: Option<Seconds>,
        pub custom_data: CustomData,
        pub items: Option<Vec<QueueItem>>,

        /// Play the item forward or back by this offset in the queue items list.
        pub jump: Option<i32>,
        pub repeat_mode: Option<RepeatMode>,
    }

    impl RequestInner for QueueUpdateRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_QUEUE_UPDATE;
    }

    impl QueueUpdateRequestArgs {
        pub fn jump_item(item_id: ItemId) -> QueueUpdateRequestArgs {
            QueueUpdateRequestArgs {
                current_item_id: Some(item_id),
                .. Self::empty()
            }
        }

        pub fn jump_next() -> QueueUpdateRequestArgs {
            Self::jump_offset(1)
        }

        pub fn jump_offset(offset: i32) -> QueueUpdateRequestArgs {
            QueueUpdateRequestArgs {
                jump: Some(offset),
                .. Self::empty()
            }
        }

        pub fn jump_prev() -> QueueUpdateRequestArgs {
            Self::jump_offset(-1)
        }

        pub fn empty() -> QueueUpdateRequestArgs {
            QueueUpdateRequestArgs {
                current_item_id: None,
                current_time: None,
                custom_data: CustomData::default(),
                items: None,
                jump: None,
                repeat_mode: None,
            }
        }
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SeekRequest {
        pub media_session_id: MediaSessionId,
        pub custom_data: CustomData,

        pub current_time: Option<Seconds>,
        pub resume_state: Option<ResumeState>,
    }

    impl RequestInner for SeekRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_SEEK;
    }

    #[derive(Clone, Copy, Debug, Serialize)]
    #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
    pub enum ResumeState {
        #[serde(rename = "PLAYBACK_PAUSE")]
        Pause,

        #[serde(rename = "PLAYBACK_START")]
        Start,
    }


    #[skip_serializing_none]
    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SetPlaybackRateRequest {
        pub media_session_id: MediaSessionId,

        #[serde(flatten)]
        pub args: SetPlaybackRateRequestArgs,
    }

    #[skip_serializing_none]
    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SetPlaybackRateRequestArgs {
        pub custom_data: CustomData,

        pub playback_rate: Option<f32>,

        /// Only used if `playback_rate` is `None`.
        pub relative_playback_rate: Option<f32>,
    }

    impl RequestInner for SetPlaybackRateRequest {
        const CHANNEL_NAMESPACE: NamespaceConst = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageTypeConst = MESSAGE_REQUEST_TYPE_SET_PLAYBACK_RATE;
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

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct StatusWrapper {
            pub status: Status,
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
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

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Application {
            pub app_id: AppId,

            #[serde(rename = "sessionId")]
            pub app_session_id: AppSessionId,
            pub transport_id: EndpointId,

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
                    app_session_id: self.app_session_id.clone(),
                })
            }
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
                    .field("app_session_id", &self.0.app_session_id)
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
        #[serde(rename = "sessionId")]
        pub app_session_id: AppSessionId,
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
