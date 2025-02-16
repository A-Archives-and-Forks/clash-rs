use std::{
    io,
    net::{SocketAddrV4, SocketAddrV6},
};

use crate::proxy::utils::Interface;

pub(crate) fn must_bind_socket_on_interface(
    socket: &socket2::Socket,
    iface: &Interface,
    family: socket2::Domain,
) -> io::Result<()> {
    match iface {
        Interface::IpAddr(v4, v6) => match family {
            socket2::Domain::IPV4 => {
                let addr = v4.ok_or(io::ErrorKind::AddrNotAvailable)?;

                socket.bind(&SocketAddrV4::new(addr, 0).into())
            }
            socket2::Domain::IPV6 => {
                let addr = v6.ok_or(io::ErrorKind::AddrNotAvailable)?;
                socket.bind(&SocketAddrV6::new(addr, 0, 0, 0).into())
            }
            _ => unreachable!(),
        },
        Interface::Name(name) => {
            #[cfg(any(
                target_os = "android",
                target_os = "fuchsia",
                target_os = "linux",
            ))]
            {
                socket.bind_device(Some(name.as_bytes()))
            }
            #[cfg(not(any(
                target_os = "android",
                target_os = "fuchsia",
                target_os = "linux",
            )))]
            {
                use crate::common::errors::new_io_error;
                Err(new_io_error(format!("unsupported platform: {}", name)))
            }
        }
    }
}
