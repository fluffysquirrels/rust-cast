/// Payloads used in Chromecast protocol messages.
///
/// For many `enum`s in this module, there will be fieldless variants matching
/// what's in the Chromecast documentation, and then an `Unknown(T)` variant
/// to deserialize any unknown variants returned by the Chromecast, where `T`
/// is the wire representation (mostly `String` and a few integers).
///
/// Ideally, the `Unknown` variant would be marked with `#[serde(skip_serializing)]`,
/// so serializing this variant would return an error, preventing consumers of this library
/// from serializing messages to the Chromecast with unknown variants the Chromecast probably
/// won't support. However another use case for serializing these `enum`s is to faithfully
/// pass on whatever the Chromecast returned to this library, for example serializing the
/// value to return as JSON in a web API. To support that use case, `#[serde(skip_serializing)]`
/// is not applied to the `Unknown` variants at this time.

use anyhow::{bail, format_err};
use crate::{
    async_client::{self as client, Result},
    message::{EndpointId, Namespace},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{
    borrow::{Borrow, Cow},
    fmt::{self, Debug, Display},
    str::FromStr,
    sync::atomic::{AtomicI32, Ordering},
    time::Duration,
};

pub use csscolorparser::Color;

/// i32 that represents a request_id in the Chromecast protocol.
///
/// Zero is only used in broadcast responses with no corresponding request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RequestId(i32);

pub(crate) struct RequestIdGen(AtomicI32);

#[derive(Clone, Debug,
        Hash, Eq, PartialEq, Ord, PartialOrd,
        Deserialize, Serialize)]
#[serde(transparent)]
pub struct MessageType(Cow<'static, str>);

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
    const CHANNEL_NAMESPACE: Namespace;
    const TYPE_NAME: MessageType;
}

pub trait ResponseInner: Debug + DeserializeOwned
{
    const CHANNEL_NAMESPACE: Namespace;
    const TYPE_NAMES: &'static [MessageType];
}

// TODO: Update USER_AGENT?
pub const USER_AGENT: &str = "RustCast; https://github.com/azasypkin/rust-cast";


impl RequestId {
    pub const BROADCAST: RequestId = RequestId(Self::BROADCAST_I32);
    const BROADCAST_I32: i32 = 0;

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
    /// Some broadcasts have `request_id` 0, so skip that.
    const INITIAL_I32: i32 = RequestId::BROADCAST_I32 + 1;

    pub(crate) fn new() -> RequestIdGen {
        RequestIdGen(AtomicI32::new(Self::INITIAL_I32))
    }

    pub(crate) fn take_next(&self) -> RequestId {
        loop {
            let id = self.0.fetch_add(1, Ordering::AcqRel);
            if id == RequestId::BROADCAST_I32 {
                // Receivers use 0 for broadcast messages, take the next value.
                continue;
            }

            return RequestId::rpc_id_from(id);
        }
    }
}

impl MessageType {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub const fn from_const(s: &'static str) -> MessageType {
        Self(Cow::Borrowed(s))
    }

    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl From<&str> for MessageType {
    fn from(s: &str) -> Self {
        Self(Cow::Owned(s.to_string()))
    }
}

impl From<String> for MessageType {
    fn from(s: String) -> Self {
        Self(s.into())
    }
}

impl From<MessageType> for String {
    fn from(id: MessageType) -> String {
        id.0.into()
    }
}

impl Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}


pub mod connection {
    use super::*;

    pub const CHANNEL_NAMESPACE: Namespace =
        Namespace::from_const("urn:x-cast:com.google.cast.tp.connection");

    pub const MESSAGE_TYPE_CONNECT: MessageType = MessageType::from_const("CONNECT");
    pub const MESSAGE_TYPE_CLOSE: MessageType = MessageType::from_const("CLOSE");

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectRequest {
        pub user_agent: String,
    }

    impl RequestInner for ConnectRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_TYPE_CONNECT;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ConnectResponse {}

    impl ResponseInner for ConnectResponse {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[MESSAGE_TYPE_CONNECT];
    }
}

pub mod heartbeat {
    use super::*;

    pub const CHANNEL_NAMESPACE: Namespace =
        Namespace::from_const("urn:x-cast:com.google.cast.tp.heartbeat");

    pub const MESSAGE_TYPE_PING: MessageType = MessageType::from_const("PING");
    pub const MESSAGE_TYPE_PONG: MessageType = MessageType::from_const("PONG");

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Ping {}

    impl RequestInner for Ping {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_TYPE_PING;
    }

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Pong {}

    impl RequestInner for Pong {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_TYPE_PONG;
    }
}

/// Messages and types for the media namespace, as used by the Default Media Receiver app.
///
/// Reference: <https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages>
pub mod media {
    use crate::payload::receiver::AppSessionId;
    use super::*;

    pub const CHANNEL_NAMESPACE: Namespace =
        Namespace::from_const("urn:x-cast:com.google.cast.media");

