// Simple blocking TCP networking API.
//
// Each open socket is identified by a small integer socket id. Sockets are
// either listeners, created by net_listen and consumed by net_accept, or
// streams, created by net_accept/net_connect and used by net_read/net_write.
//
// The API is blocking: net_accept waits for a connection and net_read waits
// for data. Programs handle multiple connections by spawning one actor per
// socket. This subsystem deliberately spawns no threads of its own.
//
// Errors are reported through return values, since a host error would kill
// the program and a lost connection is routine. A socket id that is unknown
// or already closed is treated as a connection that is over rather than as a
// mistake: closing a socket from another actor while one is blocked on it is
// a normal pattern, not a bug. Ids are unsigned, so a negative one is a type
// error and does end the program.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::sleep;
use std::time::Duration;
use crate::vm::Actor;
use crate::value::*;
use crate::str::Str;
use crate::host::HostResult;
use crate::*;

/// How often net_accept wakes up to check whether its listening socket has
/// been closed. This bounds how quickly a blocked accept notices a net_close
const ACCEPT_POLL_MS: u64 = 20;

/// An open socket, either listening or connected.
///
/// Handles are held behind an Arc so a blocking operation can clone the handle,
/// release the socket table lock, and then block without holding it. That keeps
/// one blocked socket from stalling operations on other sockets, and lets
/// net_close shut a stream down to wake an actor blocked in net_read
enum Socket
{
    Listener {
        listener: Arc<TcpListener>,
        local_addr: String,
    },

    // Addresses are captured when the socket is created. Asking the OS for
    // them later fails once the peer resets, which is when a program most
    // wants them for a log line
    Stream {
        stream: Arc<TcpStream>,
        peer_addr: String,
        local_addr: String,
    },
}

/// Global table of open sockets.
///
/// Sockets are shared between actors: one actor accepts and hands the socket
/// id to another that handles the connection, so the table cannot live on the
/// actor. Keeping it separate from the VM lock also avoids serializing socket
/// operations against unrelated VM activity
struct NetState
{
    // Next socket id to assign. Starts at 1 so that 0 is never a valid id
    next_id: u64,

    // Map of open sockets by id
    sockets: HashMap<u64, Socket>,
}

impl Default for NetState
{
    fn default() -> Self
    {
        Self {
            next_id: 1,
            sockets: HashMap::new(),
        }
    }
}

/// Get a handle on the global socket table, initializing it on first use
fn net_state() -> &'static Mutex<NetState>
{
    static NET_STATE: OnceLock<Mutex<NetState>> = OnceLock::new();
    NET_STATE.get_or_init(|| Mutex::new(NetState::default()))
}

/// Insert a socket into the table and return its freshly assigned id
fn add_socket(socket: Socket) -> u64
{
    let mut state = net_state().lock().unwrap();
    let id = state.next_id;
    state.next_id += 1;
    state.sockets.insert(id, socket);
    id
}

/// Look up a listening socket, cloning out its handle.
/// Returns None if the id is unknown or refers to a stream
fn get_listener(socket_id: u64) -> Option<Arc<TcpListener>>
{
    let state = net_state().lock().unwrap();
    match state.sockets.get(&socket_id) {
        Some(Socket::Listener { listener, .. }) => Some(listener.clone()),
        _ => None,
    }
}

/// Look up a connected stream, cloning out its handle.
/// Returns None if the id is unknown or refers to a listening socket
fn get_stream(socket_id: u64) -> Option<Arc<TcpStream>>
{
    let state = net_state().lock().unwrap();
    match state.sockets.get(&socket_id) {
        Some(Socket::Stream { stream, .. }) => Some(stream.clone()),
        _ => None,
    }
}

/// Whether a listening socket with this id is still open.
/// net_accept polls this so it can bail out once net_close removes the socket
fn listener_present(socket_id: u64) -> bool
{
    let state = net_state().lock().unwrap();
    matches!(state.sockets.get(&socket_id), Some(Socket::Listener { .. }))
}

/// Allocate a Plush string for an address looked up in the socket table
fn addr_str(actor: &mut Actor, addr: Option<String>) -> HostResult
{
    let addr = match addr {
        Some(addr) => addr,
        None => return Ok(Value::NIL),
    };

    actor.gc_check(Str::alloc_size(addr.len()), &mut []);
    Ok(Str::new(&addr, &mut actor.alloc))
}

