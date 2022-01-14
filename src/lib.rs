#![deny(missing_docs)]
#![deny(clippy::all)]

#![doc = include_str!("../README.md")]

use std::any::type_name;
use std::sync::atomic;
use std::fmt::Debug;
use std::net::{SocketAddr, ToSocketAddrs};
#[cfg(feature = "native")]
use std::sync::Arc;

use turbulence::message_channels::ChannelAlreadyRegistered;
use turbulence::{
    message_channels::ChannelMessage,
    packet::{Packet as PoolPacket, PacketPool},
};

use bevy_app::{App, Events, Plugin};
use bevy_ecs::system::ResMut;
use bevy_tasks::TaskPool;
use bevy_networking_turbulence::*;
use bevy_networking_turbulence::{
    NetworkResource as NaiaNetworkResource,
    ConnectionHandle as NaiaConnectionHandle
};
pub use bevy_networking_turbulence::{
    MessageChannelMode,
    ConnectionChannelsBuilder,
    MessageChannelSettings,
    ReliableChannelSettings,
};

#[cfg(feature = "native")]
use tokio::runtime::Builder;
#[cfg(feature = "native")]
use tokio::net::ToSocketAddrs as TokioToSocketAddrs;

use net_native::*;
pub use net_native::{ChannelProcessingError, MessageChannelID, SendMessageError, ConnectionHandle};
#[cfg(feature = "native")]
pub use net_native::Runtime;

/// Stores all the networking stuff
pub struct NetworkResource {
    // Sadly, web clients can't use TCP
    #[cfg(feature = "native")]
    native: NativeNetResourceWrapper,
    // Naia stuff is used for native WebRTC servers and wasm clients
    naia: Option<NaiaNetworkResource>,
    is_server: bool,
    is_setup: bool,

}

/// Fake Tokio runtime struct used for web compatibility
/// For example, you can use Res<Runtime> in Bevy without having to use optionals all the time
#[cfg(feature = "web")]
#[derive(Clone)]
pub struct Runtime;

impl NetworkResource {
    /// Constructs a new server, using both TCP and Naia
    #[cfg(feature = "native")]
    pub fn new_server(tokio_rt: Runtime, task_pool: TaskPool) -> Self {
        Self {
            native: NativeNetResourceWrapper::new_server(tokio_rt),
            naia: Some(NaiaNetworkResource::new(task_pool, None, MessageFlushingStrategy::OnEverySend, None, None)),
            is_server: true,
            is_setup: false,
        }
    }

    /// Constructs a new client. On web builds it uses Naia, while on native builds it uses the custom built native_client
    pub fn new_client(tokio_rt: Runtime, task_pool: TaskPool) -> Self {
        Self {
            #[cfg(feature = "native")]
            native: NativeNetResourceWrapper::new_client(tokio_rt),
            // The match statement should be optimized out by the compiler
            #[cfg(feature = "native")]
            naia: None,
            // Web clients should
            #[cfg(feature = "web")]
            naia: Some(NaiaNetworkResource::new(task_pool, None, MessageFlushingStrategy::OnEverySend, None, None)),
            is_server: false,
            is_setup: false,
        } 
    }

    /// Sets up listening for native servers
    /// Recommended to read the documentation for ListenConfig and ConnectConfig before proceeding
    /// The max_native_packet_size is only necessary for native builds
    #[cfg(feature = "native")]
    pub fn listen<T, W>(&mut self, config: ListenConfig<T, W>, max_native_packet_size: Option<usize>)
        where T: TokioToSocketAddrs + Send + Clone + 'static, W: ToSocketAddrs + Send + 'static{
        if self.is_server() {
            self.native.setup(config.tcp_addr, config.udp_addr, max_native_packet_size.unwrap());

            let naia = self.naia.as_mut().unwrap();

            let naia_addr = config.naia_addr.to_socket_addrs().unwrap().next().unwrap();
            let webrtc_listen_addr = config.webrtc_listen_addr.to_socket_addrs().unwrap().next().unwrap();
            let public_webrtc_listen_addr = config.public_webrtc_listen_addr.to_socket_addrs().unwrap().next().unwrap();

            naia.listen(naia_addr, Some(webrtc_listen_addr), Some(public_webrtc_listen_addr));


        } else {
            panic!("Tried to listen while client");

        }

        self.is_setup = true;

    }

