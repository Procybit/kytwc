use futures_util::{SinkExt, stream::SplitSink};
use tokio::net::TcpStream;
pub use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::CloseFrame},
};

#[derive(Debug)]
pub enum ErrorKind {
    Generic,
    Username,
    Overloaded,
    Unavailable,
    Security,
    Identity,
    Normal(CloseCode),
}

pub async fn send_status(
    ws_sender: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    kind: ErrorKind,
) {
    let code = match kind {
        ErrorKind::Generic => 4000,
        ErrorKind::Username => 4002,
        ErrorKind::Overloaded => 4003,
        ErrorKind::Unavailable => 4004,
        ErrorKind::Security => 4005,
        ErrorKind::Identity => 4006,
        ErrorKind::Normal(c) => c.into(),
    };
    let reason = match kind {
        ErrorKind::Generic => "Protocol violation",
        ErrorKind::Username => "Bad username",
        ErrorKind::Overloaded => "Server full",
        ErrorKind::Unavailable => "Project unavailable",
        ErrorKind::Security => "Security violation",
        ErrorKind::Identity => "Identify yourself",
        ErrorKind::Normal(c) => &c.to_string(),
    };

    ws_sender
        .send(Message::Close(Some(CloseFrame {
            code: code.into(),
            reason: reason.into(),
        })))
        .await
        .ok();
    ws_sender.flush().await.ok();
}
