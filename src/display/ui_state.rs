use std::{
    cmp,
    collections::{HashMap, HashSet, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use log::warn;

use crate::{
    display::BandwidthUnitFamily,
    network::{LocalSocket, Utilization},
    os::ProcessInfo,
};

static HISTORY_LENGTH: usize = 40;
static MAX_BANDWIDTH_ITEMS: usize = 1000;

#[derive(Clone, Default)]
pub struct NetworkData {
    pub total_bytes_downloaded: u128,
    pub total_bytes_uploaded: u128,
}

#[derive(Clone, Default)]
pub struct ProcessHistory {
    pub total_bytes_downloaded: u128,
    pub total_bytes_uploaded: u128,
    pub download_history: VecDeque<f64>,
    pub upload_history: VecDeque<f64>,
}

#[derive(Clone)]
pub struct ProcessRow {
    pub process: ProcessInfo,
    pub current_bytes_downloaded: u128,
    pub current_bytes_uploaded: u128,
    pub total_bytes_downloaded: u128,
    pub total_bytes_uploaded: u128,
    pub download_history: VecDeque<f64>,
    pub upload_history: VecDeque<f64>,
}

#[derive(Default)]
pub struct UIState {
    /// The interface name in single-interface mode. `None` means all interfaces.
    pub interface_name: Option<String>,
    pub total_bytes_downloaded: u128,
    pub total_bytes_uploaded: u128,
    pub unit_family: BandwidthUnitFamily,
    pub process_rows: Vec<ProcessRow>,
    process_history: HashMap<ProcessInfo, ProcessHistory>,
    /// Used for reducing logging noise.
    known_orphan_sockets: VecDeque<LocalSocket>,
}

impl UIState {
    pub fn update(
        &mut self,
        connections_to_procs: HashMap<LocalSocket, ProcessInfo>,
        network_utilization: Utilization,
    ) {
        let mut processes: HashMap<ProcessInfo, NetworkData> = HashMap::new();
        let mut total_bytes_downloaded: u128 = 0;
        let mut total_bytes_uploaded: u128 = 0;

        for (connection, connection_info) in &network_utilization.connections {
            total_bytes_downloaded += connection_info.total_bytes_downloaded;
            total_bytes_uploaded += connection_info.total_bytes_uploaded;

            let local_socket = connection.local_socket;
            let proc_info = get_proc_info(&connections_to_procs, &local_socket);

            if proc_info.is_none() && !self.known_orphan_sockets.contains(&local_socket) {
                self.known_orphan_sockets.push_front(local_socket);
                self.known_orphan_sockets.truncate(10_000);

                match connections_to_procs
                    .iter()
                    .find(|(&LocalSocket { port, protocol, .. }, _)| {
                        port == local_socket.port && protocol == local_socket.protocol
                    })
                    .and_then(|(local_conn_lookalike, info)| {
                        network_utilization
                            .connections
                            .keys()
                            .find(|conn| &conn.local_socket == local_conn_lookalike)
                            .map(|conn| (conn, info))
                    }) {
                    Some((lookalike, proc_info)) => {
                        warn!(
                            r#""{0}" owns a similar looking connection, but its local ip doesn't match."#,
                            proc_info.name
                        );
                        warn!("Looking for: {connection:?}; found: {lookalike:?}");
                    }
                    None => {
                        warn!("Cannot determine which process owns {connection:?}");
                    }
                };
            }

            let proc_info = proc_info
                .cloned()
                .unwrap_or_else(|| ProcessInfo::new("<UNKNOWN>", 0));
            let data_for_process = processes.entry(proc_info).or_default();

            data_for_process.total_bytes_downloaded += connection_info.total_bytes_downloaded;
            data_for_process.total_bytes_uploaded += connection_info.total_bytes_uploaded;
        }

        self.total_bytes_downloaded += total_bytes_downloaded;
        self.total_bytes_uploaded += total_bytes_uploaded;

        let mut updated_processes = HashSet::new();
        for (proc_info, data) in &processes {
            updated_processes.insert(proc_info.clone());
            let history = self.process_history.entry(proc_info.clone()).or_default();
            history.total_bytes_downloaded += data.total_bytes_downloaded;
            history.total_bytes_uploaded += data.total_bytes_uploaded;
            history
                .download_history
                .push_back(data.total_bytes_downloaded as f64);
            history
                .upload_history
                .push_back(data.total_bytes_uploaded as f64);
            trim_history(history);
        }

        for (proc_info, history) in self.process_history.iter_mut() {
            if !updated_processes.contains(proc_info) {
                history.download_history.push_back(0.0);
                history.upload_history.push_back(0.0);
                trim_history(history);
            }
        }

        let mut rows = self
            .process_history
            .iter()
            .map(|(proc_info, history)| {
                let current = processes.get(proc_info).cloned().unwrap_or_default();
                ProcessRow {
                    process: proc_info.clone(),
                    current_bytes_downloaded: current.total_bytes_downloaded,
                    current_bytes_uploaded: current.total_bytes_uploaded,
                    total_bytes_downloaded: history.total_bytes_downloaded,
                    total_bytes_uploaded: history.total_bytes_uploaded,
                    download_history: history.download_history.clone(),
                    upload_history: history.upload_history.clone(),
                }
            })
            .collect::<Vec<_>>();

        rows.sort_by_key(|row| cmp::Reverse(row.total_bytes_downloaded));
        if rows.len() > MAX_BANDWIDTH_ITEMS {
            rows.truncate(MAX_BANDWIDTH_ITEMS);
        }
        self.process_rows = rows;
    }
}

fn trim_history(history: &mut ProcessHistory) {
    while history.download_history.len() > HISTORY_LENGTH {
        history.download_history.pop_front();
    }
    while history.upload_history.len() > HISTORY_LENGTH {
        history.upload_history.pop_front();
    }
}

fn get_proc_info<'a>(
    connections_to_procs: &'a HashMap<LocalSocket, ProcessInfo>,
    local_socket: &LocalSocket,
) -> Option<&'a ProcessInfo> {
    connections_to_procs
        // direct match
        .get(local_socket)
        // IPv4-mapped IPv6 addresses
        .or_else(|| {
            let swapped: IpAddr = match local_socket.ip {
                IpAddr::V4(v4) => v4.to_ipv6_mapped().into(),
                IpAddr::V6(v6) => v6.to_ipv4_mapped()?.into(),
            };
            connections_to_procs.get(&LocalSocket {
                ip: swapped,
                ..*local_socket
            })
        })
        // address unspecified
        .or_else(|| {
            connections_to_procs.get(&LocalSocket {
                ip: Ipv4Addr::UNSPECIFIED.into(),
                ..*local_socket
            })
        })
        .or_else(|| {
            connections_to_procs.get(&LocalSocket {
                ip: Ipv6Addr::UNSPECIFIED.into(),
                ..*local_socket
            })
        })
}
