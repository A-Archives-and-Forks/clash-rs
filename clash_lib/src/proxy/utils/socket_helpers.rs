use std::{io, net::SocketAddr, ops::Deref, sync::Arc, time::Duration};

use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use socket2::TcpKeepalive;
use tokio::{
    net::{TcpSocket, TcpStream, UdpSocket},
    time::timeout,
};

use tracing::{debug, error, trace};

use super::{platform::must_bind_socket_on_interface, Interface};

pub fn apply_tcp_options(s: TcpStream) -> std::io::Result<TcpStream> {
    #[cfg(not(target_os = "windows"))]
    {
        let s = socket2::Socket::from(s.into_std()?);
        s.set_tcp_keepalive(
            &TcpKeepalive::new()
                .with_time(Duration::from_secs(10))
                .with_interval(Duration::from_secs(1))
                .with_retries(3),
        )?;
        TcpStream::from_std(s.into())
    }
    #[cfg(target_os = "windows")]
    {
        let s = socket2::Socket::from(s.into_std()?);
        s.set_tcp_keepalive(
            &TcpKeepalive::new()
                .with_time(Duration::from_secs(10))
                .with_interval(Duration::from_secs(1)),
        )?;
        TcpStream::from_std(s.into())
    }
}

pub async fn new_tcp_stream(
    endpoint: SocketAddr,
    iface: Option<Interface>,
    #[cfg(target_os = "linux")] so_mark: Option<u32>,
) -> io::Result<TcpStream> {
    let (socket, family) = match endpoint {
        SocketAddr::V4(_) => (
            socket2::Socket::new(
                socket2::Domain::IPV4,
                socket2::Type::STREAM,
                None,
            )?,
            socket2::Domain::IPV4,
        ),
        SocketAddr::V6(_) => (
            socket2::Socket::new(
                socket2::Domain::IPV6,
                socket2::Type::STREAM,
                None,
            )?,
            socket2::Domain::IPV6,
        ),
    };

    #[cfg(not(target_os = "android"))]
    if let Some(iface) = iface {
        debug!("binding tcp socket to interface: {:?}", iface);
        must_bind_socket_on_interface(&socket, &iface, family)?;
    }
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        trace!("protecting socket fd: {}", socket.as_raw_fd());
        protect_socket(socket.as_raw_fd()).expect("empty socket protector");
    }

    #[cfg(target_os = "linux")]
    if let Some(so_mark) = so_mark {
        socket.set_mark(so_mark)?;
    }

    socket.set_keepalive(true)?;
    socket.set_nodelay(true)?;
    socket.set_nonblocking(true)?;

    timeout(
        Duration::from_secs(10),
        TcpSocket::from_std_stream(socket.into()).connect(endpoint),
    )
    .await?
}

pub async fn new_udp_socket(
    src: Option<SocketAddr>,
    iface: Option<Interface>,
    #[cfg(target_os = "linux")] so_mark: Option<u32>,
) -> io::Result<UdpSocket> {
    let (socket, family) = match src {
        Some(src) => {
            if src.is_ipv4() {
                (
                    socket2::Socket::new(
                        socket2::Domain::IPV4,
                        socket2::Type::DGRAM,
                        None,
                    )?,
                    socket2::Domain::IPV4,
                )
            } else {
                (
                    socket2::Socket::new(
                        socket2::Domain::IPV6,
                        socket2::Type::DGRAM,
                        None,
                    )?,
                    socket2::Domain::IPV6,
                )
            }
        }
        None => (
            socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None)?,
            socket2::Domain::IPV4,
        ),
    };
    #[cfg(not(target_os = "android"))]
    match (src, iface) {
        (Some(_), Some(iface)) => {
            debug!("both src and iface are set, iface will be used: {:?}", src);
            must_bind_socket_on_interface(&socket, &iface, family).inspect_err(
                |x| {
                    error!("failed to bind socket to interface: {}", x);
                },
            )?;
        }
        (Some(src), None) => {
            debug!("binding socket to: {:?}", src);
            socket.bind(&src.into())?;
        }
        (None, Some(iface)) => {
            debug!("binding udp socket to interface: {:?}", iface);
            must_bind_socket_on_interface(&socket, &iface, family).inspect_err(
                |x| {
                    error!("failed to bind socket to interface: {}", x);
                },
            )?;
        }
        (None, None) => {
            debug!("not binding socket to any address or interface");
        }
    }
    #[cfg(target_os = "android")]
    {
        use std::os::fd::AsRawFd;
        trace!("protecting socket fd: {}", socket.as_raw_fd());
        protect_socket(socket.as_raw_fd()).expect("empty socket protector");
    }
    #[cfg(target_os = "linux")]
    if let Some(so_mark) = so_mark {
        socket.set_mark(so_mark)?;
    }


    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;

    UdpSocket::from_std(socket.into())
}

pub trait SocketProtector: Send + Sync {
    fn protect(&self, fd: i32);
}

static SOCKET_PROTECTOR: Lazy<ArcSwap<Option<Arc<dyn SocketProtector>>>> =
    Lazy::new(|| ArcSwap::from_pointee(None));

pub fn set_socket_protector(protector: Arc<dyn SocketProtector>) {
    SOCKET_PROTECTOR.store(Arc::new(Some(protector)));
}

pub fn protect_socket(fd: i32) -> anyhow::Result<()> {
    let guard = SOCKET_PROTECTOR.load();
    let protector = match guard.deref().deref() {
        Some(f) => f.clone(),
        None => return Err(anyhow!("Socket protector not set but invoked!")),
    };
    drop(guard);
    protector.protect(fd);
    Ok(())
}
