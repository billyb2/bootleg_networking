// Some code that's needed by both net_native and super_net
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

#[derive(Clone, Debug)]
pub enum SuperConnectionHandle {
    Native(ConnID),
    Naia(u32),
}

impl SuperConnectionHandle {
    pub const fn new_native(conn_id: ConnID) -> Self {
        SuperConnectionHandle::Native(conn_id)

    }

    pub const fn new_naia(handle: u32) -> Self {
        SuperConnectionHandle::Naia(handle)

    }

    pub fn native(&self) -> &ConnID {
        match *self {
            SuperConnectionHandle::Native(ref id) => id,
            SuperConnectionHandle::Naia(_) => panic!("Naia"),

        }
    }

    pub fn naia(&self) -> &u32 {
        match *self {
            SuperConnectionHandle::Naia(ref handle) => handle,
            SuperConnectionHandle::Native(_) => panic!("Native"),

        }
    }

    pub fn is_native(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        match self {
            SuperConnectionHandle::Native(_) => true,
            SuperConnectionHandle::Naia(_) => false,
        }

        #[cfg(target_arch = "wasm32")]
        false
    }

    pub fn is_naia(&self) -> bool {
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
    pub fn new(uuid: u32, addr: SocketAddr, mode: NativeConnectionType) -> Self {
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
