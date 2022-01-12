#![deny(clippy::all)]

// Some code that's needed by both net_native and bootleg_networking
use std::fmt::Debug;
use std::net::SocketAddr;

pub use turbulence::message_channels::{ChannelAlreadyRegistered, ChannelMessage};
use turbulence::message_channels::{MessageTypeUnregistered};


#[cfg(feature = "native")]
use tokio::sync::mpsc::error::SendError;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct MessageChannelID {
    pub id: u8,
}

impl MessageChannelID {
    pub const fn new(id: u8) -> Self {
        Self {
            id,

        }
    }
}

#[derive(Debug)]
pub enum SendMessageError {
    Bincode(bincode::Error),
    #[cfg(feature = "native")]
    Mpsc(SendError<Vec<u8>>),
    NotConnected,
    MessageTypeUnregistered,
    MessageTooLarge,
}

impl From<bincode::Error> for SendMessageError {
    fn from(error: bincode::Error) -> Self {
        Self::Bincode(error)
    }
}


#[cfg(feature = "native")]
impl From<SendError<Vec<u8>>> for SendMessageError {
    fn from(error: SendError<Vec<u8>>) -> Self {
        Self::Mpsc(error)
    }
}

impl From<MessageTypeUnregistered> for SendMessageError {
    fn from(_error: MessageTypeUnregistered) -> Self {
        Self::MessageTypeUnregistered
    }
}

#[derive(Debug)]
pub enum ChannelProcessingError {
    Bincode(bincode::Error),
    Turbulence(MessageTypeUnregistered),
}

impl From<bincode::Error> for ChannelProcessingError {
    fn from(error: bincode::Error) -> Self {
        Self::Bincode(error)
    }
}

impl From<MessageTypeUnregistered> for ChannelProcessingError {
    fn from(error: MessageTypeUnregistered) -> Self {
        Self::Turbulence(error)
    }
}

/// A connection handler built to use both native and naia handles
#[derive(Clone, Debug)]
pub enum ConnectionHandle {
    Native(ConnID),
    Naia(u32),
}

impl ConnectionHandle {
    pub const fn new_native(conn_id: ConnID) -> Self {
        ConnectionHandle::Native(conn_id)

    }

    pub const fn new_naia(handle: u32) -> Self {
        ConnectionHandle::Naia(handle)

    }

    pub fn native(&self) -> &ConnID {
        match *self {
            ConnectionHandle::Native(ref id) => id,
            ConnectionHandle::Naia(_) => panic!("Naia"),

        }
    }

    pub fn naia(&self) -> &u32 {
        match *self {
            ConnectionHandle::Naia(ref handle) => handle,
            ConnectionHandle::Native(_) => panic!("Native"),

        }
    }

    pub const fn is_native(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        match self {
            ConnectionHandle::Native(_) => true,
            ConnectionHandle::Naia(_) => false,
        }

        #[cfg(target_arch = "wasm32")]
        false
    }

    pub const fn is_naia(&self) -> bool {
        !self.is_native()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnID {
    pub uuid: u32,
    pub addr: SocketAddr,
    pub mode: NativeConnectionType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NativeConnectionType {
    Tcp,
    Udp,
}

impl ConnID {
    pub const fn new(uuid: u32, addr: SocketAddr, mode: NativeConnectionType) -> Self {
        Self {
            uuid,
            addr,
            mode,
        }
    }
}

#[derive(Debug)]
pub enum DisconnectError {
    NotConnected,
}

pub fn generate_message_bin<T>(message: &T, channel: &MessageChannelID) -> Result<Vec<u8>, bincode::Error> where T: ChannelMessage + Debug {
    let msg_bin = bincode::serialize(message)?;
    // Add one extra byte to the message length for the channel ID
    let msg_len: u32 = msg_bin.len().try_into().unwrap();

    // 4 bytes for the length, 1 byte for the channel ID, the rest for the actual message
    let mut final_message_bin = Vec::with_capacity(4 + 1 + msg_bin.len());

    final_message_bin.extend_from_slice(&msg_len.to_be_bytes());
    final_message_bin.push(channel.id);

    final_message_bin.extend_from_slice(&msg_bin);

    // Just a check to make sure we're properly encoding the message
    debug_assert_eq!(usize::try_from(u32::from_be_bytes(final_message_bin.as_slice()[..4].try_into().unwrap())).unwrap(), final_message_bin.as_slice()[5..].len());

    Ok(final_message_bin)
}