    /// Connects to a server. Recommended to read the documentation for ListenConfig and ConnectConfig before proceeding
    pub fn connect<M, U>(&mut self, config: ConnectConfig<M, U>, max_native_packet_size: Option<usize>)
        where M: ConnectAddr, U: ConnectAddr {
        if self.is_client() {
            #[cfg(feature = "native")]
            self.native.setup(config.addr, config.udp_addr.unwrap(), max_native_packet_size.unwrap());

            #[cfg(feature = "web")]
            if let Some(naia) = self.naia.as_mut() {
                let addr = config.addr.to_socket_addrs().unwrap().next().unwrap();
                naia.connect(addr);

            }

        } else {
            panic!("Tried to connect while server");

        }

        self.is_setup = true;

    }

    /// View all the messages that a certain channel as received
    pub fn view_messages<M>(&mut self, channel: &MessageChannelID) -> Result<Vec<(ConnectionHandle, M)>, ChannelProcessingError>
        where M: ChannelMessage + Debug + Clone {
        let mut messages: Vec<(ConnectionHandle, M)> = Vec::new();

        #[cfg(feature = "native")]
        {
            let mut tcp_messages = self.native.process_message_channel(channel)?;
            messages.append(&mut tcp_messages);
        }

        if let Some(naia) = self.naia.as_mut() {
            for (handle, connection) in naia.connections.iter_mut() {
                let channels = connection.channels().unwrap();

                while let Some(message) = channels.try_recv::<M>()? {
                    messages.push((ConnectionHandle::new_naia(*handle), message));

                }
            }

        }

        Ok(messages)

    }

    /// Sends a message to each connected client on servers, and on client it is equivalent to send_message
    pub fn broadcast_message<M>(&mut self, message: &M, channel: &MessageChannelID) -> Result<(), SendMessageError>
        where M: ChannelMessage + Debug + Clone {
        #[cfg(feature = "native")]
        self.native.broadcast_message(message, channel)?;

        if let Some(naia) = self.naia.as_mut() {
            check_naia_message_len(message, channel)?;

            // Inlined version of naia.broadcast_message(), with some modifications
            if !naia.connections.is_empty() {
                for (_handle, connection) in naia.connections.iter_mut() {
                    let channels = connection.channels().unwrap();
                    // If the result is Some(msg), that means that the message channel is full, which is no bueno. 
                    //  There's probably a better way to do this (TODO?) but since I haven't run into this issue yet, 
                    //  I don't care lol
                    if channels.try_send(message.clone())?.is_some() {
                        panic!("Message channel full for type: {:?}", type_name::<M>());

                    }

                    // Since we're using OnEverySend channel flushing, we don't need the if statement in the normal fn
                    channels.try_flush::<M>()?;

                }
            // If there are no connections and the resource is a client
            } else if self.is_client() {
                return Err(SendMessageError::NotConnected);

            }

        }

        Ok(())

    }

    /// Sends a message to a specific connection handle
    pub fn send_message<M>(&mut self, message: &M, channel: &MessageChannelID, handle: &ConnectionHandle) -> Result<(), SendMessageError>
        where M: ChannelMessage + Debug + Clone {
        if handle.is_native() {
            #[cfg(feature = "native")]
            self.native.send_message(message, channel, handle.native())?;

        } else {
            let naia = self.naia.as_mut().unwrap();
            check_naia_message_len(message, channel)?;

            // Inlined version of naia.send_message(), with some modifications
            match naia.connections.get_mut(handle.naia()) {
                Some(connection) => {
                    let channels = connection.channels().unwrap();
                    if channels.try_send(message.clone())?.is_some() {
                        panic!("Message channel full for type: {:?}", type_name::<M>());

                    }

                    channels.try_flush::<M>()?;
                }
                None => return Err(SendMessageError::NotConnected),
            }

        }

        Ok(())

    }

