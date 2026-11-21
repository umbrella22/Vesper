use std::io;
use std::net::{SocketAddr, TcpStream, UdpSocket};

use crate::BridgeError as Error;

pub(crate) struct Pair {
    pub rtp: UdpSocket,
    pub rtcp: UdpSocket,
    pub client_port: u16,
}

impl Pair {
    pub fn bind(control: &TcpStream) -> Result<Self, Error> {
        let local = control.local_addr().map_err(|_| Error::Unavailable)?;
        for _ in 0..24 {
            let rtp =
                UdpSocket::bind(SocketAddr::new(local.ip(), 0)).map_err(|_| Error::Unavailable)?;
            let port = rtp.local_addr().map_err(|_| Error::Unavailable)?.port();
            if port % 2 != 0 {
                continue;
            }
            let Ok(rtcp) = UdpSocket::bind(SocketAddr::new(local.ip(), port + 1)) else {
                continue;
            };
            rtp.set_nonblocking(true).map_err(|_| Error::Unavailable)?;
            rtcp.set_nonblocking(true).map_err(|_| Error::Unavailable)?;
            return Ok(Self {
                rtp,
                rtcp,
                client_port: port,
            });
        }
        Err(Error::Unavailable)
    }

    pub fn connect(&self, control: &TcpStream, header: &str) -> Result<(), Error> {
        let peer = control.peer_addr().map_err(|_| Error::Unavailable)?.ip();
        let mut fields = header.split(';').map(str::trim);
        if !fields.next().is_some_and(|protocol| {
            protocol.eq_ignore_ascii_case("RTP/AVP") || protocol.eq_ignore_ascii_case("RTP/AVP/UDP")
        }) {
            return Err(Error::UnsupportedTransport);
        }
        let mut ports = None;
        for field in fields {
            if field.eq_ignore_ascii_case("multicast") {
                return Err(Error::UnsupportedTransport);
            }
            if let Some((name, value)) = field.split_once('=') {
                match name.to_ascii_lowercase().as_str() {
                    "server_port" if ports.is_none() => ports = Some(parse_ports(value)?),
                    "server_port" => return Err(Error::InvalidResponse),
                    "client_port"
                        if parse_ports(value)? != (self.client_port, self.client_port + 1) =>
                    {
                        return Err(Error::InvalidResponse);
                    }
                    "source" if value.parse().ok() != Some(peer) => {
                        return Err(Error::UnsupportedTransport);
                    }
                    _ => (),
                }
            }
        }
        let (rtp, rtcp) = ports.ok_or(Error::UnsupportedTransport)?;
        self.rtp
            .connect(SocketAddr::new(peer, rtp))
            .map_err(|_| Error::Unavailable)?;
        self.rtcp
            .connect(SocketAddr::new(peer, rtcp))
            .map_err(|_| Error::Unavailable)?;
        Ok(())
    }
}

fn parse_ports(value: &str) -> Result<(u16, u16), Error> {
    let (rtp, rtcp) = value.split_once('-').ok_or(Error::InvalidResponse)?;
    let rtp = rtp.parse::<u16>().map_err(|_| Error::InvalidResponse)?;
    let rtcp = rtcp.parse::<u16>().map_err(|_| Error::InvalidResponse)?;
    if rtp == 0 || rtcp == 0 || rtp == rtcp {
        return Err(Error::InvalidResponse);
    }
    Ok((rtp, rtcp))
}

pub(crate) fn receive(socket: &UdpSocket, buffer: &mut [u8]) -> Result<Option<usize>, Error> {
    match socket.recv(buffer) {
        Ok(length) if length > 0 && length < buffer.len() => Ok(Some(length)),
        Ok(_) => Ok(None),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(None)
        }
        Err(_) => Err(Error::Unavailable),
    }
}