/// Open a listening socket bound to the given address, e.g. "127.0.0.1:8080".
/// Returns a socket id, or nil if the address could not be bound
/// $net_listen(addr)
pub fn net_listen(actor: &mut Actor, addr: Value) -> HostResult
{
    let addr = unwrap_str!(addr);

    let listener = match TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(_) => return Ok(Value::NIL),
    };

    // The listener is non-blocking so that net_accept can poll it and stay
    // cancelable. net_accept sets each accepted stream back to blocking
    if listener.set_nonblocking(true).is_err() {
        return Ok(Value::NIL);
    }

    // Read the bound address back, so that binding port 0 and then asking
    // $net_local_addr reports the port the OS picked
    let local_addr = match listener.local_addr() {
        Ok(addr) => addr.to_string(),
        Err(_) => return Ok(Value::NIL),
    };

    let id = add_socket(Socket::Listener {
        listener: Arc::new(listener),
        local_addr,
    });

    Ok(actor.int64(id as i64))
}

/// Connect to a remote address, e.g. "example.com:80".
/// Returns a socket id, or nil if the connection could not be established
/// $net_connect(addr)
pub fn net_connect(actor: &mut Actor, addr: Value) -> HostResult
{
    let addr = unwrap_str!(addr);

    let stream = match TcpStream::connect(addr) {
        Ok(stream) => stream,
        Err(_) => return Ok(Value::NIL),
    };

    let peer_addr = match stream.peer_addr() {
        Ok(addr) => addr.to_string(),
        Err(_) => return Ok(Value::NIL),
    };

    let local_addr = match stream.local_addr() {
        Ok(addr) => addr.to_string(),
        Err(_) => String::new(),
    };

    let id = add_socket(Socket::Stream {
        stream: Arc::new(stream),
        peer_addr,
        local_addr,
    });

    Ok(actor.int64(id as i64))
}

/// Block until a connection arrives on a listening socket.
/// Returns a socket id for the new connection, or nil if the listening
/// socket was closed or the accept failed
/// $net_accept(socket_id)
pub fn net_accept(actor: &mut Actor, socket_id: Value) -> HostResult
{
    let listen_id = unwrap_u64!(socket_id);

    let listener = match get_listener(listen_id) {
        Some(listener) => listener,
        None => return Ok(Value::NIL),
    };

    // There is no portable way to interrupt a blocking accept, so the listener
    // is non-blocking and we poll it. Between attempts we check whether the
    // listening socket has been closed, which lets another actor cancel this
    // accept. The table lock is never held while we wait
    let (stream, peer_addr) = loop {
        match listener.accept() {
            Ok(conn) => break conn,

            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                if !listener_present(listen_id) {
                    return Ok(Value::NIL);
                }
                sleep(Duration::from_millis(ACCEPT_POLL_MS));
            }

            Err(_) => return Ok(Value::NIL),
        }
    };

    // The accepted stream inherits the listener's non-blocking flag on some
    // platforms, so force it back to blocking
    if stream.set_nonblocking(false).is_err() {
        return Ok(Value::NIL);
    }

    let local_addr = match stream.local_addr() {
        Ok(addr) => addr.to_string(),
        Err(_) => String::new(),
    };

    let id = add_socket(Socket::Stream {
        stream: Arc::new(stream),
        peer_addr: peer_addr.to_string(),
        local_addr,
    });

    Ok(actor.int64(id as i64))
}

/// Get the address of the peer on a connected socket, as a string.
/// Returns nil if the socket id is not a live connection
/// $net_peer_addr(socket_id)
pub fn net_peer_addr(actor: &mut Actor, socket_id: Value) -> HostResult
{
    let socket_id = unwrap_u64!(socket_id);

    let addr = {
        let state = net_state().lock().unwrap();
        match state.sockets.get(&socket_id) {
            Some(Socket::Stream { peer_addr, .. }) => Some(peer_addr.clone()),
            _ => None,
        }
    };

    addr_str(actor, addr)
}

