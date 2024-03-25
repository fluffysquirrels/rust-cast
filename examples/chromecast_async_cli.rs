use anyhow::bail;
use clap::Parser;
use futures::StreamExt;
use rust_cast::{
    self as lib,
    async_client::{self as client, Client, Error, Result,
                   DEFAULT_RECEIVER_ID as RECEIVER_ID},
    payload::{self, media::{CustomData, ItemId}},
    types::{MediaSession, NamespaceConst},
    /* function_path, named, */
};
use tokio::{
    io::AsyncReadExt,
    pin,
};
use tokio_util::either::Either;

#[derive(clap::Parser, Clone, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,

    #[clap(flatten)]
    target: lib::args::TargetArgs,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Command {
    AppStop,
    Demo(DemoArgs),
    Heartbeat,
    MediaEditTracksInfo(MediaEditTracksInfoArgs),
    MediaLaunch,
    MediaLoad(MediaLoadArgs),
    MediaQueueGetItemIds,
    MediaQueueGetItems(MediaQueueGetItemsArgs),
    MediaQueueInsert(MediaQueueInsertArgs),
    MediaQueueJump(MediaQueueJumpArgs),
    MediaQueueLoad(MediaQueueLoadArgs),
    MediaQueueRemove(MediaQueueRemoveArgs),
    MediaQueueReorder(MediaQueueReorderArgs),
    // TODO: MediaQueueUpdate(MediaQueueUpdateArgs),
    Pause,
    Play,
    Seek(SeekArgs),
    SetVolume(SetVolumeArgs),
    Status(StatusArgs),
    Stop,
}

#[derive(clap::Args, Clone, Debug)]
struct DemoArgs {
    #[arg(long)]
    no_stop: bool,
}

#[derive(clap::Args, Clone, Debug)]
struct MediaEditTracksInfoArgs {
    #[arg(long, visible_alias = "tracks",
          action(clap::ArgAction::Set),
          allow_negative_numbers = true,
          require_equals = true,
          default_missing_values([""; 0]),
          num_args(0..),
          value_delimiter = ',')]
    active_track_ids: Option<Vec<payload::media::TrackId>>,

    #[arg(long, visible_alias = "text-style",
          default_value_t = false)]
    set_text_track_style: bool,

    #[clap(flatten)]
    text_track_style: TextTrackStyleArgs,
}

#[derive(clap::Args, Clone, Debug)]
struct MediaLoadArgs {
    #[arg(long)]
    url: String,
}

#[derive(clap::Args, Clone, Debug)]
struct MediaQueueGetItemsArgs {
    #[arg(long,
          action(clap::ArgAction::Set),
          allow_negative_numbers = true,
          require_equals = true,
          value_delimiter = ',')]
    items: Vec<ItemId>,
}

#[derive(clap::Args, Clone, Debug)]
struct MediaQueueInsertArgs {
    #[arg(long, value_name = "ITEM_ID")]
    before: Option<ItemId>,

    #[clap(flatten)]
    items: QueueItemsArgs,
}

#[derive(clap::Args, Clone, Debug)]
#[group(required = true, multiple = false)]
struct MediaQueueJumpArgs {
    #[arg(long)]
    item: Option<ItemId>,

    #[arg(long, default_value_t = false)]
    next: bool,

    #[arg(long)]
    offset: Option<i32>,

    #[arg(long, default_value_t = false)]
    prev: bool,
}

#[derive(clap::Args, Clone, Debug)]
struct MediaQueueLoadArgs {
    #[clap(flatten)]
    items: QueueItemsArgs,
}

#[derive(clap::Args, Clone, Debug)]
struct QueueItemsArgs {
    #[arg(long = "url")]
    urls: Vec<String>,

    /// If present, parsed as Vec<payload::media::QueueItem>
    #[arg(long)]
    items_json: Option<String>,
}

#[derive(clap::Args, Clone, Debug)]
struct MediaQueueRemoveArgs {
    #[arg(long,
          required = true, num_args(1..),
          action(clap::ArgAction::Set),
          allow_negative_numbers = true,
          require_equals = true,
          value_delimiter = ',')]
    items: Vec<ItemId>,
}

#[derive(clap::Args, Clone, Debug)]
struct MediaQueueReorderArgs {
    #[arg(long, value_name = "ITEM_ID")]
    before: Option<ItemId>,