    pub const MESSAGE_REQUEST_TYPE_GET_STATUS: MessageType = MessageType::from_const("GET_STATUS");
    pub const MESSAGE_REQUEST_TYPE_LOAD: MessageType = MessageType::from_const("LOAD");
    pub const MESSAGE_REQUEST_TYPE_PLAY: MessageType = MessageType::from_const("PLAY");
    pub const MESSAGE_REQUEST_TYPE_PAUSE: MessageType = MessageType::from_const("PAUSE");
    pub const MESSAGE_REQUEST_TYPE_STOP: MessageType = MessageType::from_const("STOP");
    pub const MESSAGE_REQUEST_TYPE_SEEK: MessageType = MessageType::from_const("SEEK");
    pub const MESSAGE_REQUEST_TYPE_EDIT_TRACKS_INFO: MessageType = MessageType::from_const("EDIT_TRACKS_INFO");
    pub const MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEM_IDS: MessageType = MessageType::from_const("QUEUE_GET_ITEM_IDS");
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEM_RANGE: MessageType
        = MessageType::from_const("QUEUE_GET_ITEM_RANGE");
    pub const MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEMS: MessageType = MessageType::from_const("QUEUE_GET_ITEMS");
    pub const MESSAGE_REQUEST_TYPE_QUEUE_INSERT: MessageType = MessageType::from_const("QUEUE_INSERT");
    pub const MESSAGE_REQUEST_TYPE_QUEUE_LOAD: MessageType = MessageType::from_const("QUEUE_LOAD");

    // Already supported by `QUEUE_UPDATE` with QueueUpdateRequestArgs::next()
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_NEXT: MessageType = MessageType::from_const("QUEUE_NEXT");

    // Already supported by `QUEUE_UPDATE` with QueueUpdateRequestArgs::prev()
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_PREV: MessageType = MessageType::from_const("QUEUE_PREV");

    pub const MESSAGE_REQUEST_TYPE_QUEUE_REMOVE: MessageType = MessageType::from_const("QUEUE_REMOVE");
    pub const MESSAGE_REQUEST_TYPE_QUEUE_REORDER: MessageType = MessageType::from_const("QUEUE_REORDER");
    pub const _MESSAGE_REQUEST_TYPE_QUEUE_SHUFFLE: MessageType = MessageType::from_const("QUEUE_SHUFFLE");
    pub const MESSAGE_REQUEST_TYPE_QUEUE_UPDATE: MessageType = MessageType::from_const("QUEUE_UPDATE");

    pub const MESSAGE_REQUEST_TYPE_SET_PLAYBACK_RATE: MessageType = MessageType::from_const("SET_PLAYBACK_RATE");

    pub const MESSAGE_RESPONSE_TYPE_MEDIA_STATUS: MessageType = MessageType::from_const("MEDIA_STATUS");
    pub const MESSAGE_RESPONSE_TYPE_LOAD_CANCELLED: MessageType = MessageType::from_const("LOAD_CANCELLED");
    pub const MESSAGE_RESPONSE_TYPE_LOAD_FAILED: MessageType = MessageType::from_const("LOAD_FAILED");
    pub const MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE: MessageType
        = MessageType::from_const("INVALID_PLAYER_STATE");
    pub const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: MessageType = MessageType::from_const("INVALID_REQUEST");
    pub const MESSAGE_RESPONSE_TYPE_QUEUE_ITEM_IDS: MessageType = MessageType::from_const("QUEUE_ITEM_IDS");
    pub const MESSAGE_RESPONSE_TYPE_QUEUE_ITEMS: MessageType = MessageType::from_const("QUEUE_ITEMS");

    pub const MESSAGE_EVENT_TYPE_QUEUE_CHANGE: MessageType = MessageType::from_const("QUEUE_CHANGE");

    mod shared {
        use super::*;

        /// Unique ID for the playback of an item in this app session.
        /// This ID is set by the receiver when processing a `LOAD` message.
        #[derive(Clone, Copy, Debug,
                 Hash, Eq, PartialEq, Ord, PartialOrd,
                 Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct MediaSessionId(i32);

        impl MediaSessionId {
            pub fn to_i32(self) -> i32 {
                self.0
            }
        }

        impl From<i32> for MediaSessionId {
            fn from(id: i32) -> Self {
                Self(id)
            }
        }

        impl From<MediaSessionId> for i32 {
            fn from(id: MediaSessionId) -> i32 {
                id.0
            }
        }

        impl Display for MediaSessionId {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

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
                    content_type: MimeType::EMPTY,
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

            pub fn push_track(&mut self, track: Track) -> Result<&mut Media> {
                match &mut self.tracks {
                    None => self.tracks = Some(vec![
                        Track {
                            track_id: track.track_id.or(Some(TrackId::FIRST)),
                            .. track
                        }
                    ]),
                    Some(tracks) => {
                        let max_id_i32 = tracks.iter()
                                               .flat_map(|t| t.track_id.map(|id| id.to_i32()))
                                               .max()
                                               .unwrap_or(0_i32);
                        let next_id = max_id_i32.checked_add(1_i32)
                            .ok_or_else(|| format_err!(
                                "with_track: \
                                 TrackID overflowed while calculating 1 + current maximum."))?;
                        tracks.push(
                            Track {
                                track_id: Some(TrackId::from(next_id)),
                                .. track
                            });
                    },
                };

                Ok(self)
            }

            pub fn with_track(mut self, track: Track) -> Result<Media> {
                self.push_track(track)?;
                Ok(self)
            }
        }

        mod media_commands {
            #![allow(unused_braces)] // modular_bitfield::bitfield macro triggers this on some compiler versions.

            use serde::{Serialize, Serializer, Deserialize, Deserializer};

