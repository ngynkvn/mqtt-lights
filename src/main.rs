#![feature(generic_associated_types)]
mod frontend;
use core::fmt::Debug;
use core::ops::Deref;
use mqttrs::Packet::Pingresp;




use std::net::SocketAddr;






use mqttrs::{Connack, Packet};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{Sender};







#[derive(Debug)]
pub struct Session {
    addr: SocketAddr,
    is_on: bool,
    tx: Sender<[u8; 1024]>,
}

impl Session {
    fn new(addr: SocketAddr, tx: Sender<[u8; 1024]>) -> Self {
        Session {
            addr,
            is_on: false,
            tx,
        }
    }
}

use clap::Clap;

#[derive(Clap, Debug)]
#[clap(name = "mqtt-lights")]
struct Args {
    /// External IP to host on
    external: String,

    /// Port to host on
    port: u32,

    /// Number of times to greet
    #[clap(short, long, default_value = "1")]
    count: u8,
}

impl Args {
    pub fn address(&self) -> String {
        format!("{}:{}", self.external, self.port)
    }
}

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

extern crate color_eyre;

use color_eyre::Result;

#[derive(Debug)]
pub struct Client {
    stream: TcpStream,
    addr: SocketAddr,
}

pub struct ClientMsg([u8; 1024], SocketAddr);

impl ClientMsg {
    fn new(buf: [u8; 1024], addr: SocketAddr) -> Self {
        Self(buf, addr)
    }
    fn read(&self) -> Option<Packet<'_>> {
        mqttrs::decode_slice(self.deref()).unwrap()
    }
}

impl Deref for ClientMsg {
    type Target = [u8; 1024];

    fn deref(&self) -> &<Self as std::ops::Deref>::Target {
        &self.0
    }
}

impl Debug for ClientMsg {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        write!(fmt, "[{}] {:?}", self.1, self.read())
    }
}




pub struct ClientConnection {
    socket: TcpStream,
    addr: SocketAddr,
    buf: [u8; 1024],
}



impl ClientConnection {
    async fn read(&mut self) -> ClientMsg {
        self.socket.read(&mut self.buf).await.unwrap();
        if matches!(mqttrs::decode_slice(&self.buf), Ok(Some(_))) {
            let mut buf = [0; 1024];
            buf.copy_from_slice(&self.buf);
            ClientMsg::new(buf, self.addr)
        } else {
            panic!("?");
        }
    }
    async fn reply(&mut self, packet: &Packet<'_>) {
        let mut buf = [0; 1024];
        match mqttrs::encode_slice(packet, &mut buf) {
            Ok(size) => {
                self.socket.write(&buf[..size]).await.unwrap();
            }
            Err(error) => println!("{}", error),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    color_eyre::install()?;
    pretty_env_logger::init();
    let args = Args::parse();

    let listener = TcpListener::bind(args.address()).await?;
    debug!("Server listening.");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn(async move {
        loop {
            let (socket, addr) = listener.accept().await.unwrap();
            // let tx = tx.clone();
            let mut client = ClientConnection {
                socket,
                addr,
                buf: [0; 1024],
            };
            debug!("Spawned a client listener for {:?}", client.addr);
            let tx = tx.clone();
            tokio::task::spawn(async move {
                loop {
                    let ClientMsg(buf, addr) = client.read().await;
                    tx.send(ClientMsg::new(buf, addr)).unwrap();
                    match mqttrs::decode_slice(&buf) {
                        Ok(Some(packet)) => match packet {
                            Packet::Connect(_) => {
                                let connack = Packet::Connack(Connack {
                                    session_present: false,
                                    code: mqttrs::ConnectReturnCode::Accepted,
                                });
                                client.reply(&connack).await;
                            }
                            Packet::Pingreq => {
                                println!("Pong");
                                client.reply(&Pingresp).await;
                            }
                            Packet::Connack(_) => todo!(),
                            Packet::Publish(_) => {}
                            Packet::Puback(_) => todo!(),
                            Packet::Pubrec(_) => todo!(),
                            Packet::Pubrel(_) => todo!(),
                            Packet::Pubcomp(_) => todo!(),
                            Packet::Subscribe(_) => todo!(),
                            Packet::Suback(_) => todo!(),
                            Packet::Unsubscribe(_) => todo!(),
                            Packet::Unsuback(_) => todo!(),
                            Packet::Pingresp => todo!(),
                            Packet::Disconnect => todo!(),
                        },
                        Ok(None) => {}
                        Err(decode_err) => {
                            dbg!(decode_err);
                        }
                    }
                }
            });
        }
    });
    debug!("Starting tui worker.");
    while let Some(pkt) = rx.recv().await {
        info!("{:?}", pkt);
    }
    Ok(())
}