    #[arg(long,
          required = true, num_args(1..),
          action(clap::ArgAction::Set),
          allow_negative_numbers = true,
          require_equals = true,
          value_delimiter = ',')]
    items: Vec<ItemId>,
}

#[derive(clap::Args, Clone, Debug)]
struct SetVolumeArgs {
    /// Set volume level as a float between 0.0 and 1.0
    #[arg(long)]
    level: Option<f32>,

    #[arg(long,
          action = clap::ArgAction::Set, default_missing_value = "true",
          require_equals = true, num_args = 0..=1)]
    mute: Option<bool>,
}

#[derive(clap::Args, Clone, Debug)]
struct SeekArgs {
    #[arg(long)]
    time: Option<f64>,

    // TODO: offset: Option<f64>,

    #[arg(long, value_enum)]
    resume_state: Option<payload::media::ResumeState>,
}

#[derive(clap::Args, Clone, Debug)]
struct StatusArgs {
    /// Pass flag to stay connected and follow status until stopped.
    #[arg(long, default_value_t = false)]
    follow: bool,

    /// Pass flag to poll cast device's status.
    ///
    /// Requires `--follow`.
    #[arg(long, default_value_t = false, requires = "follow")]
    poll: bool,
}

#[derive(clap::Args, Clone, Debug)]
struct TextTrackStyleArgs {
    #[arg(long, value_enum, default_value_t = TextTrackStylePreset::Basic)]
    preset: TextTrackStylePreset,

    #[arg(long)]
    background_color: Option<ColorArg>,

    #[arg(long)]
    edge_color: Option<ColorArg>,

    // TODO: edge_type: Option<TextTrackEdgeType>,
    #[arg(long)]
    edge_type: Option<String>,

    #[arg(long)]
    font_family: Option<String>,

    // TODO: font_generic_family: Option<FontGenericFamily>,
    #[arg(long)]
    font_generic_family: Option<String>,

    /// Default scaling is 1.0.
    #[arg(long)]
    font_scale: Option<f64>,

    // TODO: font_style: Option<FontStyle>,
    #[arg(long)]
    font_style: Option<String>,

    #[arg(long)]
    foreground_color: Option<ColorArg>,

    #[arg(long)]
    window_color: Option<ColorArg>,

    /// Rounded corner radius absolute value in pixels (px).
    /// This value will be ignored if window_type is not RoundedCorners.
    #[arg(long)]
    window_rounded_corner_radius: Option<f64>,

    // TODO: window_type: Option<TextTrackWindowType>,
    #[arg(long)]
    window_type: Option<String>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, Eq, PartialEq)]
enum TextTrackStylePreset {
    Basic,
    Empty,
}

// TODO: Probably validate, use a strongly typed value.
type ColorArg = String;

const MEDIA_NS: NamespaceConst = payload::media::CHANNEL_NAMESPACE;


#[tokio::main]
// #[named]
async fn main() -> Result<()> {
    init_logging(/* json: */ false)?;

    let args = Args::parse();

    tracing::debug!(?args, "args");

    let addr = args.target.resolve_to_socket_addr().await?;

    let config = client::Config {
        addr,
        sender: None, // Use default
    };
    let mut client = config.connect().await?;

    match args.command {
        Command::AppStop => app_stop_main(&mut client).await?,
        Command::Demo(sub_args) => demo_main(&mut client, sub_args).await?,
        Command::Heartbeat => heartbeat_main(&mut client).await?,
        Command::MediaEditTracksInfo(sub_args) => media_edit_tracks_info_main(
                                                      &mut client, sub_args).await?,
        Command::MediaLaunch => media_launch_main(&mut client).await?,
        Command::MediaLoad(sub_args) => media_load_main(&mut client, sub_args).await?,
        Command::MediaQueueGetItemIds => media_queue_get_item_ids_main(&mut client).await?,
        Command::MediaQueueGetItems(sub_args) => media_queue_get_items_main(
                                                     &mut client, sub_args).await?,
        Command::MediaQueueInsert(sub_args) => media_queue_insert_main(
                                                   &mut client, sub_args).await?,
        Command::MediaQueueJump(sub_args) => media_queue_jump_main(&mut client, sub_args).await?,
        Command::MediaQueueLoad(sub_args) => media_queue_load_main(&mut client, sub_args).await?,
        Command::MediaQueueRemove(sub_args) => media_queue_remove_main(
                                                   &mut client, sub_args).await?,
        Command::MediaQueueReorder(sub_args) => media_queue_reorder_main(
                                                    &mut client, sub_args).await?,
        Command::Pause => media_pause_main(&mut client).await?,
        Command::Play => media_play_main(&mut client).await?,
        Command::Seek(sub_args) => media_seek_main(&mut client, sub_args).await?,
        Command::SetVolume(sub_args) => set_volume_main(&mut client, sub_args).await?,
        Command::Status(sub_args) => status_main(&mut client, sub_args).await?,
        Command::Stop => media_stop_main(&mut client).await?,
    };

    client.close().await?;

    Ok(())
}

