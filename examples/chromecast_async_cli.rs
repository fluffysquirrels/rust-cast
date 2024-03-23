use anyhow::bail;
use clap::Parser;
use futures::StreamExt;
use rust_cast::{
    self as lib,
    async_client::{self as client, Client, /* Error, */ Result,
                   DEFAULT_RECEIVER_ID as RECEIVER_ID},
    payload,
    types::{MediaSession, NamespaceConst},
    function_path, named,
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
    MediaLoad(MediaLoadArgs),
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
struct MediaLoadArgs {
    #[arg(long)]
    url: String,
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
struct SeekArgs {
    #[arg(long)]
    time: Option<f64>,

    #[arg(long, value_enum)]
    resume_state: Option<payload::media::ResumeState>,
}

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
        Command::MediaLoad(sub_args) => media_load_main(&mut client, sub_args).await?,
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
    let receiver_status = client.receiver_status().await?;

    print_receiver_status(&receiver_status);

    for media_app in
        receiver_status.applications.iter().filter(
            |app| app.has_namespace(MEDIA_NS))
    {
        let session = media_app.to_app_session(RECEIVER_ID.to_string())?;
        client.connection_connect(session.app_destination_id.clone()).await?;
        let media_status = client.media_status(session, /* media_session_id: */ None).await?;
        print_media_status(&media_status);
    }

    Ok(())
}

async fn app_stop_main(client: &mut Client) -> Result<()> {
    let initial_status = client.receiver_status().await?;

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

async fn media_pause_main(client: &mut Client) -> Result<()> {
    let media_session = get_media_session(client).await?;
    let media_status = client.media_pause(media_session).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn media_play_main(client: &mut Client) -> Result<()> {
    let media_session = get_media_session(client).await?;
    let media_status = client.media_play(media_session).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn media_stop_main(client: &mut Client) -> Result<()> {
    let media_session = get_media_session(client).await?;
    let media_status = client.media_stop(media_session).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn media_seek_main(client: &mut Client, sub_args: SeekArgs) -> Result<()> {
    let media_session = get_media_session(client).await?;
    let media_status = client.media_seek(media_session,
                                         sub_args.time,
                                         sub_args.resume_state,
                                         payload::media::CustomData::default()
                       ).await?;
    print_media_status(&media_status);
    Ok(())
}

async fn media_load_main(client: &mut Client, sub_args: MediaLoadArgs) -> Result<()> {
    status_single(client).await?;

    let load_args = payload::media::LoadRequestArgs::from_url(
        &sub_args.url);

    let _media_session = media_load(client, load_args).await?;

    Ok(())
}

async fn demo_main(client: &mut Client, sub_args: DemoArgs) -> Result<()> {
    status_single(client).await?;

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

#[named]
async fn get_media_session(client: &mut Client) -> Result<MediaSession> {
    const FUNCTION_PATH: &str = function_path!();

    let receiver_status = client.receiver_status().await?;

    let Some(media_app)
        = receiver_status.applications.iter()
            .find(|app| app.has_namespace(MEDIA_NS)) else
    {
        bail!("{FUNCTION_PATH}: No media app found.\n\
               _ receiver_status = {receiver_status:#?}");
    };

    let app_session = media_app.to_app_session(RECEIVER_ID.to_string())?;
    client.connection_connect(app_session.app_destination_id.clone()).await?;

    let media_status = client.media_status(app_session.clone(),
                                           /* media_session_id: */ None).await?;

    let Some(media_session_id) =
        media_status.entries.first()
            .map(|s| s.media_session_id) else
    {
        bail!("{FUNCTION_PATH}: No media status entry\n\
               _ receiver_status = {media_status:#?}");
    };

    Ok(MediaSession {
        app_session,
        media_session_id,
    })
}

#[named]
async fn media_load(client: &mut Client, load_args: payload::media::LoadRequestArgs)
 -> Result<MediaSession>
{
    const FUNCTION_PATH: &str = function_path!();

    status_single(client).await?;

    let (app_session, launch_status) =
        client.media_launch_default(RECEIVER_ID.into()).await?;
    println!(" # launched:\n\
              _  app_session = {app_session:#?}\n\
              _  status = {launch_status_small:#?}\n\n",
             launch_status_small =
                 payload::receiver::small_debug::ReceiverStatus(&launch_status));

    let load_res = client.media_load(app_session.clone(), load_args).await?;
    println!("media_load_res = {load_res:#?}",
             load_res = payload::media::small_debug::MediaStatus(&load_res));

    let Some(media_session_id) =
        load_res.entries.first()
            .map(|s| s.media_session_id) else
    {
        bail!("{FUNCTION_PATH}: No media status entry\n\
               _ media_status = {load_res:#?}");
    };

    Ok(MediaSession {
        app_session,
        media_session_id,
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
