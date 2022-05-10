# bootleg_networking
A cross platform (wasm included) networking library!

A networking plugin for the Bevy game engine that wraps around bevy_networking_turbulence and a custom TCP/UDP network server in order to make writing cross-platform multiplayer games fun and easy!

## Getting started
Currently, the library is not published on crates.io, due to a few forks of other popular libraries it currently uses. Because of this, in order to use the libary, you must specify it as a git dependency

```toml
[dependencies]
bootleg_networking = { version = "0.2", git = "https://github.com/billyb2/bootleg_networking" }
```

If you want to use this library with wasm, you should disable the native feature and enable the web feature

```toml
[dependencies]
bootleg_networking = { version = "0.2", git = "https://github.com/billyb2/bootleg_networking", default-features = false, features = ["web"]}
```

I recommend pinning to a specific commit, since there are currently no stability guarantees.

Below is an example of how to use this library. It mainly shows how to setup a basic server, although a client is nearly identical

```rust
use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use bootleg_networking::*;

const MESSAGE_CHANNEL_ID: MessageChannelID = MessageChannelID::new(0);
const MESSAGE_SETTINGS: MessageChannelSettings = MessageChannelSettings {
    channel: MESSAGE_CHANNEL_ID.id,
    channel_mode: MessageChannelMode::Unreliable {
		settings: turbulence::unreliable_channel::Settings {
			bandwidth: 4096,
			burst_bandwidth: 1024,
		},
		max_message_len: 256,	
	},
    message_buffer_size: 256,
    packet_buffer_size: 256,
};

fn main() {
    let mut app = App::new();
    app
    .add_plugins(MinimalPlugins)
    .add_plugin(NetworkingPlugin)
    .add_startup_system(setup)
    .add_system(send)
    .add_system(receive);

    //Uncomment the line below!
    //app.run();
}

fn setup(mut commands: Commands, tokio_rt: Res<Runtime>, task_pool: Res<IoTaskPool>) {
    // First we need to actually initiate the NetworkReource. In this case, it's a server
    // We could use the new_client function if wanted a client
    let mut net = NetworkResource::new_server(tokio_rt.clone(), task_pool.0.clone());

    // Next, we need tell the server to setup listening
    // The equivalent function for clients is connect
    // Listen on ports 9000 for TCP and 9001 for UDP, and 9003
    // The first address is the one that the connect() function needs to use, and the other two are for WebRTC
    // Finally, the last argument is the maximum size of each packet. That argument is only necessary for native builds
    let listen_config = ListenConfig {
        tcp_addr: "127.0.0.1:9000",
        udp_addr: "127.0.0.1:9001",
        naia_addr: "127.0.0.1:9003",
        webrtc_listen_addr: "127.0.0.1:9004",
        public_webrtc_listen_addr: "127.0.0.1:9004",
    };

    net.listen(listen_config, Some(2048));
    // If we were calling net.connect, the first argument we would either have 9000 or 9003 as the port, depending on whether we were a native client or a web client
    // The second argument is only necessary on native builds, and it's asking for the UDP server SocketAddr
    /* let connect_config = ConnectConfig {
     *      addr: "127.0.0.1:9000",
     *      udp_addr: Some("127.0.0.1:9001"),
     * };
     *
     * net.connect(connect_config, Some(2048));
    */

    // We need to register for the native tcp/udp server and for naia seperately
    // Native registration
    net.register_message_channel_native(MESSAGE_SETTINGS, &MESSAGE_CHANNEL_ID).unwrap();
    // Naia registration
    net.set_channels_builder(|builder: &mut ConnectionChannelsBuilder| {
        builder
            .register::<String>(MESSAGE_SETTINGS)
            .unwrap();
    });

    // Finally, insert the network resource so it can be used by other systems
    commands.insert_resource(net);

}

// The following two functions are equivalent for both clients and servers, provided you've set up the NetworkResource properly

fn send(mut net: ResMut<NetworkResource>) {
    let message = String::from("Hello world");
    net.broadcast_message(&message, &MESSAGE_CHANNEL_ID).unwrap();

}

fn receive(mut net: ResMut<NetworkResource>) {
    let messages = net.view_messages::<String>(&MESSAGE_CHANNEL_ID).unwrap();

    for (_handle, message) in messages.iter() {
        println!("{}", message);

    }

}
```
## Warning about sending messages larger than 1095 bytes
@totalkrill noticed that when attempting to send a message larger than 1095 bytes, the message wouldn't go through. This is due to a limitation in WebRTC that doesn't allow messages larger than 1200 bytes (which 1095 bytes + any overhead in the WebRTC protocol and anything Naia adds would definetly do). The NetworkResource will return an error upon trying to send a message above that size when sending it through Naia, though there isn't an issue on the TCP/UDP front. Just be warned though, that if you're trying to have cross platform compatibility, messages should be under that size.


## If the crate isn't published on crates.io, how do I view the documentation?
Simply run `cargo doc` in the terminal. If you want to view the documentation for the web version of this crate, then run `cargo doc --no-default-features --features web`

## Why make this crate
While working with the wonderful bevy_networking_turbulence library, I realized that I wasn't able to create network clients on anything other than wasm. While it is doable using the fantastic new WebRTC-rs library (and I [even attempted it](https://github.com/naia-rs/naia-socket/pull/46), it still currently isn't possible. After a lot of effort, I eventually gave up, and decided instead to write this library. While initially, it was just for internal use in a project I'm workin on, I realized that it would have a lot of potential as a public library.