/// Get the local address a socket is bound to, as a string. Binding a
/// listener to port 0 and then asking for this reports the port assigned
/// by the OS. Returns nil if the socket id is unknown
/// $net_local_addr(socket_id)
pub fn net_local_addr(actor: &mut Actor, socket_id: Value) -> HostResult
{
    let socket_id = unwrap_u64!(socket_id);

    let addr = {
        let state = net_state().lock().unwrap();
        match state.sockets.get(&socket_id) {
            Some(Socket::Listener { local_addr, .. }) => Some(local_addr.clone()),
            Some(Socket::Stream { local_addr, .. }) => Some(local_addr.clone()),
            None => None,
        }
    };

    addr_str(actor, addr)
}

/// Read from a socket into a byte array, blocking until data is available or
/// the read timeout elapses. Returns the number of bytes read, 0 once the
/// connection is over, or nil if the read timed out
/// $net_read(socket_id, byte_array)
pub fn net_read(actor: &mut Actor, socket_id: Value, buf: Value) -> HostResult
{
    let socket_id = unwrap_u64!(socket_id);
    let buf = unwrap_ba!(buf);

    let stream = match get_stream(socket_id) {
        Some(stream) => stream,
        // Another actor may have closed this socket, which is not an error
        None => return Ok(actor.int64(0)),
    };

    // Read straight into the heap. Only this actor's own thread collects, and
    // it is blocked here for the duration, so nothing can move these bytes
    let num_bytes = buf.num_bytes();
    let slice: &mut [u8] = unsafe { buf.get_slice_mut(0, num_bytes) };

    // Read/Write are implemented for &TcpStream, so a shared handle suffices
    match (&*stream).read(slice) {
        // Ok(0) is the peer closing the connection, reported as 0 bytes
        Ok(num_read) => Ok(actor.int64(num_read as i64)),

        // A read timeout is WouldBlock on Unix and TimedOut on Windows
        Err(ref e) if e.kind() == ErrorKind::WouldBlock
                   || e.kind() == ErrorKind::TimedOut => Ok(Value::NIL),

        // A reset connection is over, same as an orderly close
        Err(_) => Ok(actor.int64(0)),
    }
}

/// Write the first num_bytes of a byte array to a socket, blocking until all
/// of it has been written. Returns num_bytes, or nil if the connection is
/// over. There are no partial writes, so callers never have to retry a
/// remainder the way POSIX write() asks them to
/// $net_write(socket_id, byte_array, num_bytes)
pub fn net_write(actor: &mut Actor, socket_id: Value, buf: Value, num_bytes: Value) -> HostResult
{
    let socket_id = unwrap_u64!(socket_id);
    let buf = unwrap_ba!(buf);
    let num_bytes = unwrap_usize!(num_bytes);

    if num_bytes > buf.num_bytes() {
        error!(
            "net_write asked to write {} bytes from a byte array of {} bytes",
            num_bytes,
            buf.num_bytes()
        );
    }

    let stream = match get_stream(socket_id) {
        Some(stream) => stream,
        None => return Ok(Value::NIL),
    };

    let slice: &[u8] = unsafe { buf.get_slice(0, num_bytes) };

    // write_all reports a socket that stopped accepting bytes as WriteZero,
    // so a short write can only reach us as an error
    match (&*stream).write_all(slice) {
        Ok(()) => Ok(actor.int64(num_bytes as i64)),
        Err(_) => Ok(Value::NIL),
    }
}

/// Shut down the writing half of a connection, sending the peer an EOF while
/// this end stays able to read. This is how to end a request that the peer
/// reads to EOF without giving up the response
/// $net_shutdown_write(socket_id)
pub fn net_shutdown_write(_actor: &mut Actor, socket_id: Value) -> HostResult
{
    let socket_id = unwrap_u64!(socket_id);

    let stream = match get_stream(socket_id) {
        Some(stream) => stream,
        None => return Ok(Value::NIL),
    };

    // Unlike net_close, the socket stays in the table, so reads keep working
    let _ = stream.shutdown(Shutdown::Write);

    Ok(Value::NIL)
}