            /// Bitfield of supported media commands.
            ///
            /// Documentation:
            /// * [`Command` in Web Receiver API reference](https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages#.Command)
            /// * [`MediaStatus.supportedMediaCommands` in media messages guide](https://developers.google.com/cast/docs/media/messages#MediaStatus)
            #[modular_bitfield::bitfield]
            #[repr(u32)]
            #[derive(Clone, Copy, Debug)]
            pub struct MediaCommands {
                /// As bits: 1
                pause: bool,

                /// As bits: 2
                seek: bool,

                /// As bits: 4
                stream_volume: bool,

                /// As bits: 8
                stream_mute: bool,

                /// As bits: 16
                skip_forward: bool,

                /// As bits: 32
                skip_backward: bool,

                /// Ignore remainder of `u32`.
                #[skip]
                __: modular_bitfield::specifiers::B26,

                // TODO: Test and add more bits from the web receiver API reference.
            }

            /// Serialise MediaCommands as JSON with each bit extracted
            /// for easy use in the web client.
            #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
            pub(in crate::payload::media) struct MediaCommandsObject {
                pub pause: bool,
                pub seek: bool,
                pub stream_volume: bool,
                pub stream_mute: bool,
                pub skip_forward: bool,
                pub skip_backward: bool,

                pub as_bitfield: u32,
            }

            impl MediaCommands {
                pub fn serialize_as_object<S>(mc: &MediaCommands, s: S)
                -> Result<S::Ok, S::Error>
                where S: Serializer
                {
                    let proxy = MediaCommandsObject::from(*mc);
                    proxy.serialize(s)
                }

                pub fn deserialize_from_bitfield<'de, D>(d: D) -> Result<MediaCommands, D::Error>
                where D: Deserializer<'de>
                {
                    let as_u32 = u32::deserialize(d)?;
                    Ok(MediaCommands::from(as_u32))
                }
            }

            impl From<u32> for MediaCommandsObject {
                fn from(n: u32) -> MediaCommandsObject {
                    let mc = MediaCommands::from(n);
                    MediaCommandsObject::from(mc)
                }
            }

            impl From<MediaCommands> for MediaCommandsObject {
                fn from(mc: MediaCommands) -> MediaCommandsObject {
                    let as_u32 = u32::from(mc);

                    MediaCommandsObject {
                        pause: mc.pause(),
                        seek: mc.seek(),
                        stream_volume: mc.stream_volume(),
                        stream_mute: mc.stream_mute(),
                        skip_forward: mc.skip_forward(),
                        skip_backward: mc.skip_backward(),
                        as_bitfield: as_u32,
                    }
                }
            }
        }
        pub use media_commands::*;

        #[skip_serializing_none]
        #[derive(Clone, Debug, Default, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Metadata {
            pub metadata_type: MetadataType,

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
            #[serde(default)]
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

            /// Playback duration of the item in seconds
            ///
            /// This can be less than the duration of the item to only play a part of it.
            pub playback_duration: Option<Seconds>,
            pub preload_time: Option<Seconds>,

            /// Seconds from the beginning of the media to start playback.
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

