use anyhow::{bail, Result};
use std::net::{IpAddr, SocketAddr};

#[derive(clap::Args, Clone, Debug)]
pub struct TargetArgs {
    // TODO: mdns.

    #[arg(long = "ip", conflicts_with = "target")]
    pub target_ip: Option<IpAddr>,

    #[arg(long = "port", default_value_t = crate::async_client::DEFAULT_PORT)]
    pub target_port: u16,

    #[arg(long, conflicts_with = "target_ip")]
    pub target: Option<SocketAddr>,
}

impl TargetArgs {
    /// If a socket address was specified directly, calculate and return it.
    ///
    /// Does not resolve targets specified with mDNS.
    pub fn to_direct_socket_addr(&self) -> Result<SocketAddr> {
        let addr: SocketAddr =
            if let Some(sa) = self.target { sa }
            else if let Some(ip) = self.target_ip {
                SocketAddr::from((ip, self.target_port))
            } else {
                bail!("Exactly one of the arguments `--ip` or `--target` is required.");
            };

        Ok(addr)
    }
}
