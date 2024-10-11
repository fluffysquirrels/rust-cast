use anyhow::{/* bail, */ Error, format_err, Result};
use crate::util::named;
use futures::TryStreamExt;
use ::mdns::{self as mdns_lib, RecordKind};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use tokio::pin;

#[derive(Clone, Debug)]
pub struct Client {
    pub addr: SocketAddr,
    pub service_host: String,
    pub display_name: String,
    pub app_name: Option<String>,
    pub uuid: Option<String>,
}

pub const SERVICE_NAME: &'static str = "_googlecast._tcp.local";

#[derive(Clone, Debug)]
pub enum Target {
    First,
    DisplayName(String),
    Id(String),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum RecordKindVariant {
    A,
    PTR,
    SRV,
    TXT,
}

/// TODO: Make this configurable.
const DISCOVER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// TODO: Make this configurable.
const DISCOVER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tracing::instrument(level = "debug", ret, err)]
pub async fn resolve_target(target: &Target) -> Result<Client> {
    let deadline = tokio::time::Instant::now() + DISCOVER_TIMEOUT;

    let discover_stream = mdns_lib::discover::all(SERVICE_NAME, DISCOVER_INTERVAL)?
        .initial_send(true)
        .listen()
        .err_into::<Error>()
        .try_filter_map(|resp: mdns_lib::Response| {
            let client: Result<Option<Client>> = (move || {
                let clients: Vec<Client> = clients_from_mdns_response(&resp)?;

                for client in clients.into_iter() {
                    let is_match = match target {
                        Target::First => true,
                        Target::DisplayName(ref dn) if (&client.display_name == dn) => true,
                        Target::Id(ref id) if (client.uuid.as_ref() == Some(id)) => true,
                        _ => false,
                    };

                    if is_match {
                        return Ok(Some(client));
                    }
                }

                Ok(None)
            })();

            futures::future::ready(client)
        });

    pin!(discover_stream);

    let first_match_fut = discover_stream.try_next();

    let timeout = tokio::time::timeout_at(deadline, first_match_fut);

    Ok(timeout.await??
       .ok_or_else(|| format_err!("resolve_target: Client not found\n\
                                   target = {target:#?}"))?)
}

impl RecordKindVariant {
    fn from_kind(rk: &RecordKind) -> Option<RecordKindVariant> {
        Some(match rk {
            RecordKind::A(_)       => RecordKindVariant::A,
            RecordKind::PTR(_)     => RecordKindVariant::PTR,
            RecordKind::SRV { .. } => RecordKindVariant::SRV,
            RecordKind::TXT(_)     => RecordKindVariant::TXT,
            _ => return None,
        })
    }
}

