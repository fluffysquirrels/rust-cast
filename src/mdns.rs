use anyhow::{/* bail, */ Result};
use std::net::SocketAddr;

pub const SERVICE_NAME: &'static str = "_googlecast._tcp.local";

#[derive(Clone, Debug)]
pub enum Target {
    First,
    DisplayName(String),
    Id(String),
}

pub async fn resolve_target(target: Target) -> Result<SocketAddr> {
    todo!();
}
