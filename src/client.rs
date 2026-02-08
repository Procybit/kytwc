use futures_util::{
    SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use log::*;
use serde_json::Value;
use thiserror::Error;
use tokio::{
    net::TcpStream,
    select,
    sync::mpsc::{self, Receiver, error::SendError},
};
use tokio_tungstenite::{
    WebSocketStream,
    accept_async,
    tungstenite::{self, Message},
};

use crate::{
    protocol::Operation,
    server::*,
    status::{CloseCode, ErrorKind, send_status},
};

#[derive(Debug, Clone)]
pub struct Auth {
    pub project_id: String,
    pub user: String,
}

#[derive(Debug)]
pub enum ClientSignal {
    InformSet { name: String, value: Value },
    Close { kind: ErrorKind },
}

#[derive(Debug)]
pub enum Origin {
    Server,
    Client,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("websocket error: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("stream is finished")]
    StreamFinished,
    #[error("server is dead, can't send: {0}")]
    DeadServerTx(#[from] SendError<ServerSignal>),
    #[error("server is dead, can't recieve")]
    DeadServerRx,
    #[error("protocol violation")]
    ProtocolViolation,
    #[error("server closed connection, possible protocol violation")]
    Closed(Origin),
    #[error("server refused to register client")]
    CantRegister,
}

#[derive(Debug)]
pub struct Client {
    pub id: u64,
    rx: Receiver<ClientSignal>,
    server: ServerHandle,
    ws_sender: SplitSink<WebSocketStream<TcpStream>, Message>,
    ws_receiver: SplitStream<WebSocketStream<TcpStream>>,
}

impl Client {
    pub async fn try_handshake(
        mut server: ServerHandle,
        tcp_stream: TcpStream,
        mpsc_channel_buffer: usize,
    ) -> Result<Self, ClientError> {
        // accept as websocket; close if error
        let ws_stream = accept_async(tcp_stream).await?;

        // server -> client communication
        let (tx, rx) = mpsc::channel(mpsc_channel_buffer);

        // scratch cloud handshake
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let msg = ws_receiver.next().await;
        match msg {
            Some(msg) => {
                let msg = msg?;

                // if compliant handshake
                if msg.is_text()
                    && let Ok(Operation::Handshake { project_id, user }) =
                        serde_json::de::from_str(msg.to_text().unwrap())
                {
                    // register in server
                    let auth = Auth { project_id, user };
                    let Some((id, inform)) = server.register(tx.clone(), auth.clone()).await else {
                        return Err(ClientError::CantRegister);
                    };
                    for (name, value) in inform {
                        Self::inform(&mut ws_sender, name, value).await?;
                    }
                    info!("Client {id} {auth:?} connected");
                    Ok(Self {
                        id,
                        rx,
                        server,
                        ws_sender,
                        ws_receiver,
                    })
                } else {
                    send_status(&mut ws_sender, ErrorKind::Generic).await;
                    Err(ClientError::ProtocolViolation)
                }
            }
            None => Err(ClientError::StreamFinished),
        }
    }

    pub async fn run(mut self) -> Result<(), ClientError> {
        // scratch cloud operations loop
        loop {
            select! {
                msg = self.ws_receiver.next() => {
                    match msg {
                        Some(msg) => {
                            let msg = msg?;
                            let res = self.process_message(msg).await;
                            if let Err(ref e) = res {
                                if let ClientError::Closed(Origin::Client) = e {
                                    send_status(&mut self.ws_sender, ErrorKind::Normal(CloseCode::Normal)).await;
                                    break Ok(());
                                } else {
                                    send_status(&mut self.ws_sender, ErrorKind::Generic).await;
                                    break res;
                                }
                            }
                        },
                        None => {return Err(ClientError::StreamFinished)}
                    }
                }
                signal = self.rx.recv() => {
                    match signal {
                        None => break Err(ClientError::DeadServerRx),
                        Some(signal) => match signal {
                            ClientSignal::InformSet { name, value } => {
                                Self::inform(&mut self.ws_sender, name, value).await?;
                            }
                            ClientSignal::Close {kind} => {
                                send_status(&mut self.ws_sender, kind).await;
                                self.server.unregister(self.id).await;
                                break Err(ClientError::Closed(Origin::Server));
                            }
                        }
                    }
                }
            }
        }
    }

    async fn inform(
        ws_sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
        name: String,
        value: Value,
    ) -> Result<(), ClientError> {
        let msg = serde_json::ser::to_string(&Operation::Set { name, value }).unwrap();
        Ok(ws_sender.send(Message::Text(msg.into())).await?)
    }

    async fn process_message(&mut self, msg: Message) -> Result<(), ClientError> {
        match msg {
            Message::Text(bytes) => {
                let msg = bytes.as_str();

                match serde_json::de::from_str(msg) {
                    Ok(Operation::Set { name, value }) | Ok(Operation::Create { name, value }) => {
                        self.server
                            .tx
                            .send(ServerSignal::Method(
                                self.id,
                                ServerMethod::Set { name, value },
                            ))
                            .await?
                    }
                    Ok(Operation::Rename { name, new_name }) => {
                        self.server
                            .tx
                            .send(ServerSignal::Method(
                                self.id,
                                ServerMethod::Rename { name, new_name },
                            ))
                            .await?
                    }
                    Ok(Operation::Delete { name }) => {
                        self.server
                            .tx
                            .send(ServerSignal::Method(self.id, ServerMethod::Delete { name }))
                            .await?
                    }
                    _ => return Err(ClientError::ProtocolViolation),
                };
                Ok(())
            }
            Message::Close(_) => {
                info!("Client {} closed connection", self.id);
                self.server.tx.send(ServerSignal::Close(self.id)).await?;
                self.server.unregister(self.id).await;
                Err(ClientError::Closed(Origin::Client))
            }
            _ => Err(ClientError::ProtocolViolation),
        }
    }
}
