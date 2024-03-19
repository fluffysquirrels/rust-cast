use anyhow::{bail, Result};
use crate::mdns;
use std::net::{IpAddr, SocketAddr};

#[derive(clap::Args, Clone, Debug)]
#[group(id = "target_args", multiple = false, required = true)]
pub struct TargetArgs {
    #[arg(long)]
    pub mdns_first: bool,

    #[arg(long)]
    pub mdns_display_name: Option<String>,

    #[arg(long)]
    pub mdns_id: Option<String>,

    /// Chromecast target IP address (IPv4 or IPv6).
    ///
    /// Uses default TCP port 8009. Use `--addr` to override the TCP port.
    #[arg(long, value_name = "TARGET_IP")]
    pub ip: Option<IpAddr>,

    /// Chromecast target IP address (IPv4 or IPv6) and TCP port.
    #[arg(long, value_name = "TARGET_IP:PORT")]
    pub addr: Option<SocketAddr>,
}

#[derive(Clone, Debug)]
pub enum Target {
    Mdns(mdns::Target),
    SocketAddr(SocketAddr),
}

impl TargetArgs {
    /// Convert to `Target` enum from specified clap args.
    ///
    /// Assumes that only one target specification is given.
    pub fn to_target(&self) -> Result<Target> {
        Ok(
            if let Some(sa) = self.addr {
                Target::SocketAddr(sa)
            }
            else if let Some(ip) = self.ip {
                Target::SocketAddr(SocketAddr::from((ip, crate::async_client::DEFAULT_PORT)))
            } else if self.mdns_first {
                Target::Mdns(mdns::Target::First)
            } else if let Some(ref name) = self.mdns_display_name {
                Target::Mdns(mdns::Target::DisplayName(name.clone()))
            } else if let Some(ref id) = self.mdns_id {
                Target::Mdns(mdns::Target::Id(id.clone()))
            } else {
                // clap should have already bailed (with a better help message)
                // if parsing arguments from process arguments.
                //
                // This should only occur when `TargetArgs` is constructed programatically.
                bail!("TargetArgs::to_target: no target specified\n\
                       self = {self:#?}");
            }
        )
    }

    pub async fn resolve_to_socket_addr(&self) -> Result<SocketAddr> {
        let target = self.to_target()?;

        Ok(match target {
            Target::SocketAddr(sa) => sa,
            Target::Mdns(mt) => mdns::resolve_target(&mt).await?.addr,
        })
    }
}
