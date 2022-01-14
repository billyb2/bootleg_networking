#![deny(clippy::all)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::fmt::Debug;
use dashmap::DashMap;
use crossbeam_channel::{Sender, Receiver, unbounded as crossbeam_unbounded};

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, ToSocketAddrs, UdpSocket};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;

use native_shared::*;

// Basically, there's the top level hashmap of each messagechannel, with a vec below that consisting of every message recipient and a binary version of the packet
pub struct NativeServer {
    pub task_pool: Arc<Runtime>,
    pub tcp_connection_handler: Option<JoinHandle<()>>,
    pub udp_connection_handler: Option<JoinHandle<()>>,
    pub tcp_connected_clients: Arc<DashMap<ConnID, TcpCliConn>>,
    pub udp_connected_clients: Arc<DashMap<ConnID, UdpCliConn>>,
    pub registered_channels: Arc<DashMap<MessageChannelID, ChannelType>>,
    pub unprocessed_message_recv_queue: RecvQueue,
    pub server_handle: Option<JoinHandle<()>>,
    pub next_uuid: Arc<AtomicU32>,
    disconnect_event_sender: Sender<ConnID>,
    disconnect_event_receiver: Receiver<ConnID>,

}

impl NativeServer {
    pub fn new(task_pool: Arc<Runtime>) -> Self {
        let (disconnect_event_sender, disconnect_event_receiver) = crossbeam_unbounded();

        Self {
            task_pool,
            tcp_connected_clients: Arc::new(DashMap::new()),
            udp_connected_clients: Arc::new(DashMap::new()),
            server_handle: None,
            tcp_connection_handler: None,
            udp_connection_handler: None,
            unprocessed_message_recv_queue: Arc::new(DashMap::new()),
            registered_channels: Arc::new(DashMap::new()),
            next_uuid: Arc::new(AtomicU32::new(0)),
            disconnect_event_sender,
            disconnect_event_receiver,
        }
    }
}