async fn status_main(client: &mut Client, sub_args: StatusArgs) -> Result<()> {
    status_single(client).await?;

    if sub_args.follow {
        enum Event {
            PollStatus,
            Update(client::StatusUpdate),
            UserExit,
        }

        let listener = client.listen_status_2();

        pin! {
            let cancel_stream = futures::stream::once(pause());
        };

        let poll_interval = if sub_args.poll {
            Either::Left(tokio_stream::wrappers::IntervalStream::new(
                tokio::time::interval(std::time::Duration::from_secs(2))))
        } else {
            Either::Right(futures::stream::pending())
        };

        let mut merged = futures_concurrency::stream::Merge::merge((
            poll_interval.map(|_| Event::PollStatus),
            listener.map(Event::Update),
            cancel_stream.map(|_| Event::UserExit),
        ));

        while let Some(event) = merged.next().await {
            match event {
                Event::PollStatus => {
                    status_single(client).await?;
                },
                Event::Update(update) => {
                    // println!(" # listener status update = {update:#?}\n\n");
                    println!(" # listener status update (small) = {update_small:#?}\n\
                              ====\n\n",
                             update_small = client::small_debug::StatusUpdate(&update));

                    // TODO: if receiver app changes and has media namespace, get status.
                    // TODO: Support this optionally in Client?
                    if let client::StatusMessage::Receiver(status) = update.msg {
                        for media_app in
                               status.applications
                                   .iter()
                                   .filter(|app| app.has_namespace(MEDIA_NS))
                        {
                            client.connection_connect(media_app.transport_id.clone()).await?;
                        }
                    }
                },
                Event::UserExit => break,
            }
        }
    }

    Ok(())
}

async fn status_single(client: &mut Client) -> Result<()> {
    let receiver_status = client.receiver_status(RECEIVER_ID.to_string()).await?;

    print_receiver_status(&receiver_status);

    for media_app in
        receiver_status.applications.iter().filter(
            |app| app.has_namespace(MEDIA_NS))
    {
        let app_session = media_app.to_app_session(RECEIVER_ID.to_string())?;
        client.connection_connect(app_session.app_destination_id.clone()).await?;
        let media_status = client.media_status(app_session.clone(),
                                               /* media_session_id: */ None).await?;
        print_media_status(&media_status);

        if let Some(media_session_id) = media_status.try_find_media_session_id().ok() {
            let media_session = MediaSession {
                app_session: app_session.clone(),
                media_session_id,
            };
            let item_ids = client.media_queue_get_item_ids(media_session).await?;
            println!("Queue item IDs for media_session_id {media_session_id}: {item_ids:#?}");

            let media_status_entry = media_status.entries.first().unwrap();
            println!("current_item_id = {:?}",
                     media_status_entry.current_item_id);
        }
    }

    Ok(())
}

async fn app_stop_main(client: &mut Client) -> Result<()> {
    let initial_status = client.receiver_status(RECEIVER_ID.to_string()).await?;

    println!("initial_status = {initial_status:#?}");

    let Some(app) = initial_status.applications.first() else {
        bail!("Receiver has no apps\n\
               _ status = {initial_status:#?}");
    };

    let app_session = app.to_app_session(RECEIVER_ID.into())?;

    let stop_status = client.receiver_stop_app(app_session).await?;

    println!("stop_status = {stop_status:#?}");

    Ok(())
}