            #[serde(serialize_with = "MediaCommands::serialize_as_object",
                    deserialize_with = "MediaCommands::deserialize_from_bitfield")]
            pub supported_media_commands: MediaCommands,

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
                    background_color: Some(Color::from_str("#00000099").unwrap()),
                    font_family: Some("Droid Sans".to_string()),
                    font_generic_family: Some(FontGenericFamily::SansSerif),
                    font_scale: Some(1.2),
                    font_style: Some(FontStyle::Normal),
                    foreground_color: Some(Color::from_str("#ffff00ff").unwrap()),

                    .. TextTrackStyle::empty()
                }
            }

            pub fn merge_overrides(self, overrides: TextTrackStyle) -> TextTrackStyle {
                Self {
                    background_color: overrides.background_color.or(self.background_color),
                    custom_data: overrides.custom_data.or(self.custom_data),
                    edge_color: overrides.edge_color.or(self.edge_color),
                    edge_type: overrides.edge_type.or(self.edge_type),
                    font_family: overrides.font_family.or(self.font_family),
                    font_generic_family: overrides.font_generic_family
                                                  .or(self.font_generic_family),
                    font_scale: overrides.font_scale.or(self.font_scale),
                    font_style: overrides.font_style.or(self.font_style),
                    foreground_color: overrides.foreground_color.or(self.foreground_color),
                    window_color: overrides.window_color.or(self.window_color),
                    window_rounded_corner_radius: overrides.window_rounded_corner_radius
                                                           .or(self.window_rounded_corner_radius),
                    window_type: overrides.window_type.or(self.window_type),
                }
            }
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(rename_all = "camelCase")]
        // Deliberately serialize Option::None values.
        // This forces the serialization of `track_id` when it is None,
        // which returns a validation error.
        pub struct Track {
            #[serde(default)]
            pub custom_data: CustomData,

            pub is_inband: Option<bool>,
            pub language: Option<String>,
            pub name: Option<String>,
            pub subtype: Option<TextTrackType>,

            /// Typically a URL for the track.
            ///
            /// Note: The value is wrapped in `Option` because the payload specification supports
            /// this for in-band tracks stored within the main media file.
            pub track_content_id: Option<String>,

            pub track_content_type: Option<MimeType>,

            /// Must be set before serialising.
            ///
            /// The value is wrapped in `Option` to make it easier for consumers
            /// to build a request without explicitly setting a `track_id` themselves.
            ///
            /// See: `Media.with_track`.
            #[serde(serialize_with = "serialize_track_id")]
            pub track_id: Option<TrackId>,

            #[serde(rename = "type")]
            pub track_type: TrackType,
        }

        fn serialize_track_id<S: serde::Serializer>(track_id: &Option<TrackId>, serializer: S)
        -> Result<S::Ok, S::Error>
        {
            let Some(track_id) = track_id.as_ref() else {
                return Err(serde::ser::Error::custom(
                    "Track::track_id must be set before serialization."));
            };

            serializer.serialize_i32(track_id.to_i32())
        }

        impl Track {
            pub fn vtt_subtitles_from_url(url: &str) -> Track {
                Track {
                    custom_data: CustomData::default(),
                    is_inband: None,
                    language: None,
                    name: None,
                    subtype: Some(TextTrackType::Subtitles),
                    track_content_id: Some(url.to_string()),
                    track_content_type: Some(MimeType::TEXT_VTT),
                    track_id: None,
                    track_type: TrackType::Text,
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

            #[serde(untagged)]
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

            #[serde(untagged)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum IdleReason {
            Cancelled,
            Interrupted,
            Finished,
            Error,

            #[serde(untagged)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum MediaCategory {
            Audio,
            Video,
            Image,

            #[serde(untagged)]
            Unknown(String),
        }

        #[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
        #[repr(u8)] // Discriminant stored as u8.
        #[serde(from = "i32", into = "i32")]
        pub enum MetadataType {
            #[default]
            Generic = 0,
            Movie = 1,
            TvShow = 2,
            MusicTrack = 3,
            Photo = 4,

            // TODO: Add variant AudiobookChapter = 5?,
            // This is documented, but the integer discriminant is not yet known and tested.
            // Ref: https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages#.MetadataType

            Unknown(i32),
        }

        impl From<i32> for MetadataType {
            fn from(d: i32) -> MetadataType {
                match d {
                    0 => MetadataType::Generic,
                    1 => MetadataType::Movie,
                    2 => MetadataType::TvShow,
                    3 => MetadataType::MusicTrack,
                    4 => MetadataType::Photo,

                    // TODO: Add variant AudiobookChapter = 5?
                    // 5? => MetadataType::AudiobookChapter,

                    d => MetadataType::Unknown(d),
                }
            }
        }

        impl Into<i32> for MetadataType {
            fn into(self) -> i32 {
                match self {
                    MetadataType::Generic => 0,
                    MetadataType::Movie => 1,
                    MetadataType::TvShow => 2,
                    MetadataType::MusicTrack => 3,
                    MetadataType::Photo => 4,

                    // TODO: Add variant AudiobookChapter = 5?
                    // MetadataType::AudiobookChapter => 5?,

                    MetadataType::Unknown(d) => d,
                }
            }
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum PlayerState {
            Idle,
            Playing,
            Paused,
            Buffering,

            #[serde(untagged)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

            #[serde(untagged)]
            #[clap(skip)]
            Unknown(String),
        }

        impl Default for RepeatMode {
            fn default() -> RepeatMode {
                RepeatMode::Off
            }
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum StreamType {
            Buffered,
            Live,
            Other,

            #[serde(untagged)]
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

            #[serde(untagged)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum TextTrackType {
            Captions,
            Chapters,
            Descriptions,
            Metadata,
            Subtitles,

            #[serde(untagged)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum TextTrackWindowType {
            None,
            Normal,
            RoundedCorners,

            #[serde(untagged)]
            #[clap(skip)]
            Unknown(String),
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
        pub enum TrackType {
            Audio,
            Video,
            Text,

            #[serde(untagged)]
            Unknown(String),
        }

        #[derive(Debug, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct MediaRequestCommon {
            #[serde(default)]
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

            pub fn from_serialize<T: Serialize>(v: T) -> Result<CustomData> {
                Ok(CustomData(serde_json::to_value(v)?))
            }

            pub fn to_deserialize<T: DeserializeOwned>(&self) -> Result<T> {
                self.clone().into_deserialize()
            }

            pub fn into_deserialize<T: DeserializeOwned>(self) -> Result<T> {
                Ok(serde_json::from_value(self.0)?)
            }

            pub fn is_null(&self) -> bool {
                self.0.is_null()
            }

            pub fn or(self, fallback: CustomData) -> CustomData {
                if self.is_null() {
                    fallback
                }
                else {
                    self
                }
            }

            pub fn take(&mut self) -> CustomData {
                CustomData(self.0.take())
            }
        }


        #[derive(Clone, Copy, Debug, Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct Seconds(pub f64);

        impl From<&Duration> for Seconds {
            fn from(dur: &Duration) -> Seconds {
                (*dur).into()
            }
        }

        impl From<Duration> for Seconds {
            fn from(dur: Duration) -> Seconds {
                Seconds(dur.as_secs_f64())
            }
        }

        impl From<Seconds> for Duration {
            fn from(s: Seconds) -> Duration {
                Duration::from_secs_f64(s.0)
            }
        }


        #[derive(Clone, Copy, Debug,
                 Hash, Eq, PartialEq, Ord, PartialOrd,
                 Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct ItemId(i32);

        impl ItemId {
            pub fn to_i32(self) -> i32 {
                self.0
            }
        }

        impl From<i32> for ItemId {
            fn from(id: i32) -> Self {
                Self(id)
            }
        }

        impl From<ItemId> for i32 {
            fn from(id: ItemId) -> i32 {
                id.0
            }
        }

        impl Display for ItemId {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        impl FromStr for ItemId {
            type Err = std::num::ParseIntError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(ItemId::from(i32::from_str(s)?))
            }
        }


        #[derive(Clone, Copy, Debug,
                 Hash, Eq, PartialEq, Ord, PartialOrd,
                 Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct TrackId(i32);

        impl TrackId {
            pub fn to_i32(self) -> i32 {
                self.0
            }

            pub const FIRST: TrackId = TrackId(1);
        }

        impl From<i32> for TrackId {
            fn from(id: i32) -> Self {
                Self(id)
            }
        }

        impl From<TrackId> for i32 {
            fn from(id: TrackId) -> i32 {
                id.0
            }
        }

        impl Display for TrackId {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        impl FromStr for TrackId {
            type Err = std::num::ParseIntError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok(TrackId::from(i32::from_str(s)?))
            }
        }


        #[derive(Clone, Debug, Hash,
                 Eq, PartialEq, Ord, PartialOrd,
                 Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct MimeType(Cow<'static, str>);

        impl MimeType {
            pub const EMPTY: MimeType = MimeType::from_const("");
            pub const TEXT_VTT: MimeType = MimeType::from_const("text/vtt");
        }

        impl MimeType {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub const fn from_const(s: &'static str) -> MimeType {
                Self(Cow::Borrowed(s))
            }

            pub fn to_string(&self) -> String {
                self.0.to_string()
            }
        }

        impl From<&str> for MimeType {
            fn from(s: &str) -> Self {
                Self(Cow::Owned(s.to_string()))
            }
        }

        impl From<String> for MimeType {
            fn from(s: String) -> Self {
                Self(s.into())
            }
        }

        impl From<MimeType> for String {
            fn from(m: MimeType) -> String {
                m.0.into()
            }
        }

        impl Display for MimeType {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }


        #[derive(Clone, Copy, Debug,
                 Eq, PartialEq, Ord, PartialOrd,
                 Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct SequenceNumber(i32);

        impl Display for SequenceNumber {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        impl From<i32> for SequenceNumber {
            fn from(n: i32) -> Self {
                Self(n)
            }
        }

        impl From<SequenceNumber> for i32 {
            fn from(seq: SequenceNumber) -> i32 {
                seq.0
            }
        }
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
                const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
                const TYPE_NAME: MessageType = $msg_type_name;
            }
        };
    }

    simple_media_request!(PlayRequest,  MESSAGE_REQUEST_TYPE_PLAY);
    simple_media_request!(PauseRequest, MESSAGE_REQUEST_TYPE_PAUSE);
    simple_media_request!(StopRequest,  MESSAGE_REQUEST_TYPE_STOP);

    simple_media_request!(QueueGetItemIdsRequest, MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEM_IDS);



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

        #[serde(default)]
        pub custom_data: CustomData,

        pub media: Media,
        pub playback_rate: Option<f64>,
        pub queue_data: Option<QueueData>,
    }

    impl RequestInner for LoadRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_LOAD;
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_EDIT_TRACKS_INFO;
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusRequest {
        pub media_session_id: Option<MediaSessionId>,
        pub options: GetStatusOptions,
    }

    impl RequestInner for GetStatusRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_GET_STATUS;
    }

    mod get_status_options {
        #![allow(unused_braces)] // modular_bitfield::bitfield macro triggers this on some compiler versions.

        use serde::{Deserialize, Serialize};

        /// Only documented in the Cast Web Receiver.
        ///
        /// Const definitions extracted from web cast receiver JavaScript.
        ///
        /// Ref: https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages#.GetStatusOptions
        #[modular_bitfield::bitfield]
        #[repr(u8)]
        #[derive(Clone, Copy, Debug)]
        #[derive(Deserialize, Serialize)]
        #[serde(from = "u8", into = "u8")]
        pub struct GetStatusOptions {
            /// As bits: 1
            pub no_metadata: bool,

            /// As bits: 2
            pub no_queue_items: bool,

            /// Ignore remainder of `u8`.
            #[skip]
            __: modular_bitfield::specifiers::B6,
        }

        impl Default for GetStatusOptions {
            fn default() -> Self {
                Self::new()
            }
        }
    }
    pub use get_status_options::*;

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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[
            MESSAGE_RESPONSE_TYPE_MEDIA_STATUS,
            MESSAGE_RESPONSE_TYPE_INVALID_PLAYER_STATE,
            MESSAGE_RESPONSE_TYPE_INVALID_REQUEST,
        ];
    }



    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueChangeEvent {
        pub change_type: Option<QueueChangeType>,
        pub insert_before: Option<ItemId>,
        pub item_ids: Option<Vec<ItemId>>,
        pub reorder_item_ids: Option<Vec<ItemId>>,
        pub sequence_number: Option<SequenceNumber>,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    pub enum QueueChangeType {
        /// Queue had items inserted.
        Insert,

        /// Queue had items removed.
        Remove,

        /// A list of items changed.
        ItemsChange,

        /// The queue went through an update and a new ordered list is sent.
        Update,

        /// The queue had no change.
        /// This is used to echo back when multiple senders ended up requesting the same data.
        NoChange,

        #[serde(untagged)]
        Unknown(String),
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
        #[serde(default)]
        pub custom_data: CustomData,

        pub item_ids: Vec<ItemId>,
    }

    impl RequestInner for QueueGetItemsRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_QUEUE_GET_ITEMS;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueGetItemsResponse {
        pub items: Vec<QueueItem>,
    }

    impl ResponseInner for QueueGetItemsResponse {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[
            MESSAGE_RESPONSE_TYPE_QUEUE_ITEMS,
        ];
    }



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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[
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
    #[derive(Debug, Default, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueInsertRequestArgs {
        #[serde(default)]
        pub custom_data: CustomData,

        /// When None, insert the items to the end of the queue.
        pub insert_before: Option<ItemId>,
        pub items: Vec<QueueItem>,

        // TODO: These are in the web receiver documentation, but not yet tested.
        //
        // They support jumping to a queue item as one operation while inserting to the queue.
        //
        // Ref: https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages.QueueInsertRequestData

        // pub current_item_id: Option<ItemId>,
        // pub current_item_index: Option<u32>,
        // pub current_time: Option<Seconds>,
    }

    impl RequestInner for QueueInsertRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_QUEUE_INSERT;
    }

    impl QueueInsertRequestArgs {
        pub fn from_items(items: Vec<QueueItem>) -> QueueInsertRequestArgs {
            QueueInsertRequestArgs {
                custom_data: CustomData::default(),
                insert_before: None,
                items,
            }
        }
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
    #[derive(Debug, Default, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct QueueLoadRequestArgs {
        pub current_time: Option<Seconds>,

        #[serde(default)]
        pub custom_data: CustomData,

        pub items: Vec<QueueItem>,
        pub repeat_mode: Option<RepeatMode>,

        /// Treated as 0 if None.
        pub start_index: Option<u32>,
    }

    impl RequestInner for QueueLoadRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_QUEUE_LOAD;
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
        #[serde(default)]
        pub custom_data: CustomData,

        pub current_item_id: Option<ItemId>,
        pub current_time: Option<Seconds>,
        pub item_ids: Vec<ItemId>,
    }

    impl RequestInner for QueueRemoveRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_QUEUE_REMOVE;
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
        #[serde(default)]
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_QUEUE_REORDER;
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

        #[serde(default)]
        pub custom_data: CustomData,

        pub items: Option<Vec<QueueItem>>,

        /// Play the item forward or back by this offset in the queue items list.
        pub jump: Option<i32>,
        pub repeat_mode: Option<RepeatMode>,
        pub shuffle: Option<bool>,
    }

    impl RequestInner for QueueUpdateRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_QUEUE_UPDATE;
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
                shuffle: None,
            }
        }
    }



    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SeekRequest {
        pub media_session_id: MediaSessionId,

        #[serde(flatten)]
        pub args: SeekRequestArgs,
    }

    #[derive(Debug, Deserialize, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct SeekRequestArgs {
        #[serde(default)]
        pub custom_data: CustomData,

        pub current_time: Option<Seconds>,
        pub resume_state: Option<ResumeState>,
    }

    impl RequestInner for SeekRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_SEEK;
    }

    #[derive(Clone, Copy, Debug, Deserialize, Serialize)]
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
        #[serde(default)]
        pub custom_data: CustomData,

        pub playback_rate: Option<f32>,

        /// Only used if `playback_rate` is `None`.
        pub relative_playback_rate: Option<f32>,
    }

    impl RequestInner for SetPlaybackRateRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_SET_PLAYBACK_RATE;
    }


    #[cfg(test)]
    mod test {
        use serde_json::json;
        use super::*;

        const EMPTY_MEDIA_COMMANDS_OBJECT: MediaCommandsObject = MediaCommandsObject {
            pause: false,
            seek: false,
            stream_volume: false,
            stream_mute: false,
            skip_forward: false,
            skip_backward: false,
            as_bitfield: 0_u32,
        };

        #[test]
        fn media_commands_round_trip() {
            fn case(label: &str, expected: MediaCommandsObject) {
                let input: u32 = expected.as_bitfield;
                let bitfield = MediaCommands::from(input);
                let output = MediaCommandsObject::from(input);
                let round_trip: u32 = output.as_bitfield;

                let description =
                    format!("case {label:?}:\n\
                             _ input       = {input}_u32\n\
                             _ input (hex) = {input:#x}\n\
                             _ input (bin) = {input:#b}\n\
                             _ round_trip  = {round_trip}_u32\n\
                             _ bitfield    = {bitfield:#?}\n\
                             _ output      = {output:#?}\n\
                             _ expected    = {expected:#?}\n\n\n");

                println!("{description}");

                assert_eq!(input, round_trip, "{description}");
                assert_eq!(output, expected, "{description}");
            }

            case("empty", EMPTY_MEDIA_COMMANDS_OBJECT);

            case("single bit - pause",
                 MediaCommandsObject {
                     pause: true,
                     as_bitfield: 1,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("single bit - seek",
                 MediaCommandsObject {
                     seek: true,
                     as_bitfield: 2,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("single bit - stream_volume",
                 MediaCommandsObject {
                     stream_volume: true,
                     as_bitfield: 4,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("single bit - stream_mute",
                 MediaCommandsObject {
                     stream_mute: true,
                     as_bitfield: 8,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("single bit - skip_forward",
                 MediaCommandsObject {
                     skip_forward: true,
                     as_bitfield: 16,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("single bit - skip_backward",
                 MediaCommandsObject {
                     skip_backward: true,
                     as_bitfield: 32,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("combination - pre-recorded video",
                 MediaCommandsObject {
                     pause: true,
                     seek: true,
                     skip_forward: true,
                     skip_backward: true,
                     as_bitfield: 0b110011,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("unknown bits, otherwise empty",
                 MediaCommandsObject {
                     as_bitfield: 0b1111_1111_1100_0000,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });


            case("known and unknown bits",
                 MediaCommandsObject {
                     pause: true,
                     seek: true,
                     as_bitfield: 0b1111_1111_1100_0011,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });
        }

        #[test]
        fn deserialize_status_entry_media_commands() {
            fn case(label: &str, input_commands_obj: MediaCommandsObject) {
                let input_bitfield: u32 = input_commands_obj.as_bitfield;
                let input_commands = MediaCommands::from(input_bitfield);
                let status_entry_json = json!({
                    "mediaSessionId": 1,
                    "playbackRate": 17.0,
                    "playerState": "IDLE",
                    "supportedMediaCommands": input_bitfield,
                });
                let status_entry: StatusEntry
                    = serde_json::from_value(status_entry_json.clone()).unwrap();
                let output_commands = status_entry.supported_media_commands;
                let output_bitfield = u32::from(output_commands);
                let output_commands_obj = MediaCommandsObject::from(output_bitfield);

                let description =
                    format!("case {label:?}:\n\
                             _ input_bitfield (hex)  = {input_bitfield:#x}\n\
                             _ output_bitfield (hex) = {output_bitfield:#x}\n\
                             _ input_commands_obj    = {input_commands_obj:#?}\n\
                             _ input_commands        = {input_commands:#?}\n\
                             _ output_commands       = {output_commands:#?}\n\
                             _ status_entry_json     = {status_entry_json:#?}\n\
                             _ status_entry          = {status_entry:#?}\n\n");

                println!("{description}");

                assert_eq!(input_bitfield, output_bitfield, "{description}");
                assert_eq!(input_commands_obj, output_commands_obj, "{description}");
            }

            case("empty", EMPTY_MEDIA_COMMANDS_OBJECT);

            case("single bit - seek",
                 MediaCommandsObject {
                     seek: true,
                     as_bitfield: 0b_0010,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });
            case("combined known bits",
                 MediaCommandsObject {
                     pause: true,
                     seek: true,
                     as_bitfield: 0b_0011,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("unknown bits",
                 MediaCommandsObject {
                     as_bitfield: 0b_1100_0000,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("known and unknown bits",
                 MediaCommandsObject {
                     pause: true,
                     seek: true,
                     as_bitfield: 0b_1100_0011,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });
        }

        #[test]
        fn serialize_status_entry_media_commands() {
            fn case(label: &str, input_commands_obj: MediaCommandsObject) {
                let entry = StatusEntry {
                    media_session_id: 1.into(),

                    active_track_ids: None,
                    current_item_id: None,
                    current_time: None,
                    idle_reason: None,
                    items: None,
                    loading_item_id: None,
                    media: None,
                    playback_rate: 1.0,
                    player_state: PlayerState::Idle,
                    preloaded_item_id: None,
                    queue_data: None,
                    repeat_mode: None,

                    supported_media_commands:
                        MediaCommands::from(input_commands_obj.as_bitfield),
                };

                let entry_json = serde_json::to_value(entry.clone()).unwrap();
                let output_commands_json = entry_json.get("supportedMediaCommands").unwrap();
                let expected_commands_json = serde_json::to_value(input_commands_obj.clone())
                                                        .unwrap();

                let description =
                    format!("case {label:?}:\n\
                             _ input_commands_obj     = {input_commands_obj:#?}\n\
                             _ output_commands_json   = {output_commands_json:#?}\n\
                             _ expected_commands_json = {expected_commands_json:#?}\n\
                             _ entry_json             = {entry_json:#?}\n\n");

                println!("{description}");

                assert_eq!(output_commands_json, &expected_commands_json, "{description}");
                assert_eq!(u64::from(input_commands_obj.as_bitfield),
                           expected_commands_json.get("as_bitfield").unwrap()
                                                 .as_u64().unwrap(),
                           "{description}");
            }

            case("empty", EMPTY_MEDIA_COMMANDS_OBJECT);

            case("single bit - seek",
                 MediaCommandsObject {
                     seek: true,
                     as_bitfield: 0b_0010,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });
            case("combined known bits",
                 MediaCommandsObject {
                     pause: true,
                     seek: true,
                     as_bitfield: 0b_0011,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("unknown bits",
                 MediaCommandsObject {
                     as_bitfield: 0b_1100_0000,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });

            case("known and unknown bits",
                 MediaCommandsObject {
                     pause: true,
                     seek: true,
                     as_bitfield: 0b_1100_0011,
                     ..EMPTY_MEDIA_COMMANDS_OBJECT
                 });
        }

        #[test]
        #[expect(non_snake_case)]
        fn serde_GetStatusOptions() {
            fn case(opts: GetStatusOptions, expected_as_u8: u8) {
                let as_u8 = u8::from(opts);
                let as_json = serde_json::to_string(&opts).unwrap();
                let from_json: GetStatusOptions = serde_json::from_str(&as_json).unwrap();
                let from_json_as_u8 = u8::from(from_json);

                let msg = format!(
                    "opts            = {opts:?}\n\
                     expected_as_u8  = {expected_as_u8}\n\
                     as_u8           = {as_u8}\n\
                     as_json         = {as_json:?}\n\
                     from_json       = {from_json:?}\n\
                     from_json_as_u8 = {from_json_as_u8}");

                print!("\n\
                        serde_GetStatusOptions case\n\
                        ===========================\n\
                        {msg}\n\n");

                assert_eq!(as_u8, expected_as_u8, "expected as u8\n{msg}");
                assert_eq!(as_u8, from_json_as_u8, "round trip through json as u8\n{msg}");
                assert_eq!(opts.no_metadata(), from_json.no_metadata(),
                           "no_metadata field through json\n{msg}");

                assert_eq!(opts.no_queue_items(), from_json.no_queue_items(),
                           "no_queue_items field through json\n{msg}");
            }

            let default_opts = GetStatusOptions::default();

            case(default_opts, 0);
            case(default_opts.with_no_metadata(true), 1);
            case(default_opts.with_no_queue_items(true), 2);
            case(default_opts.with_no_metadata(true)
                             .with_no_queue_items(true),
                 3);
        }
    }
}

pub mod receiver {
    use super::*;

    pub const CHANNEL_NAMESPACE: Namespace =
        Namespace::from_const("urn:x-cast:com.google.cast.receiver");

    pub const MESSAGE_REQUEST_TYPE_LAUNCH: MessageType = MessageType::from_const("LAUNCH");
    pub const MESSAGE_REQUEST_TYPE_STOP: MessageType = MessageType::from_const("STOP");
    pub const MESSAGE_REQUEST_TYPE_GET_STATUS: MessageType = MessageType::from_const("GET_STATUS");
    pub const MESSAGE_REQUEST_TYPE_SET_VOLUME: MessageType = MessageType::from_const("SET_VOLUME");

    pub const MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS: MessageType = MessageType::from_const("RECEIVER_STATUS");
    pub const MESSAGE_RESPONSE_TYPE_LAUNCH_ERROR: MessageType = MessageType::from_const("LAUNCH_ERROR");
    pub const MESSAGE_RESPONSE_TYPE_INVALID_REQUEST: MessageType = MessageType::from_const("INVALID_REQUEST");

    mod shared {
        use super::*;


        #[derive(Clone, Debug, Eq, PartialEq,
                 Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct AppId(Cow<'static, str>);

        /// Well known cast receiver app IDs
        impl AppId {
            pub const DEFAULT_MEDIA_RECEIVER: AppId = AppId::from_const("CC1AD845");
            pub const BACKDROP: AppId = AppId::from_const("E8C28D3C");
            pub const NETFLIX: AppId = AppId::from_const("CA5E8412");
            pub const SPOTIFY: AppId = AppId::from_const("CC32E753");
            pub const TWITCH: AppId = AppId::from_const("B3DCF968");
            pub const YOUTUBE: AppId = AppId::from_const("233637DE");
        }

        impl AppId {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub const fn from_const(s: &'static str) -> AppId {
                Self(Cow::Borrowed(s))
            }

            pub fn to_string(&self) -> String {
                self.0.to_string()
            }
        }

        impl From<&str> for AppId {
            fn from(s: &str) -> Self {
                Self(Cow::Owned(s.to_string()))
            }
        }

        impl From<String> for AppId {
            fn from(s: String) -> Self {
                Self(s.into())
            }
        }

        impl From<AppId> for String {
            fn from(id: AppId) -> String {
                id.0.into()
            }
        }

        impl Display for AppId {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                Display::fmt(&self.0, f)
            }
        }


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

        impl Status {
            pub fn applications_with_namespace(&self, ns: impl Into<Namespace>)
            -> impl Iterator<Item = &Application> {
                let ns = ns.into();
                self.applications.iter().filter(move |app| app.has_namespace(&ns))
            }
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
            pub fn has_namespace(&self, ns: impl Borrow<Namespace>) -> bool {
                let ns = ns.borrow();
                self.namespaces.iter().any(|app_ns| app_ns == ns)
            }

            pub fn to_app_session(&self, receiver_destination_id: EndpointId)
            -> Result<client::AppSession> {
                Ok(client::AppSession {
                    receiver_destination_id,
                    app_destination_id: self.transport_id.clone(),
                    app_session_id: self.app_session_id.clone(),
                })
            }
        }

        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "camelCase")]
        pub struct AppNamespace {
            pub name: Namespace,
        }

        impl PartialEq<Namespace> for AppNamespace {
            fn eq(&self, rhs: &Namespace) -> bool {
                self.name.eq(rhs)
            }
        }

        impl PartialEq<AppNamespace> for Namespace {
            fn eq(&self, rhs: &AppNamespace) -> bool {
                rhs.eq(self)
            }
        }

        #[derive(Clone, Debug,
                 Eq, Ord, PartialEq, PartialOrd,
                 Deserialize, Serialize)]
        #[serde(transparent)]
        pub struct AppSessionId(String);

        /// Describes cast device volume options.
        #[skip_serializing_none]
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_GET_STATUS;
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct GetStatusResponse(pub StatusWrapper);

    impl ResponseInner for GetStatusResponse {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[MESSAGE_RESPONSE_TYPE_RECEIVER_STATUS];
    }


    // TODO: Decide: split operations into modules?

    #[derive(Debug, Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LaunchRequest {
        pub app_id: AppId,
    }

    impl RequestInner for LaunchRequest {
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_LAUNCH;
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_STOP;
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAME: MessageType = MESSAGE_REQUEST_TYPE_SET_VOLUME;
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
        const CHANNEL_NAMESPACE: Namespace = CHANNEL_NAMESPACE;
        const TYPE_NAMES: &'static [MessageType] = &[
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
