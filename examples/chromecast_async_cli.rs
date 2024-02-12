use anyhow::bail;
use chromecast_tokio::{
    self as lib,
    async_client::{self as client, Client, /* Error, */ Result,
                   LoadMediaArgs,
                   DEFAULT_RECEIVER_ID as RECEIVER_ID},
    cast::proxies::{self, media::CustomData, receiver::AppNamespace},
    types::MediaSession,
    function_path, named,
};
use clap::Parser;
use futures::StreamExt;
use std::net::{IpAddr, SocketAddr};
use tokio::{
    io::AsyncReadExt,
    pin,
};

#[derive(clap::Parser, Clone, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,

    // TODO: mdns.

    #[arg(long = "ip", conflicts_with = "target")]
    target_ip: Option<IpAddr>,

    #[arg(long = "port", default_value_t = 8009)]
    target_port: u16,

    #[arg(long, conflicts_with = "target_ip")]
    target: Option<SocketAddr>,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Command {
    AppStop,
    Demo(DemoArgs),
    Heartbeat,
    Pause,
    Play,
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
struct SetVolumeArgs {
    /// Set volume level as a float between 0.0 and 1.0
    #[arg(long)]
    level: f32,
}

#[derive(clap::Args, Clone, Debug)]
struct StatusArgs {
    /// Pass flag to stay connected and follow status until stopped.
    #[arg(long, default_value_t = false)]
    follow: bool,
}

#[tokio::main]
// #[named]
async fn main() -> Result<()> {
    init_logging(/* json: */ false)?;

    let args = Args::parse();

    let addr: SocketAddr =
        if let Some(sa) = args.target { sa }
        else if let Some(ip) = args.target_ip {
            SocketAddr::from((ip, args.target_port))
        } else {
            bail!("Exactly one of the arguments `--ip` or `--target` is required.");
        };

    // TODO: mdns service discovery
    let config = client::Config {
        addr,
        sender: None, // Use default
    };
    let client = config.connect().await?;

    match args.command {
        Command::AppStop => app_stop_main(client).await?,
        Command::Demo(sub_args) => demo_main(client, sub_args).await?,
        Command::Heartbeat => heartbeat_main(client).await?,
        Command::Pause => media_pause_main(client).await?,
        Command::Play => media_play_main(client).await?,
        Command::SetVolume(sub_args) => set_volume_main(client, sub_args).await?,
        Command::Status(sub_args) => status_main(client, sub_args).await?,
        Command::Stop => media_stop_main(client).await?,
    };

    Ok(())
}

async fn status_main(mut client: Client, sub_args: StatusArgs) -> Result<()> {
    let receiver_status = client.receiver_status().await?;
    println!("receiver_status = {receiver_status:#?}");

    let media_ns = AppNamespace::from(lib::payload::media::CHANNEL_NAMESPACE);

    for media_app in
        receiver_status.applications.iter().filter(
            |app| app.namespaces.contains(&media_ns))
    {
        let session = media_app.to_app_session(RECEIVER_ID.to_string())?;
        client.connection_connect(session.app_destination_id.clone()).await?;
        let media_status = client.media_status(session, /* media_session_id: */ None).await?;
        println!("media_status = {media_status:#?}");
    }

    if sub_args.follow {
        let listener = client.listen_to_status();

        pin! {
            let listen_stream = futures::stream::unfold(
                listener, |mut l: client::StatusListener| async move {
                    match l.recv().await {
                        Ok(update) => Some((update, l)),

                        // Err means no more broadcast::Sender's are left to send events,
                        // i.e. the `Client` has been dropped.
                        Err(_) => None,
                    }
                });
            let cancel_stream = futures::stream::once(pause());
        };


        enum Event {
            Update(client::StatusUpdate),
            UserExit,
        }

        let mut merged = futures_concurrency::stream::Merge::merge((
            listen_stream.map(Event::Update),
            cancel_stream.map(|_| Event::UserExit),
        ));

        while let Some(event) = merged.next().await {
            match event {
                Event::Update(update) => {
                    println!(" # listener status update = {update:#?}\n # ====\n\n");
                },
                Event::UserExit => break,
            }
        }
    }

    client.close().await?;

    Ok(())
}

async fn app_stop_main(mut client: Client) -> Result<()> {
    let initial_status = client.receiver_status().await?;

    println!("initial_status = {initial_status:#?}");

    let Some(app) = initial_status.applications.first() else {
        bail!("Receiver has no apps\n\
               _ status = {initial_status:#?}");
    };

    let app_session = app.to_app_session(RECEIVER_ID.into())?;

    let stop_status = client.receiver_stop_app(app_session).await?;

    println!("stop_status = {stop_status:#?}");

    client.close().await?;
    Ok(())
}

async fn set_volume_main(mut client: Client, sub_args: SetVolumeArgs) -> Result<()> {
    let level = sub_args.level;

    if level < 0.0 || level > 1.0 {
        bail!("Bad volume level {level}; it must be between 0.0 and 1.0");
    }

    let volume = proxies::receiver::Volume {
        level: Some(level),
        muted: None,

        control_type: None,
        step_interval: None,
    };
    let status = client.receiver_set_volume(RECEIVER_ID.to_string(), volume).await?;
    println!("status = {status:#?}");

    client.close().await?;

    Ok(())
}

async fn media_pause_main(mut client: Client) -> Result<()> {
    let media_session = get_media_session(&mut client).await?;
    let media_status = client.media_pause(media_session).await?;
    println!("media_status = {media_status:#?}");
    client.close().await?;
    Ok(())
}

async fn media_play_main(mut client: Client) -> Result<()> {
    let media_session = get_media_session(&mut client).await?;
    let media_status = client.media_play(media_session).await?;
    println!("media_status = {media_status:#?}");
    client.close().await?;
    Ok(())
}

async fn media_stop_main(mut client: Client) -> Result<()> {
    let media_session = get_media_session(&mut client).await?;
    let media_status = client.media_stop(media_session).await?;
    println!("media_status = {media_status:#?}");
    client.close().await?;
    Ok(())
}

#[named]
async fn get_media_session(client: &mut Client) -> Result<MediaSession> {
    const FUNCTION_PATH: &str = function_path!();

    let receiver_status = client.receiver_status().await?;

    let media_ns = AppNamespace::from(lib::payload::media::CHANNEL_NAMESPACE);

    let Some(media_app)
        = receiver_status.applications.iter()
            .find(|app| app.namespaces.contains(&media_ns)) else
    {
        bail!("{FUNCTION_PATH}: No media app found.\n\
               _ receiver_status = {receiver_status:#?}");
    };

    let app_session = media_app.to_app_session(RECEIVER_ID.to_string())?;
    client.connection_connect(app_session.app_destination_id.clone()).await?;

    let media_status = client.media_status(app_session.clone(),
                                           /* media_session_id: */ None).await?;

    let Some(media_session_id) =
        media_status.status.first()
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

async fn demo_main(mut client: Client, sub_args: DemoArgs) -> Result<()> {
    let status = client.receiver_status().await?;
    println!("status = {status:#?}");

    let (app_session, launch_status) =
        client.media_launch_default(RECEIVER_ID.into()).await?;
    println!(" # launched:\n\
              _  app_session = {app_session:#?}\n\
              _  status = {launch_status:#?}\n\n");

    let media_url =
        "https://commondatastorage.googleapis.com/gtv-videos-bucket/sample/BigBuckBunny.mp4";
    let media = LoadMediaArgs {
        media: proxies::media::Media {
            content_id: media_url.to_string(),
            stream_type: "NONE".to_string(), // one of "NONE", "BUFFERED", "LIVE"
            content_type: "".to_string(),
            metadata: None,
            duration: None, // : Option<f32>
            content_url: None, // : Option<String>
            custom_data: CustomData::default(),
        },

        current_time: 0_f64,
        autoplay: true,
        preload_time: None, // Use default.
    };

    let media_load_res = client.media_load(app_session.clone(), media).await?;
    println!("media_load_res = {media_load_res:#?}");

    sleep(std::time::Duration::from_secs(2)).await;

    let receiver_status = client.receiver_status().await?;
    println!("receiver_status = {receiver_status:#?}");

    let media_status_res = client.media_status(app_session.clone(),
                                               /* media_session_id: */ None).await?;
    println!("media_status_res = {media_status_res:#?}");

    sleep(std::time::Duration::from_secs(2)).await;

    if !sub_args.no_stop {
        let stop_res = client.receiver_stop_app(app_session.clone()).await?;
        println!("stop_res = {stop_res:#?}");

        sleep(std::time::Duration::from_secs(1)).await;

        let receiver_status_2 = client.receiver_status().await?;
        println!("receiver_status_2 = {receiver_status_2:#?}");
    }

    client.close().await?;
    Ok(())
}

async fn heartbeat_main(mut client: Client) -> Result<()>
{
    client.connection_connect(RECEIVER_ID.to_string()).await?;

    pause().await?;

    client.close().await?;
    Ok(())
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
