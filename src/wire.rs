//! Bound local-socket service for the authority contract.

use std::io;
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};

use signal_frame::{
    LaneSequence, NonEmpty, OperationFailureReason, Reply, StreamEventIdentifier,
    StreamingFrameBody, SubReply, SubscriptionTokenInner,
};
use signal_sema_translator::{AuthorityReply, NoWriteFailure, PostCommitEvent, TranslatorFrame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::{AUTHORITY_ROUTE, Runtime, principal_for_unix_uid, store::Error};

const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// One bound socket node held for the complete authority-listener lifetime.
///
/// Dropping the owner removes the path only when it still names the exact
/// socket inode created by [`bind`].
pub struct BoundUnixListener {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl BoundUnixListener {
    async fn accept(&self) -> io::Result<(UnixStream, tokio::net::unix::SocketAddr)> {
        self.listener.accept().await
    }
}

impl Drop for BoundUnixListener {
    fn drop(&mut self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Serve the configured local socket until the listener fails or the task is
/// cancelled.
pub async fn serve(socket: &Path, runtime: Runtime) -> Result<(), Error> {
    let listener = bind(socket)?;
    serve_listener(listener, runtime).await
}

/// Bind the configured socket after safely removing only a stale socket node.
pub fn bind(socket: &Path) -> Result<BoundUnixListener, Error> {
    prepare_socket(socket).map_err(|error| Error::Actor(error.to_string()))?;
    let listener = UnixListener::bind(socket).map_err(|error| Error::Actor(error.to_string()))?;
    let metadata =
        std::fs::symlink_metadata(socket).map_err(|error| Error::Actor(error.to_string()))?;
    Ok(BoundUnixListener {
        listener,
        path: socket.to_path_buf(),
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

/// Serve an already-bound listener.
pub async fn serve_listener(listener: BoundUnixListener, runtime: Runtime) -> Result<(), Error> {
    loop {
        let (stream, _) = listener
            .accept()
            .await
            .map_err(|error| Error::Actor(error.to_string()))?;
        let runtime = runtime.clone();
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, runtime).await {
                eprintln!("sema-translator connection refused: {error}");
            }
        });
    }
}

/// Serve exactly one bound authority request on one accepted connection.
///
/// `TranslatorFrame::decode_length_prefixed` validates the contract and wire
/// revision in the fixed header before rkyv body validation or request
/// dispatch. A wrong or legacy header closes the connection without a write.
pub async fn serve_connection(mut stream: UnixStream, runtime: Runtime) -> Result<(), Error> {
    let authenticated_principal = principal_for_unix_uid(
        stream
            .peer_cred()
            .map_err(|error| Error::Actor(error.to_string()))?
            .uid(),
    );
    let bytes = read_frame(&mut stream)
        .await
        .map_err(|error| Error::Actor(error.to_string()))?;
    let frame = TranslatorFrame::decode_length_prefixed(&bytes)
        .map_err(|error| Error::Actor(error.to_string()))?;
    let route = frame.short_header().route();
    if route != AUTHORITY_ROUTE {
        return Err(Error::Actor(
            "bound frame used an unknown authority route".into(),
        ));
    }
    let StreamingFrameBody::Request { exchange, request } = frame.into_body() else {
        return Err(Error::Actor(
            "first bound frame body was not an authority request".into(),
        ));
    };

    if !request.payloads.tail().is_empty() {
        let frame = TranslatorFrame::new(
            route,
            StreamingFrameBody::Reply {
                exchange,
                reply: Reply::rejected(signal_frame::RequestRejectionReason::Internal),
            },
        );
        write_frame(&mut stream, &frame).await?;
        return Ok(());
    }

    let outcome = match runtime
        .request(authenticated_principal, request.payloads.head().clone())
        .await
    {
        Ok(outcome) => outcome,
        Err(_) => crate::DispatchOutcome::rejected(NoWriteFailure::AuthorityUnavailable),
    };
    let reply = authority_reply(outcome.reply);
    let frame = TranslatorFrame::new(route, StreamingFrameBody::Reply { exchange, reply });
    write_frame(&mut stream, &frame).await?;

    if let Some(event) = outcome.event {
        write_event(&mut stream, route, exchange, event).await?;
    }
    Ok(())
}

fn authority_reply(reply: AuthorityReply) -> Reply<AuthorityReply> {
    match reply {
        rejected @ AuthorityReply::Rejected(_) => Reply::operation_aborted(
            0,
            OperationFailureReason::DomainRejection,
            NonEmpty::single(SubReply::Failed {
                reason: OperationFailureReason::DomainRejection,
                detail: Some(rejected),
            }),
        ),
        accepted => Reply::committed(NonEmpty::single(SubReply::Ok(accepted))),
    }
}

async fn write_event(
    stream: &mut UnixStream,
    route: signal_frame::WireRoute,
    exchange: signal_frame::ExchangeIdentifier,
    event: PostCommitEvent,
) -> Result<(), Error> {
    let sequence = exchange
        .sequence()
        .value()
        .checked_add(1)
        .ok_or_else(|| Error::Actor("acceptor event sequence exhausted".into()))?;
    let frame = TranslatorFrame::new(
        route,
        StreamingFrameBody::SubscriptionEvent {
            event_identifier: StreamEventIdentifier::acceptor(
                exchange.session_epoch(),
                LaneSequence::new(sequence),
            ),
            // A revision-1 mutating exchange subscribes only to its own
            // causally corresponding post-commit event.
            token: SubscriptionTokenInner::new(exchange.sequence().value()),
            event,
        },
    );
    write_frame(stream, &frame).await
}

async fn write_frame(stream: &mut UnixStream, frame: &TranslatorFrame) -> Result<(), Error> {
    let bytes = frame
        .encode_length_prefixed()
        .map_err(|error| Error::Actor(error.to_string()))?;
    stream
        .write_all(&bytes)
        .await
        .map_err(|error| Error::Actor(error.to_string()))
}

async fn read_frame(stream: &mut UnixStream) -> io::Result<Vec<u8>> {
    let length = stream.read_u32().await? as usize;
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "authority frame exceeds size limit",
        ));
    }
    let mut bytes = Vec::with_capacity(length + 4);
    bytes.extend_from_slice(&(length as u32).to_be_bytes());
    bytes.resize(length + 4, 0);
    stream.read_exact(&mut bytes[4..]).await?;
    Ok(bytes)
}

fn prepare_socket(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        let metadata = std::fs::metadata(parent)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o022 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "configured socket directory must not be group- or world-writable",
            ));
        }
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(unix)]
            {
                if !metadata.file_type().is_socket() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "configured socket path exists and is not a socket",
                    ));
                }
                match std::os::unix::net::UnixStream::connect(path) {
                    Ok(_) => {
                        return Err(io::Error::new(
                            io::ErrorKind::AddrInUse,
                            "configured socket already has a live listener",
                        ));
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {}
                    Err(error) => return Err(error),
                }
                let current = std::fs::symlink_metadata(path)?;
                if !current.file_type().is_socket()
                    || current.dev() != metadata.dev()
                    || current.ino() != metadata.ino()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "configured socket changed during stale-node validation",
                    ));
                }
            }
            std::fs::remove_file(path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}