impl NativeResourceTrait for NativeServer {
    fn setup(&mut self, tcp_addr: impl ToSocketAddrs + Send + Clone + 'static, udp_addr: impl ToSocketAddrs + Send + Clone + 'static, max_packet_size: usize) {
        let (reliable_conn_send, mut reliable_conn_recv) = unbounded_channel();

        // Arc clones of some of self to prevent moving
        let task_pool = Arc::clone(&self.task_pool);
        let task_pool_2 = Arc::clone(&self.task_pool);
        let task_pool_clone = Arc::clone(&self.task_pool);

        let tcp_connected_clients_clone = Arc::clone(&self.tcp_connected_clients);
        let udp_connected_clients_clone = Arc::clone(&self.udp_connected_clients);

        let tcp_connected_clients_clone_2 = Arc::clone(&self.tcp_connected_clients);
        let udp_connected_clients_clone_2 = Arc::clone(&self.udp_connected_clients);

        let msg_rcv_queue = Arc::clone(&self.unprocessed_message_recv_queue);
        let msg_rcv_queue_2 = Arc::clone(&self.unprocessed_message_recv_queue);
        let next_uuid = Arc::clone(&self.next_uuid);
        let next_uuid_2 = Arc::clone(&self.next_uuid);

        let (req_destroy_cli, mut clients_to_destroy) = unbounded_channel::<ConnID>();

        let req_destroy_cli_clone_2 = req_destroy_cli.clone();

        let tcp_listen_loop = async move {
            let listener = TcpListener::bind(tcp_addr).await.unwrap();

            while let Ok(socket_and_addr) = listener.accept().await {
                reliable_conn_send.send(socket_and_addr).unwrap();

            }

        };

        let udp_listen_loop = async move {
            let sock = UdpSocket::bind(udp_addr).await.unwrap();
            let arc_sock = Arc::new(sock);

            let mut buffer: Vec<u8> = vec![0; max_packet_size];

            while let Ok((_num_bytes_read, recv_addr)) = arc_sock.recv_from(&mut buffer).await {
                // Checks to see if the addr is in the connected_clients dashmap, and if it isn't, it adds it
                let mut conn_id = None;

                let not_connected = udp_connected_clients_clone.iter().find(|key_val| {
                    let local_conn_id = key_val.key();

                    if local_conn_id.addr == recv_addr {
                        conn_id = Some(local_conn_id.clone());
                        true

                    } else {
                        false
                    }

                }).is_none();

                // If the address isn't already in the DashMap, add it
                if not_connected {
                    let next_uuid = next_uuid_2.fetch_add(1, Ordering::Relaxed);
                    let (msg_send, mut msg_to_send) = unbounded_channel::<Vec<u8>>();

                    let arc_sock_clone = Arc::clone(&arc_sock);

                    let new_conn_id = ConnID::new(next_uuid, recv_addr.clone(), NativeConnectionType::Udp);
                    conn_id = Some(new_conn_id.clone());

                    udp_connected_clients_clone.insert(new_conn_id, UdpCliConn {
                        send_task: task_pool_2.spawn(async move {
                            let recv_addr = recv_addr.clone();

                            while let Some(msg) = msg_to_send.recv().await {
                                arc_sock_clone.send_to(&msg, recv_addr.clone()).await.unwrap();

                            }

                        }),
                        send_message: msg_send,
                    });

                }

                let msg_len: usize = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]).try_into().unwrap();
                let channel_id = MessageChannelID::new(buffer[4]);

                if msg_len > max_packet_size {
                    eprintln!("Received a packet that was too big!\nPacket was {} bytes", msg_len);
                    break;
                }

                let msg_buffer = &mut buffer[5..msg_len + 5];

                //let num_bytes_read = num_bytes_read - 4;

                // If these differ, we read a corrupted message
                // TODO: Error something
                //assert_eq!(msg_len, num_bytes_read);

                let mut key_val_pair = msg_rcv_queue_2.get_mut(&channel_id).unwrap();
                let messages = key_val_pair.value_mut();

                let byte_vec = msg_buffer.to_vec();

                messages.push((ConnectionHandle::new_native(conn_id.unwrap()), byte_vec));

            }

        };


        let disconnect_event_sender = self.disconnect_event_sender.clone();

        task_pool.spawn(async move {
            let tcp_conn_cli = tcp_connected_clients_clone_2;
            let udp_conn_cli = udp_connected_clients_clone_2;

            while let Some(cli) = clients_to_destroy.recv().await {
                match cli.mode {
                    NativeConnectionType::Tcp => {tcp_conn_cli.remove(&cli);},
                    NativeConnectionType::Udp => {udp_conn_cli.remove(&cli);},
                };

                disconnect_event_sender.clone().send(cli).unwrap();

            }

        });

        let handle_connections = async move {
            let task_pool = task_pool_clone;
            let connected_clients = tcp_connected_clients_clone;

            while let Some((socket, addr)) = reliable_conn_recv.recv().await {
                let req_destroy_clone = req_destroy_cli_clone_2.clone();

                let task_pool = Arc::clone(&task_pool);

                let msg_rcv_queue = Arc::clone(&msg_rcv_queue);
                let (tcp_read_socket, mut tcp_write_socket) = socket.into_split();

                let (tcp_message_sender, mut tcp_messages_to_send) = unbounded_channel::<Vec<u8>>();

                let next_uuid = next_uuid.fetch_add(1, Ordering::Relaxed);

                let conn_id = ConnID {
                    uuid: next_uuid,
                    addr,
                    mode: NativeConnectionType::Tcp,
                };
                let conn_id_clone = conn_id.clone();

                let handle = ConnectionHandle::new_native(conn_id.clone());

                task_pool.spawn(tcp_add_to_msg_queue(tcp_read_socket, msg_rcv_queue, handle, max_packet_size));

                connected_clients.insert(
                    conn_id,
                    TcpCliConn {
                        send_task: task_pool.spawn(async move {
                            let conn_id = conn_id_clone;
                            let req_destroy_cli = req_destroy_clone;

                            while let Some(message) = tcp_messages_to_send.recv().await {
                                if let Some(err) = tcp_write_socket.write_all(&message).await.err() {
                                    use std::io::ErrorKind;

                                    match err.kind() {
                                        ErrorKind::BrokenPipe | ErrorKind::ConnectionRefused => {
                                            // Should gracefully close connection and shutdown tokio task
                                            req_destroy_cli.clone().send(conn_id.clone()).unwrap();
                                            break;
                                        },
                                        // Any other errors should also close the connection, and shutdown the tokio task
                                        _ => {
                                            req_destroy_cli.clone().send(conn_id.clone()).unwrap();
                                            break;
                                        },

                                    }

                                }

                            }

                        }),
                        send_message: tcp_message_sender,
                    }
                );

            }

        };      

        self.udp_connection_handler = Some(self.task_pool.spawn(udp_listen_loop));
        self.tcp_connection_handler = Some(self.task_pool.spawn(handle_connections));
        self.server_handle = Some(self.task_pool.spawn(tcp_listen_loop));

    }

    fn broadcast_message<T>(&self, message: &T, channel: &MessageChannelID) -> Result<(), SendMessageError> where T: ChannelMessage + Debug + Clone {
        let message_bin = generate_message_bin(message, channel)?;

        // TODO: Return an error
        let key_val_pair = match self.registered_channels.get(channel) {
            Some(channel) => channel,
            None => return Err(SendMessageError::MessageTypeUnregistered),

        };
        let mode = key_val_pair.value();

        match mode {
            ChannelType::Reliable => {
                for key_val_pair in self.tcp_connected_clients.iter() {
                    let conn = key_val_pair.value();
                    match conn.send_message.send(message_bin.clone()) {
                        Ok(()) => (),
                        Err(_) => Err(SendMessageError::NotConnected)?,
                    }

                }

            },
            ChannelType::Unreliable => {
                for key_val_pair in self.udp_connected_clients.iter() {
                    let conn = key_val_pair.value();
                    match conn.send_message.send(message_bin.clone()) {
                        Ok(()) => (),
                        Err(_) => Err(SendMessageError::NotConnected)?,
                    }

                }
            },
        };

        Ok(())

    }

    fn send_message<T>(&self, message: &T, channel: &MessageChannelID, conn_id: &ConnID) -> Result<(), SendMessageError> where T: ChannelMessage + Debug {
        let message_bin = generate_message_bin(message, channel)?;

        // TODO: Return an error
        let key_val_pair = self.registered_channels.get(channel).unwrap();
        let mode = key_val_pair.value();

        match mode {
            ChannelType::Reliable => {
                let cli = self.tcp_connected_clients.get(conn_id);

                match cli {
                    Some(cli) =>  {
                        let conn = cli.value();
                        conn.send_message.send(message_bin)?;
                    },
                    None => return Err(SendMessageError::NotConnected),

                }

            },
            ChannelType::Unreliable => {
                let cli = self.udp_connected_clients.get(conn_id);

                match cli {
                    Some(cli) =>  {
                        let conn = cli.value();
                        conn.send_message.send(message_bin)?;
                    },
                    None => return Err(SendMessageError::NotConnected),

                }

            },
        };

        Ok(())

    }

    fn register_message(&self, channel: &MessageChannelID, mode: ChannelType) -> Result<(), ChannelAlreadyRegistered> {
        match self.unprocessed_message_recv_queue.contains_key(channel) {
            false => {
                self.registered_channels.insert(channel.clone(), mode);
                self.unprocessed_message_recv_queue.insert(channel.clone(), Vec::with_capacity(5));

            },
            true => return Err(ChannelAlreadyRegistered::Channel),
        };

        Ok(())
        

    }

    fn disconnect_from(&mut self, conn_id: &ConnID) -> Result<(), DisconnectError> {
        match conn_id.mode {
            NativeConnectionType::Tcp => {
                match self.tcp_connected_clients.get(conn_id) {
                    Some(conn) => conn.send_task.abort(),
                    None => return Err(DisconnectError::NotConnected),
                };

                self.tcp_connected_clients.remove(conn_id);

            },
            NativeConnectionType::Udp => {
                match self.udp_connected_clients.get(conn_id) {
                    Some(conn) => conn.send_task.abort(),
                    None => return Err(DisconnectError::NotConnected),
                };

                self.udp_connected_clients.remove(conn_id);

            },
        };

        Ok(())

    }

    fn disconnect_from_all(&mut self) {
        self.tcp_connected_clients.clear();
        self.udp_connected_clients.clear();

    }

    fn rcv_disconnect_events(&self) -> Option<ConnID> {
        self.disconnect_event_receiver.try_recv().ok()

    }

}