#[named]
pub fn clients_from_mdns_response(resp: &mdns_lib::Response)
-> Result<Vec<Client>>
{
    const FUNCTION_PATH: &str = function_path!();

    tracing::trace!(target: FUNCTION_PATH,
                    ?resp,
                    "answers.len" = resp.answers.len(),
                    "nameservers.len" = resp.nameservers.len(),
                    "additional.len" = resp.additional.len(),
                    "mdns::Response");

    // TODO: Skip this conversion to a BTreeMap?

    // TODO: Move util structs and methods to mdns module
    // TODO: Perhaps add utils: RecordKind.as_ptr() -> Option<PtrRecord>
    // TODO: Perhaps add util: get typed record with host and kind

    // Map from record name and variant to records.
    let mut recs = BTreeMap::<(String, RecordKindVariant),
                              Vec<&mdns_lib::Record>>::new();

    for rec in resp.answers.iter().chain(resp.additional.iter()) {
        let rec: &mdns_lib::Record = rec;
        let Some(kind) = RecordKindVariant::from_kind(&rec.kind) else {
            continue;
        };

        let recs_for_name =
            recs.entry((rec.name.clone(), kind))
            .or_insert(Vec::with_capacity(1));
        recs_for_name.push(rec);
    }

    let recs = recs;

    let mut clients = Vec::new();

    for ptr_rec
        in recs.get(&(SERVICE_NAME.to_string(), RecordKindVariant::PTR))
               .into_iter().flatten()
    {
        let service_host = match ptr_rec.kind {
            RecordKind::PTR(ref h) => h.as_str(),
            _ => panic!("{FUNCTION_PATH}: logic bug: wrong RecordKind."),
        };

        let txt_rec: Option::<(&mdns_lib::Record, &Vec<String>)> =
            recs.get(&(service_host.to_string(), RecordKindVariant::TXT))
                .into_iter().flatten()
                .find_map(|rec| Some((*rec,
                                      match &rec.kind {
                                          RecordKind::TXT(entries) => entries,
                                          _ => panic!("{FUNCTION_PATH}: logic bug: \
                                                       wrong RecordKind."),
                                      })));

        let mut app_name = None::<String>;
        let mut friendly_name = None::<String>;
        let mut uuid = None::<String>;

        for entry in txt_rec.iter().map(|ts| ts.1).flatten() {
            let Some((k, v)) = entry.split_once('=') else {
                continue;
            };

            match k {
                "fn" => friendly_name = Some(v.to_string()),
                "rs" => app_name = Some(v.to_string()),
                "id" => uuid = Some(v.to_string()),
                _ => (),
            };
        }

         let srv_rec: Option::<(&mdns_lib::Record, &str, u16)> =
            recs.get(&(service_host.to_string(), RecordKindVariant::SRV))
                .into_iter().flatten()
                .find_map(|rec| match &rec.kind {
                    RecordKind::SRV { target, port, .. } =>
                        Some((*rec, target.as_str(), *port)),

                    _ => panic!("{FUNCTION_PATH}: logic bug: \
                                 wrong RecordKind."),
                });

        let a_rec: Option<(&mdns_lib::Record, std::net::IpAddr)>;

        if let Some((_, target, port)) = srv_rec {
            // TODO: AAAA?
            a_rec =
                recs.get(&(target.to_string(), RecordKindVariant::A))
                    .into_iter().flatten()
                    .find_map(|rec| match &rec.kind {
                        RecordKind::A(ipv4) =>
                            Some((*rec, std::net::IpAddr::from(*ipv4))),
                        _ => panic!("{FUNCTION_PATH}: logic bug: \
                                     wrong RecordKind."),
                    });

            if let Some((_rec, ip)) = a_rec {
                let client = Client {
                    addr: SocketAddr::from((ip, port)),
                    service_host: service_host.to_string(),
                    display_name: friendly_name.unwrap_or_else(
                        || format!("{ip}:{port}")),
                    app_name,
                    uuid,
                };

                tracing::trace!(target: FUNCTION_PATH,
                                ?client,
                                "client");

                clients.push(client);
            }
        } else {
            a_rec = None;
        }

        tracing::trace!(target: FUNCTION_PATH,
                        ?ptr_rec, service_host, ?txt_rec, ?srv_rec, ?a_rec,
                        "parsed components");
    } // for each PTR record in response

    Ok(clients)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn extract_from_mdns_response() -> Result<()> {
        use mdns_lib::{Record, RecordKind, Response};
        use std::net::Ipv4Addr;

        // crate::test::ensure_init();

        let class = dns_parser::Class::IN;
        let ttl = 123_u32;

        let id_uuid = "3123456789-abcd-0123-4567-89abcdef0123";
        let id = id_uuid.replace('-', "");
        let srv_name = format!("Chromecast-{id}.{SERVICE_NAME}");
        let a_name = format!("{id_uuid}.local");

        let ip = Ipv4Addr::from([192,168,17,42]);
        let port = 8009;

        let friendly_name = "My Chromecast".to_string();
        let app_name = "YouTube".to_string();

        let resp = Response {
            answers: vec![
                Record {
                    name: SERVICE_NAME.to_string(),
                    kind: RecordKind::PTR(srv_name.clone()),
                    class, ttl
                },
            ],
            nameservers: vec![],
            additional: vec![
                Record {
                    name: srv_name.clone(),
                    kind: RecordKind::TXT([
                        &format!("id={id}"),
                        "cd=0123456789ABCDEF0123456789ABCDEF",
                        "rm=1123456789ABCDEF",
                        "ve=05",
                        "md=Chromecast",
                        "ic=/setup/icon.png",
                        &format!("fn={friendly_name}"),
                        "ca=201221",
                        "st=1",
                        "bs=2123456789AB",
                        "nf=1",
                        &format!("rs={app_name}"),
                    ].into_iter()
                     .map(|s| s.to_string())
                     .collect()),
                    class, ttl
                },
                Record {
                    name: srv_name.clone(),
                    kind: RecordKind::SRV {
                        priority: 0, weight: 0, port,
                        target: a_name.clone(),
                    },
                    class, ttl
                },
                Record {
                    name: a_name.clone(),
                    kind: RecordKind::A(ip),
                    class, ttl
                },
            ],
        };
        println!("response = {resp:#?}");

        let clients = clients_from_mdns_response(&resp)?;
        println!("clients = {clients:#?}");

        assert_eq!(clients.len(), 1);

        let client = &clients[0];

        assert_eq!(client.service_host, srv_name);
        assert_eq!(client.addr, SocketAddr::from((ip, port)));
        assert_eq!(client.display_name, friendly_name);
        assert_eq!(client.app_name, Some(app_name));
        assert_eq!(client.uuid, Some(id));

        Ok(())
    }
}
