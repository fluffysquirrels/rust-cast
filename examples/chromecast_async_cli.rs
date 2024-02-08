use chromecast_tokio::{
//     self as lib,
    async_client::{self as client, /* Error, */ Result,
                   LoadMediaArgs},
    cast::proxies,
//     function_path, named,

};
use std::net::SocketAddr;

#[tokio::main]
// #[named]
async fn main() -> Result<()> {
    init_logging(/* json: */ false)?;

    // TODO: mdns service discovery

    let config = client::Config {
        addr: SocketAddr::from(([192, 168, 0, 144], 8009)),
        sender: None, // Use default
    };
    let mut client = config.connect().await?;

    let status = client.receiver_status().await?;
    println!("status = {status:#?}");

    let (receiver_session, launch_status) =
        client.media_launch_default(client::DEFAULT_RECEIVER_ID.into()).await?;
    println!(" # launched:\n\
              _  receiver_session = {receiver_session:#?}\n\
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
        },

        current_time: 0_f64,
        autoplay: true,
        preload_time: None, // Use default.
    };

    let media_load_res = client.media_load(receiver_session.clone(), media).await?;
    println!("media_load_res = {media_load_res:#?}");

    sleep(std::time::Duration::from_secs(1)).await;

    let stop_res = client.receiver_stop_app(receiver_session.clone()).await?;
    println!("stop_res = {stop_res:#?}");

    sleep(std::time::Duration::from_secs(1)).await;

    let status_2 = client.receiver_status().await?;
    println!("status_2 = {status_2:#?}");

    client.close().await?;
    Ok(())
}

async fn sleep(dur: std::time::Duration) {
    println!("sleeping for {dur:?}");
    tokio::time::sleep(dur).await;
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
