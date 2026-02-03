#![feature(hash_set_entry)]

mod client;
mod protocol;
mod server;
mod status;
mod utils;

use futures_util::FutureExt;
use log::*;
use tokio::{net::TcpListener, select};

use crate::{client::Client, server::*};

#[tokio::main]
async fn main() {
    colog::init();

    // TODO: TLS
    // TODO: configuration (RON file)
    let (cloud_server, server_handle) = Server::new();
    tokio::spawn(cloud_server.serve());
    let tcp_listener = TcpListener::bind("0.0.0.0:9001").await.unwrap();

    // for incoming tcp connections
    loop {
        select! {
            Ok((stream, peer)) = tcp_listener.accept() => {
                info!("Inbound connection: {peer}");
                tokio::spawn(
                    Client::try_handshake(server_handle.clone(), stream)
                    .then(async |client_res| {
                        match client_res {
                            Ok(client) => {
                                let id = client.id;
                                if let Err(e) = client.run().await {
                                    error!("Client {}: {e}", id);
                                }
                            }
                            Err(e) => warn!("Handshake error: {e}")
                        }
                    })
                );
            }
            // FIXME: shutdown if server is dead, check with select! branch
        }
    }
}
