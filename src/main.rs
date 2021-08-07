mod frontend;
use std::collections::HashMap;

use std::io::{self, Stdout};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Context;
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent};
use mqttrs::{decode_slice, encode_slice, Connack, Packet, Publish};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tui::backend::CrosstermBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph};
use tui::Terminal;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let listener = TcpListener::bind(args.address()).await?;

    let state: HashMap<SocketAddr, Session> = HashMap::new();
    let state_ref = Arc::new(Mutex::new(state));

    let state = state_ref.clone();
    tokio::spawn(async move {
        let stdout = io::stdout();
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).unwrap();
        frontend::tui_worker(state, terminal).await;
    });

    let state = state_ref.clone();
    loop {
        let (mut socket, addr) = listener.accept().await?;

        // let mut session = state.lock().await;
        let (buf_tx, buf_rx) = channel::<[u8; 1024]>(1);
        let (msg_tx, msg_rx) = channel::<Msg>(1);
        tokio::spawn(async move {
            client_worker(&mut socket, msg_tx, buf_rx).await;
        });
        // session.insert(addr, Session::new(addr, tx));

        let state = state.clone();
        println!("found conn");
    }
}

async fn reply<'a>(socket: &mut tokio::net::TcpStream, packet: &Packet<'a>, buf: &mut [u8; 1024]) {
    match encode_slice(packet, buf) {
        Ok(size) => {
            socket.write(&buf[..size]).await.unwrap();
        }
        Err(error) => println!("{}", error),
    }
}

enum Msg<'a> {
    Packet(Packet<'a>),
}

async fn client_worker<'a>(
    socket: &mut TcpStream,
    mut tx: Sender<Msg<'a>>,
    mut rx: Receiver<([u8; 1024])>,
) {
    let mut buf = [0u8; 1024];

    loop {
        if let Ok(buffer) = rx.try_recv(cx) {
            socket.write(buffer);    
        }
        socket.read(&mut buf).await.unwrap();
        match decode_slice(&buf) {
            Ok(Some(packet)) => match packet {
                p @ Packet::Connect(_) => {
                    tx.send(Msg::Packet(p));
                    let connack = Packet::Connack(Connack {
                        session_present: false,
                        code: mqttrs::ConnectReturnCode::Accepted,
                    });
                    reply(socket, &connack, &mut buf).await;
                }
                Packet::Pingreq => {
                    reply(socket, &Packet::Pingresp, &mut buf).await;
                }
                Packet::Connack(_) => todo!(),
                Packet::Publish(_) => todo!(),
                Packet::Puback(_) => todo!(),
                Packet::Pubrec(_) => todo!(),
                Packet::Pubrel(_) => todo!(),
                Packet::Pubcomp(_) => todo!(),
                Packet::Subscribe(_) => todo!(),
                Packet::Suback(_) => todo!(),
                Packet::Unsubscribe(_) => todo!(),
                Packet::Unsuback(_) => todo!(),
                Packet::Pingreq => todo!(),
                Packet::Pingresp => todo!(),
                Packet::Disconnect => todo!(),
            },
            Ok(None) => {}
            Err(decode_err) => {
                dbg!(decode_err);
            }
        }
    }

    //     loop {
    //         rd.read(&mut buf).await.unwrap();
    //         match decode_slice(&buf) {
    //             Ok(Some(pkt)) => match pkt {
    //                 mqttrs::Packet::Connect(pkt) => {
    //                     let connack = Packet::Connack(Connack {
    //                         session_present: false,
    //                         code: mqttrs::ConnectReturnCode::Accepted,
    //                     });
    //                     reply(&mut socket, &connack, &mut buf).await;
    //                 }
    //                 Packet::Publish(pkt) => {
    //                     // println!(
    //                     //     "[{}]: {} [{:?}]",
    //                     //     pkt.topic_name,
    //                     //     String::from_utf8_lossy(&pkt.payload),
    //                     //     pkt.payload,
    //                     // )
    //                 }
    //                 Packet::Pingreq => {
    //                     reply(&mut socket, &Packet::Pingresp, &mut buf).await;
    //                 }
    //                 unknown => {
    //                     println!("Not handled: {:?}", unknown);
    //                 }
    //             },
    //             Ok(None) => {
    //                 println!("Decode error");
    //             }
    //             Err(error) => {
    //                 dbg!(error);
    //             }
    //         }
    //     }
}