/// Close a socket. Closing a listening socket cancels an actor blocked in
/// net_accept, and closing a connection wakes one blocked in net_read.
/// Closing an unknown or already closed socket does nothing
/// $net_close(socket_id)
pub fn net_close(_actor: &mut Actor, socket_id: Value) -> HostResult
{
    let socket_id = unwrap_u64!(socket_id);
    let socket = net_state().lock().unwrap().sockets.remove(&socket_id);

    // Shut the stream down after releasing the table lock, so that an actor
    // blocked in net_read wakes up without contending for it
    if let Some(Socket::Stream { stream, .. }) = socket {
        let _ = stream.shutdown(Shutdown::Both);
    }

    Ok(Value::NIL)
}

/// Set the read timeout on a connected socket, in milliseconds. A timeout of
/// 0 clears it, making subsequent reads block indefinitely. Writes are not
/// affected: they always run to completion
/// $net_set_timeout(socket_id, timeout_ms)
pub fn net_set_timeout(_actor: &mut Actor, socket_id: Value, timeout_ms: Value) -> HostResult
{
    let socket_id = unwrap_u64!(socket_id);
    let timeout_ms = unwrap_u64!(timeout_ms);

    let stream = match get_stream(socket_id) {
        Some(stream) => stream,
        None => return Ok(Value::NIL),
    };

    // A zero duration is rejected by the OS, so map 0 ms to "no timeout"
    let timeout = if timeout_ms == 0 {
        None
    } else {
        Some(Duration::from_millis(timeout_ms))
    };

    let _ = stream.set_read_timeout(timeout);

    Ok(Value::NIL)
}

#[cfg(test)]
mod tests
{
    use super::*;

    // Put one socket of each kind in the table, so that lookups have
    // something to tell apart. The ids are whatever the global table hands
    // out, since these tests run alongside each other
    fn test_sockets() -> (u64, u64)
    {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // Connecting without accepting leaves the connection in the accept
        // queue, which is all these tests need
        let stream = TcpStream::connect(addr).unwrap();

        let listen_id = add_socket(Socket::Listener {
            listener: Arc::new(listener),
            local_addr: addr.to_string(),
        });

        let stream_id = add_socket(Socket::Stream {
            peer_addr: stream.peer_addr().unwrap().to_string(),
            local_addr: stream.local_addr().unwrap().to_string(),
            stream: Arc::new(stream),
        });

        (listen_id, stream_id)
    }

    #[test]
    fn ids_are_distinct_and_never_zero()
    {
        let (listen_id, stream_id) = test_sockets();
        assert!(listen_id > 0);
        assert!(stream_id > 0);
        assert_ne!(listen_id, stream_id);
    }

    // A listener id passed to a stream operation, or the reverse, has to read
    // as absent rather than matching the wrong socket
    #[test]
    fn lookups_tell_the_two_kinds_apart()
    {
        let (listen_id, stream_id) = test_sockets();

        assert!(get_listener(listen_id).is_some());
        assert!(get_stream(listen_id).is_none());
        assert!(listener_present(listen_id));

        assert!(get_stream(stream_id).is_some());
        assert!(get_listener(stream_id).is_none());
        assert!(!listener_present(stream_id));
    }

    #[test]
    fn unknown_ids_are_absent()
    {
        assert!(get_listener(u64::MAX).is_none());
        assert!(get_stream(u64::MAX).is_none());
        assert!(!listener_present(u64::MAX));
    }

    // This is what cancels an accept: the poll loop stops once net_close has
    // taken the listener out of the table
    #[test]
    fn removing_a_listener_clears_listener_present()
    {
        let (listen_id, _) = test_sockets();
        assert!(listener_present(listen_id));

        net_state().lock().unwrap().sockets.remove(&listen_id);
        assert!(!listener_present(listen_id));
    }

    // Closing a stream shuts the connection down, which is what wakes an
    // actor blocked in net_read
    #[test]
    fn closing_a_stream_shuts_it_down()
    {
        let (_, stream_id) = test_sockets();

        let stream = get_stream(stream_id).unwrap();
        net_state().lock().unwrap().sockets.remove(&stream_id);
        assert!(stream.shutdown(Shutdown::Both).is_ok());

        // A shut down stream reads as EOF rather than blocking
        let mut buf = [0u8; 8];
        assert_eq!((&*stream).read(&mut buf).unwrap(), 0);
    }
}