async fn set_volume_main(client: &mut Client, sub_args: SetVolumeArgs) -> Result<()> {
    let level: Option<f32> = sub_args.level;

    match level {
        Some(level) if level < 0.0 || level > 1.0 =>
            bail!("Bad volume level {level}; it must be between 0.0 and 1.0 inclusive"),
        _ => (),
    };

    let mute: Option<bool> = sub_args.mute;

    let num_args = (level.is_some() as u8) + (mute.is_some() as u8);
    if num_args != 1 {
        bail!("Exactly one of the arguments `--level` or `--mute` must be given.\n\
               The Chromecast does not set `mute` when both `level` and `mute` are supplied.\n\
               {num_args} arguments were supplied.");
    }

    let volume = payload::receiver::Volume {
        level,
        muted: mute,

        control_type: None,
        step_interval: None,
    };
    let status = client.receiver_set_volume(RECEIVER_ID.to_string(), volume).await?;
    println!("status = {status:#?}");

    Ok(())
}

async fn media_edit_tracks_info_main(client: &mut Client, sub_args: MediaEditTracksInfoArgs)
-> Result<()> {
    let args = payload::media::EditTracksInfoRequestArgs {
        active_track_ids: sub_args.active_track_ids,
        text_track_style: if sub_args.set_text_track_style {
                              Some(sub_args.text_track_style.try_into()?)
                          } else {
                              None
                          },
    };

    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let media_status = client.media_edit_tracks_info(
                           media_session,
                           args
                       ).await?;

    print_media_status(&media_status);

    status_single(client).await?;

    Ok(())
}

async fn media_launch_main(client: &mut Client) -> Result<()> {
    let _app_session = client.media_get_or_launch_default_app_session(
        RECEIVER_ID.to_string()).await?;

    Ok(())
}

async fn media_load_main(client: &mut Client, sub_args: MediaLoadArgs) -> Result<()> {
    let load_args = payload::media::LoadRequestArgs::from_url(
        &sub_args.url);

    let _media_session = media_load(client, load_args).await?;

    Ok(())
}