    /// On native builds, this sets up a channel to use TCP or UDP. On web builds, this does nothing
    pub fn register_message_channel_native(&mut self, settings: MessageChannelSettings, channel: &MessageChannelID) -> Result<(), ChannelAlreadyRegistered> {
        #[cfg(feature = "native")]
        self.native.register_message(channel, match &settings.channel_mode {
            MessageChannelMode::Unreliable => ChannelType::Unreliable,
            _ => ChannelType::Reliable,

        })?;

        Ok(())
        
    }

    // TODO: Combine register_message_channel_native with this fn
    /// Used for registering message channels with naia.
    pub fn set_channels_builder<F>(&mut self, builder: F) where F: Fn(&mut ConnectionChannelsBuilder) + Send + Sync + 'static {
        if let Some(naia) = self.naia.as_mut() {
            naia.set_channels_builder(builder);
        }

    }

    /// Checks for a connection
    pub fn is_connected(&self) -> bool {
        let naia_connected = match self.naia.as_ref() {
            Some(naia) => !naia.connections.is_empty(),
            None => false,
        };


        let tcp_connected = {
            #[cfg(feature = "native")]
            let connected = self.native.is_connected();

            #[cfg(feature = "web")]
            let connected = false;

            connected

        };

        tcp_connected || naia_connected

    }

    /// Disconnects from a specific client on servers, and just disconnects from the server on clients
    pub fn disconnect_from(&mut self, handle: &ConnectionHandle) -> Result<(), DisconnectError> {
        #[cfg(feature = "native")]
        if handle.is_native() {
            let handle = handle.native();
            self.native.disconnect_from(handle)?;

        }

        if handle.is_naia() {
            let handle = handle.naia();

            match self.naia.as_mut() {
                Some(naia) => naia.disconnect(*handle),
                None => return Err(DisconnectError::NotConnected),

            };

        }

        if self.is_client() {
            self.is_setup = false;

        }

        Ok(())

    }

    /// Runs the disconnect function on every connection
    pub fn disconnect_from_all(&mut self) {
        #[cfg(feature = "native")]
        self.native.disconnect_from_all();

        if let Some(naia) = self.naia.as_mut() {
            naia.connections.clear()

        }

        if self.is_client() {
            self.is_setup = false;

        }

    }

    /// Checks for any disconnect events on the tcp/udp side. Will always return None on WASM
    /// Should be run in a loop to catch all disconnect events until it returns None, for example:
    /// ```rust ignore
    /// while let Some(disconnected_handle) = net.rcv_disconnect_events() {
    ///     ...
    /// }
    /// ```
    pub fn rcv_disconnect_events_native(&self) -> Option<ConnectionHandle> {
        #[cfg(feature = "native")]
        match self.native.rcv_disconnect_events() {
            Some(conn_id) => Some(ConnectionHandle::Native(conn_id)),
            None => None,

        }

        #[cfg(feature = "web")]
        None

    }

    /// Checks if it's a server
    pub fn is_server(&self) -> bool {
        self.is_server

    }

    /// Checks if it's a client
    pub fn is_client(&self) -> bool {
        !self.is_server
    }

    /// Returns true if either the listen function or the connect function has been run
    pub fn is_setup(&self) -> bool {
        self.is_setup
    }

    /// Returns a mutable refrence to the naia NetworkResource, if it exists
    pub(crate) fn as_naia_mut(&mut self) -> Option<&mut NaiaNetworkResource> {
        self.naia.as_mut()

    }
}

/// A plugin for setting up the NetworkResource
pub struct NetworkingPlugin;

impl Plugin for NetworkingPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "native")]
        let tokio_rt = Arc::new(Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap());

        #[cfg(feature = "native")]
        app.insert_resource(tokio_rt);

        #[cfg(feature = "web")]
        app.insert_resource(Runtime);

        app
        .add_event::<NetworkEvent>()
        .add_system(rcv_naia_packets);

    }
}

