//TODO: Eventually use Bevy taskpool instead of tokio runtime

use std::fmt::Debug;
use std::sync::Arc;

pub use tokio::io::AsyncWriteExt;
pub use tokio::sync::mpsc::unbounded_channel;
use tokio::net::ToSocketAddrs;

use native_client::NativeClient;
use native_server::NativeServer;
use native_shared::*;

pub enum NativeNetResourceWrapper {
    Server(NativeServer),
    Client(NativeClient),

}

//TODO: Generic new fn?
impl NativeNetResourceWrapper {
    pub fn new_server(task_pool: Runtime) -> Self {
        NativeNetResourceWrapper::Server(NativeServer::new(task_pool))

    }

    pub fn new_client(task_pool: Runtime) -> Self {
        NativeNetResourceWrapper::Client(NativeClient::new(task_pool))
    }

    pub fn process_message_channel<T>(&self, channel_id: &MessageChannelID) -> Result<Vec<(ConnectionHandle, T)>, ChannelProcessingError> where T: ChannelMessage + Debug + Clone {
        let unprocessed_messages_recv_queue = match self {
            NativeNetResourceWrapper::Server(res) => Arc::clone(&res.unprocessed_message_recv_queue),
            NativeNetResourceWrapper::Client(res) => Arc::clone(&res.unprocessed_messages),


        };

        let result = match unprocessed_messages_recv_queue.get_mut(channel_id) {
            Some(mut unprocessed_channel) => {
                // Iterates through the channel while clearing it
                let processed_messages: Result<Vec<(ConnectionHandle, T)>, ChannelProcessingError> = unprocessed_channel.drain(..).map(|(handle, message_bin) | {
                    match bincode::deserialize::<T>(&message_bin) {
                        Ok(msg) => Ok((handle, msg)),
                        Err(e) => Err(e.into()),
                    }

                }).collect();

                processed_messages
                
            },
            None => {
                unprocessed_messages_recv_queue.insert(channel_id.clone(), Vec::with_capacity(5));
                Ok(Vec::new())

            },

        };

        result

    }

    pub fn is_connected(&self) -> bool {
        match self {
            NativeNetResourceWrapper::Server(res) => res.tcp_connected_clients.len() > 0,
            NativeNetResourceWrapper::Client(res) => res.tcp_msg_sender.lock().is_some(),
        }

    }

    pub fn is_server(&self) -> bool {
        #[cfg(feature = "native")]
        match self {
            NativeNetResourceWrapper::Server(_) => true,
            _ => false,
        }

        // Web builds are never servers (for now)
        #[cfg(feature = "web")]
        false
    }

    pub fn is_client(&self) -> bool {
        !self.is_server()

    }
}

impl NativeResourceTrait for NativeNetResourceWrapper {
    fn setup(&mut self, tcp_addr: impl ToSocketAddrs + Send + Clone + 'static, udp_addr: impl ToSocketAddrs + Clone + Send + 'static, max_packet_size: usize) {
        match self {
            NativeNetResourceWrapper::Server(res) => res.setup(tcp_addr, udp_addr, max_packet_size),
            NativeNetResourceWrapper::Client(res) => res.setup(tcp_addr, udp_addr, max_packet_size),
        }
    }

    fn broadcast_message<M>(&self, message: &M, channel: &MessageChannelID) -> Result<(), SendMessageError>
        where M: ChannelMessage + Debug + Clone {
        match self {
            NativeNetResourceWrapper::Server(res) => res.broadcast_message(message, channel),
            NativeNetResourceWrapper::Client(res) => res.broadcast_message(message, channel),
        }
    }

    fn send_message<M>(&self, message: &M, channel: &MessageChannelID, conn_id: &ConnID) -> Result<(), SendMessageError>
        where M: ChannelMessage + Debug {
        match self {
            NativeNetResourceWrapper::Server(res) => res.send_message(message, channel, conn_id),
            NativeNetResourceWrapper::Client(res) => res.send_message(message, channel, conn_id),
        }
    }

    fn register_message(&self, channel: &MessageChannelID, mode: native_shared::ChannelType) -> Result<(), native_shared::ChannelAlreadyRegistered> {
        match self {
            NativeNetResourceWrapper::Server(res) => res.register_message(channel, mode),
            NativeNetResourceWrapper::Client(res) => res.register_message(channel, mode),
        }
    }

    fn disconnect_from(&mut self, conn_id: &ConnID) -> Result<(), native_shared::DisconnectError> {
        match self {
            NativeNetResourceWrapper::Server(res) => res.disconnect_from(conn_id),
            NativeNetResourceWrapper::Client(res) => res.disconnect_from(conn_id),
        }
    }

    fn disconnect_from_all(&mut self) {
        match self {
            NativeNetResourceWrapper::Server(res) => res.disconnect_from_all(),
            NativeNetResourceWrapper::Client(res) => res.disconnect_from_all(),
        }
    }

}

pub type Runtime = Arc<tokio::runtime::Runtime>;

