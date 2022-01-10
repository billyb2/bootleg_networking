#![deny(clippy::all)]

use std::sync::Arc;
use std::net::UdpSocket;

use dashmap::DashMap;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use crate::shared::*;
pub use crate::logic::*;

// Lists of binary messages, along with the handle of who sent it
pub type RecvQueue = Arc<DashMap<MessageChannelID, Vec<(ConnectionHandle, Vec<u8>)>>>;

pub struct TcpCliConn {
    pub send_task: JoinHandle<()>,
    pub send_message: UnboundedSender<Vec<u8>>,
}

pub struct UdpCliConn {
    pub send_task: JoinHandle<()>,
    pub send_message: UnboundedSender<Vec<u8>>,
}

pub enum ChannelType {
    Reliable, 
    Unreliable,
}

#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub fn get_available_port(ip: &str) -> Option<u16> {
    (8000..9000).into_iter().find(|port| port_is_available(ip, *port))
}

#[cfg(not(target_arch = "wasm32"))]
#[inline(always)]
fn port_is_available(ip: &str, port: u16) -> bool {
    UdpSocket::bind((ip, port)).is_ok()
}