async fn media_pause_main(client: &mut Client) -> Result<()> {
    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let media_status = client.media_pause(media_session).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn media_play_main(client: &mut Client) -> Result<()> {
    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let media_status = client.media_play(media_session).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn media_queue_get_items_main(client: &mut Client, sub_args: MediaQueueGetItemsArgs)
-> Result<()>
{
    let args = payload::media::QueueGetItemsRequestArgs {
        custom_data: CustomData::default(),
        item_ids: sub_args.items,
    };

    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let items = client.media_queue_get_items(media_session, args).await?;
    println!("Queue items: {items:#?}");
    Ok(())
}

async fn media_queue_get_item_ids_main(client: &mut Client) -> Result<()>
{
    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let item_ids = client.media_queue_get_item_ids(media_session).await?;
    println!("Queue item IDs: {item_ids:#?}");
    Ok(())
}

async fn media_queue_jump_main(client: &mut Client, sub_args: MediaQueueJumpArgs) -> Result<()> {
    use payload::media::QueueUpdateRequestArgs;

    let args: QueueUpdateRequestArgs =
        if let Some(item_id) = sub_args.item {
            QueueUpdateRequestArgs::jump_item(item_id)
        } else if let Some(offset) = sub_args.offset {
            QueueUpdateRequestArgs::jump_offset(offset)
        } else if sub_args.next {
            QueueUpdateRequestArgs::jump_next()
        } else if sub_args.prev {
            QueueUpdateRequestArgs::jump_prev()
        } else {
            bail!("MediaQueueJumpArgs: no options set\n\
                   _ sub_args = {sub_args:#?}");
        };

    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let media_status = client.media_queue_update(media_session, args).await?;
    print_media_status(&media_status);

    status_single(client).await?;

    Ok(())
}

async fn media_queue_insert_main(client: &mut Client, sub_args: MediaQueueInsertArgs)
-> Result<()>
{
    let args = payload::media::QueueInsertRequestArgs {
        custom_data: CustomData::default(),
        insert_before: sub_args.before,
        items: sub_args.items.to_items()?,
    };

    let media_session = client.media_get_default_media_session(
                            RECEIVER_ID.into()).await?;
    let media_status = client.media_queue_insert(media_session, args).await?;
    print_media_status(&media_status);

    status_single(client).await?;

    Ok(())
}

async fn media_queue_load_main(client: &mut Client, sub_args: MediaQueueLoadArgs) -> Result<()> {
    let args = payload::media::QueueLoadRequestArgs {
        custom_data: CustomData::default(),
        items: sub_args.items.to_items()?,

        // TODO: From args
        repeat_mode: payload::media::RepeatMode::default(),

        // TODO: From args
        start_index: 0,
    };

    let app_session = client.media_get_or_launch_default_app_session(RECEIVER_ID.into()).await?;
    let media_status = client.media_queue_load(app_session, args).await?;
    print_media_status(&media_status);

    status_single(client).await?;

    Ok(())
}

async fn media_queue_remove_main(client: &mut Client, sub_args: MediaQueueRemoveArgs)
-> Result<()>
{
    let args = payload::media::QueueRemoveRequestArgs {
        custom_data: CustomData::default(),

        // TODO: Take as arg
        current_item_id: None,

        // TODO: Take as arg
        current_time: None,

        item_ids: sub_args.items,
    };

    let media_session = client.media_get_default_media_session(
                            RECEIVER_ID.into()).await?;
    let media_status = client.media_queue_remove(media_session, args).await?;
    print_media_status(&media_status);

    status_single(client).await?;

    Ok(())
}

async fn media_queue_reorder_main(client: &mut Client, sub_args: MediaQueueReorderArgs)
-> Result<()>
{
    let args = payload::media::QueueReorderRequestArgs {
        custom_data: CustomData::default(),

        // TODO: Take as arg
        current_item_id: None,

        // TODO: Take as arg
        current_time: None,

        insert_before: sub_args.before,
        item_ids: sub_args.items,
    };

    let media_session = client.media_get_default_media_session(
                            RECEIVER_ID.into()).await?;
    let media_status = client.media_queue_reorder(media_session, args).await?;
    print_media_status(&media_status);

    status_single(client).await?;

    Ok(())
}

async fn media_stop_main(client: &mut Client) -> Result<()> {
    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let media_status = client.media_stop(media_session).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn media_seek_main(client: &mut Client, sub_args: SeekArgs) -> Result<()> {
    let media_session = client.media_get_media_session(RECEIVER_ID.into()).await?;
    let media_status = client.media_seek(media_session,
                                         sub_args.time,
                                         sub_args.resume_state,
                                         CustomData::default()
                       ).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn demo_main(client: &mut Client, sub_args: DemoArgs) -> Result<()> {
    let load_args = payload::media::LoadRequestArgs::from_url(
        "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/BigBuckBunny.mp4");

    let media_session = media_load(client, load_args).await?;

    sleep(std::time::Duration::from_secs(2)).await;

    status_single(client).await?;

    sleep(std::time::Duration::from_secs(2)).await;

    if !sub_args.no_stop {
        let stop_res = client.receiver_stop_app(media_session.app_session.clone()).await?;
        println!("stop_res = {stop_res:#?}",
                 stop_res = payload::receiver::small_debug::ReceiverStatus(&stop_res));

        sleep(std::time::Duration::from_secs(1)).await;

        status_single(client).await?;

        sleep(std::time::Duration::from_secs(2)).await;

        status_single(client).await?;
    }

    Ok(())
}

async fn heartbeat_main(client: &mut Client) -> Result<()>
{
    client.connection_connect(RECEIVER_ID.to_string()).await?;

    pause().await?;

    Ok(())
}

async fn media_load(client: &mut Client, load_args: payload::media::LoadRequestArgs)
 -> Result<MediaSession>
{
    let app_session = client.media_get_or_launch_default_app_session(
        RECEIVER_ID.to_string()).await?;

    let load_status = client.media_load(app_session.clone(), load_args).await?;
    println!("media_load_res = {load_status:#?}",
             load_status = payload::media::small_debug::MediaStatus(&load_status));

    Ok(MediaSession {
        app_session,
        media_session_id: load_status.try_find_media_session_id()?,
    })
}

fn print_receiver_status(receiver_status: &payload::receiver::Status) {
    if tracing::event_enabled!(tracing::Level::TRACE) {
        println!("receiver_status (full) = {receiver_status:#?}");
    }

    let receiver_status_small = payload::receiver::small_debug::ReceiverStatus(&receiver_status);
    println!("receiver_status = {receiver_status_small:#?}");
}

fn print_media_status(media_status: &payload::media::Status) {
    if tracing::event_enabled!(tracing::Level::TRACE) {
        println!("media_status (full) = {media_status:#?}");
    }

    let media_status_small = payload::media::small_debug::MediaStatus(media_status);
    println!("media_status = {media_status_small:#?}");
}

async fn sleep(dur: std::time::Duration) {
    println!("sleeping for {dur:?}");
    tokio::time::sleep(dur).await;
}

async fn pause() -> Result<()> {
    println!("Press Enter to exit . . .");
    let mut stdin = tokio::io::stdin();
    let _read = stdin.read_u8().await?;
    Ok(())
}

impl TryInto<payload::media::TextTrackStyle> for TextTrackStyleArgs {
    type Error = Error;

    fn try_into(self) -> Result<payload::media::TextTrackStyle> {
        let preset = match self.preset {
            TextTrackStylePreset::Basic => payload::media::TextTrackStyle::basic(),
            TextTrackStylePreset::Empty => payload::media::TextTrackStyle::empty(),
        };

        Ok(payload::media::TextTrackStyle {
            background_color: self.background_color.or(preset.background_color),
            custom_data: CustomData::default(),
            edge_color: self.edge_color.or(preset.edge_color),
            edge_type: self.edge_type.or(preset.edge_type),
            font_family: self.font_family.or(preset.font_family),
            font_generic_family: self.font_generic_family.or(preset.font_generic_family),
            font_scale: self.font_scale.or(preset.font_scale),
            font_style: self.font_style.or(preset.font_style),
            foreground_color: self.foreground_color.or(preset.foreground_color),
            window_color: self.window_color.or(preset.window_color),
            window_rounded_corner_radius: self.window_rounded_corner_radius
                                    .or(preset.window_rounded_corner_radius),
            window_type: self.window_type.or(preset.window_type),
        })
    }
}

impl QueueItemsArgs {
    fn to_items(&self) -> Result<Vec<payload::media::QueueItem>>
    {
        Ok(if let Some(ref items_json) = self.items_json {
            json5::from_str(items_json)?
        } else {
            self.urls.iter()
                .map(|url| payload::media::QueueItem::from_url(url))
                .collect()
        })
    }
}

#[derive(Eq, PartialEq)]
enum LogMode {
    PrettyAnsi,
    Pretty,
    Json,
}

pub fn init_logging(log_json: bool) -> Result<()> {
    use std::io::IsTerminal;
    use tracing_bunyan_formatter::{
        BunyanFormattingLayer,
        JsonStorageLayer,
    };
    use tracing_subscriber::{
        EnvFilter,
        filter::LevelFilter,
        fmt,
        prelude::*,
    };

    let log_mode =
        if log_json {
            LogMode::Json
        } else {
            if std::io::stderr().is_terminal() {
                LogMode::PrettyAnsi
            } else {
                LogMode::Pretty
            }
        };

    tracing_subscriber::Registry::default()
        .with(match log_mode {
                  LogMode::PrettyAnsi | LogMode::Pretty => {
                      Some(fmt::Layer::new()
                               .event_format(fmt::format()
                                                 .pretty()
                                                 .with_ansi(log_mode == LogMode::PrettyAnsi)
                                                 .with_timer(fmt::time::UtcTime::<_>::
                                                                 rfc_3339())
                                                 .with_target(true)
                                                 .with_source_location(true)
                                                 .with_thread_ids(true))
                               .with_ansi(log_mode == LogMode::PrettyAnsi)
                               .with_writer(std::io::stderr)
                               .with_span_events(fmt::format::FmtSpan::NEW
                                                 | fmt::format::FmtSpan::CLOSE))
                  },
                  _ => None,
             })
        .with(if log_mode == LogMode::Json {
                  Some(JsonStorageLayer
                           .and_then(BunyanFormattingLayer::new(
                               env!("CARGO_CRATE_NAME").to_string(),
                               std::io::stdout)))
              } else {
                  None
              })
        // Global filter
        .with(EnvFilter::builder()
                  .with_default_directive(LevelFilter::INFO.into())
                  .parse(std::env::var("RUST_LOG")
                             .unwrap_or(format!("warn,{crate_}=info",
                                                crate_ = env!("CARGO_CRATE_NAME"))))?)
        .try_init()?;

    Ok(())
}
