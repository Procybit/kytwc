pub mod memory;

use std::{collections::HashMap, sync::Arc};

use log::*;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{
    Mutex,
    mpsc::{self, Receiver, Sender},
};

use crate::{
    client::*,
    server::memory::{Memory, Room},
    status::ErrorKind,
    utils::new_id,
};

pub type RegistryMap = HashMap<u64, (Sender<ClientSignal>, Auth)>;
pub type Registry = Arc<Mutex<RegistryMap>>;

#[derive(Debug, Clone)]
pub struct ServerHandle {
    pub tx: Sender<ServerSignal>,
    pub registry: Registry,
    memory: Arc<Mutex<Memory>>,
}

impl ServerHandle {
    pub async fn register(
        &mut self,
        tx: Sender<ClientSignal>,
        auth: Auth,
    ) -> (u64, HashMap<String, Value>) {
        let id = new_id();
        self.registry.lock().await.insert(id, (tx, auth.clone()));
        let memory = &mut self.memory.lock().await;
        let inform = if let Some(room) = memory.rooms.get(&auth.project_id) {
            room.variables.clone()
        } else {
            memory.rooms.insert(auth.project_id.clone(), Room::new());
            HashMap::new()
        };
        (id, inform)
    }

    pub async fn unregister(&mut self, id: u64) {
        self.registry.lock().await.remove(&id);
    }

    pub async fn unregister_consuming(self, id: u64) {
        self.registry.lock().await.remove(&id);
    }
}

#[derive(Debug)]
pub enum ServerMethod {
    Set { name: String, value: Value },
    Rename { name: String, new_name: String },
    Delete { name: String },
}

#[derive(Debug)]
pub enum ServerSignal {
    Stop,
    Method(u64, ServerMethod),
    Close(u64),
}

#[derive(Debug, Error)]
pub enum ServerError {}

#[derive(Debug)]
pub struct Server {
    rx: Receiver<ServerSignal>,
    handle: ServerHandle,
}

impl Server {
    pub fn new() -> (Server, ServerHandle) {
        let (tx, rx) = mpsc::channel(100);
        let memory = Arc::new(Mutex::new(Memory::new()));
        let registry = Arc::new(Mutex::new(HashMap::new()));

        let handle = ServerHandle {
            tx,
            memory,
            registry: Arc::clone(&registry),
        };

        let server = Server {
            rx,
            handle: handle.clone(),
        };

        (server, handle)
    }

    async fn send_client_signal(
        &mut self,
        id: u64,
        auth: &Auth,
        tx: &Sender<ClientSignal>,
        signal: ClientSignal,
    ) {
        if let Err(e) = tx.send(signal).await {
            let handle = self.handle.clone();
            tokio::spawn(handle.unregister_consuming(id));
            error!("Client {id} {auth:?} channel lost: {e}")
        }
    }

    pub async fn serve(mut self) -> Result<(), ServerError> {
        loop {
            match self.rx.recv().await {
                None | Some(ServerSignal::Stop) => break,
                Some(ServerSignal::Method(id, method)) => {
                    // NOTE: panic! on PoisonError
                    let (tx, auth) = {
                        let locked_registry = self.handle.registry.lock().await;
                        let Some((tx, auth)) = locked_registry.get(&id) else {
                            error!("Got signal from Client {id} that is not registered");
                            continue;
                        };
                        (tx.clone(), auth.clone())
                    };

                    let mut locked_memory = self.handle.memory.lock().await;
                    let Some(room) = locked_memory.rooms.get_mut(&auth.project_id) else {
                        error!("Client {id} {auth:?} is in non-existent Room");
                        continue;
                    };

                    // TODO: policy checks

                    // perform methods
                    match method {
                        ServerMethod::Set { name, value } => {
                            room.variables.insert(name.clone(), value.clone());

                            let peers: Vec<(u64, (Sender<ClientSignal>, Auth))> = {
                                let registry = self.handle.registry.lock().await;
                                registry
                                    .iter()
                                    .filter(|(other_id, _)| **other_id != id)
                                    .map(|(&id, (tx, auth))| (id, (tx.clone(), auth.clone())))
                                    .collect()
                            };

                            // notify clients
                            drop(locked_memory);
                            for (other_id, (tx, auth)) in peers.iter() {
                                self.send_client_signal(
                                    *other_id,
                                    auth,
                                    tx,
                                    ClientSignal::InformSet {
                                        name: name.clone(),
                                        value: value.clone(),
                                    },
                                )
                                .await;
                            }
                        }
                        ServerMethod::Rename {
                            name: old_name,
                            new_name,
                        } => {
                            let value = room.variables.remove(&old_name);

                            match value {
                                Some(v) => {
                                    room.variables.insert(new_name, v);
                                }
                                None => {
                                    drop(locked_memory);
                                    self.send_client_signal(
                                        id,
                                        &auth,
                                        &tx,
                                        ClientSignal::Close {
                                            kind: ErrorKind::Generic,
                                        },
                                    )
                                    .await;
                                }
                            }
                        }
                        ServerMethod::Delete { name } => {
                            if room.variables.remove(&name).is_none() {
                                drop(locked_memory);
                                self.send_client_signal(
                                    id,
                                    &auth,
                                    &tx,
                                    ClientSignal::Close {
                                        kind: ErrorKind::Generic,
                                    },
                                )
                                .await;
                            };
                        }
                    }
                }
                Some(ServerSignal::Close(id)) => {
                    self.handle.unregister(id).await;
                }
            }
        }
        Ok(())
    }
}