fn rcv_naia_packets(super_net: Option<ResMut<NetworkResource>>, mut network_events: ResMut<Events<NetworkEvent>>) {
    let mut net = match super_net {
        Some(it) => it,
        _ => return,
    };

    let naia = net.as_naia_mut();

    if naia.is_none() {
        return;

    }

    let net = naia.unwrap();

    let pending_connections: Vec<Box<dyn Connection>> = net.pending_connections.lock().unwrap().drain(..).collect();
    
    for mut conn in pending_connections {
        let handle: NaiaConnectionHandle = net
            .connection_sequence
            .fetch_add(1, atomic::Ordering::Relaxed);

        if let Some(channels_builder_fn) = net.channels_builder_fn.as_ref() {
            conn.build_channels(
                channels_builder_fn,
                net.runtime.clone(),
                net.packet_pool.clone(),
            );
        }

        net.connections.insert(handle, conn);
        network_events.send(NetworkEvent::Connected(handle));

    }

    let packet_pool = net.packet_pool.clone();
    for (handle, connection) in net.connections.iter_mut() {
        while let Some(result) = connection.receive() {
            match result {
                Ok(packet) => {
                    // heartbeat packets are empty
                    if packet.is_empty() {
                        // discard without sending a NetworkEvent
                        continue;
                    }

                    if let Some(channels_rx) = connection.channels_rx() {
                        let mut pool_packet = packet_pool.acquire();
                        pool_packet.resize(packet.len(), 0);
                        pool_packet[..].copy_from_slice(&*packet);

                        if let Err(err) = channels_rx.try_send(pool_packet) {
                           network_events.send(NetworkEvent::Error(
                                *handle,
                                NetworkError::TurbulenceChannelError(err),
                            ));
                        }

                    } else {
                        network_events.send(NetworkEvent::Packet(*handle, packet));
                    }
                }
                Err(err) => {
                    network_events.send(NetworkEvent::Error(*handle, err));
                }
            }
        }
    }
}

#[inline]
fn check_naia_message_len<M>(message: &M, channel: &MessageChannelID) -> Result<(), SendMessageError> where M: ChannelMessage + Debug {
    // Since WebRTC has a limit of how large messages can be of 1200 bytes, we limit the size of messages that naia can send to 1095 bytes (since overhead will make it add to 1200 bytes).
    // We only do this check for naia since native builds can handle large messages
    if generate_message_bin(message, channel).unwrap().len() > 1095 {
        return Err(SendMessageError::MessageTooLarge)

    } else {
        Ok(())

    }
}

/// The way of passing arguments for what ports the server should listen on
#[cfg(feature = "native")]
pub struct ListenConfig<T: TokioToSocketAddrs + Send + Clone + 'static, W: ToSocketAddrs + Send + 'static> {
    /// The socket address for reliable messages
    pub tcp_addr: T,
    /// The socket address used for unreliable messages
    pub udp_addr: T,
    /// The address that naia clients will connect to
    pub naia_addr: W,
    /// Typically recommended to be the same IP address as public_webrtc_listen_addr and naia_addr, and that the port is random and unique
    pub webrtc_listen_addr: W,
    /// Typically recommended to be the same IP address as webrtc_listen_addr and naia_addr, and that the port is random and unique
    pub public_webrtc_listen_addr: W,

}

/// Everything that a connection needs. On native, TokioToSocketAddrs is used, while on WASM, std's ToSocketAddrs is used
#[cfg(feature = "native")]
pub trait ConnectAddr: TokioToSocketAddrs + Send + Clone + 'static {}

/// Everything that a connection needs. On native, TokioToSocketAddrs is used, while on WASM, std's ToSocketAddrs is used
#[cfg(feature = "web")]
pub trait ConnectAddr: ToSocketAddrs + Send + Clone + 'static {}

#[cfg(feature = "web")]
impl<T: ToSocketAddrs + Send + Clone + 'static> ConnectAddr for T {}

#[cfg(feature = "native")]
impl<T: TokioToSocketAddrs + Send + Clone + 'static> ConnectAddr for T {}

#[derive(Clone)]
/// The way of telling the client what socket addresses to connect to
pub struct ConnectConfig<M: ConnectAddr, U: ConnectAddr> {
    /// The main address to connect to. Should be either a tcp_addr or a naia_addr as specified in ListenConfig
    pub addr: M,
    /// When using a native client, this should be the UDP address of the server to connect to as specified in ListenConfig
    /// On web builds, this can just be None
    pub udp_addr: Option<U>,
}
