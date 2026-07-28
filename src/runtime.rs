use std::path::Path;
use std::sync::Arc;

use signal_sema_translator::{
    AuthorityReply, AuthorityRequest, CommittedReceipt, NoWriteFailure, PostCommitEvent,
    PrincipalId,
};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::authorization::AuthorizationPolicy;
use crate::store::{AuthorityStore, Error, Result};

/// One serialized actor result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchOutcome {
    pub reply: AuthorityReply,
    pub event: Option<PostCommitEvent>,
}

impl DispatchOutcome {
    pub(crate) fn rejected(failure: NoWriteFailure) -> Self {
        Self {
            reply: AuthorityReply::Rejected(failure),
            event: None,
        }
    }

    pub(crate) fn committed(receipt: CommittedReceipt, event: PostCommitEvent) -> Self {
        Self {
            reply: AuthorityReply::Committed(receipt),
            event: Some(event),
        }
    }
}

enum Command {
    Dispatch {
        authenticated: PrincipalId,
        request: AuthorityRequest,
        reply: oneshot::Sender<Result<DispatchOutcome>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

/// Cloneable handle to the sole-writer authority actor.
#[derive(Clone)]
pub struct Runtime {
    commands: mpsc::Sender<Command>,
    events: broadcast::Sender<PostCommitEvent>,
}

impl Runtime {
    /// Open and validate the owned embedded database before starting service.
    pub async fn open(
        database: &Path,
        authorization: Arc<dyn AuthorizationPolicy>,
    ) -> Result<Self> {
        let mut store = AuthorityStore::open(database)?;
        let (commands, mut receiver) = mpsc::channel::<Command>(64);
        let (events, _) = broadcast::channel(64);
        let actor_events = events.clone();
        tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                match command {
                    Command::Dispatch {
                        authenticated,
                        request,
                        reply,
                    } => {
                        let outcome =
                            match store.dispatch(authenticated, request, authorization.as_ref()) {
                                Err(Error::Engine(_)) => Ok(DispatchOutcome::rejected(
                                    NoWriteFailure::AuthorityUnavailable,
                                )),
                                outcome => outcome,
                            };
                        if let Ok(outcome) = &outcome
                            && let Some(event) = &outcome.event
                        {
                            let _ = actor_events.send(event.clone());
                        }
                        let fatal = outcome.is_err();
                        let _ = reply.send(outcome);
                        if fatal {
                            break;
                        }
                    }
                    Command::Shutdown { reply } => {
                        let _ = reply.send(());
                        break;
                    }
                }
            }
        });
        Ok(Self { commands, events })
    }

    /// Serialize one request through the actor that exclusively owns SEMA.
    pub async fn request(
        &self,
        authenticated: PrincipalId,
        request: AuthorityRequest,
    ) -> Result<DispatchOutcome> {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(Command::Dispatch {
                authenticated,
                request,
                reply,
            })
            .await
            .map_err(|_| Error::Actor("authority actor is unavailable".into()))?;
        receive
            .await
            .map_err(|_| Error::Actor("authority actor dropped its reply".into()))?
    }

    /// Observe committed events emitted after durable SEMA commit.
    pub fn subscribe(&self) -> broadcast::Receiver<PostCommitEvent> {
        self.events.subscribe()
    }

    /// Stop the actor after all preceding commands.
    pub async fn shutdown(&self) -> Result<()> {
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(Command::Shutdown { reply })
            .await
            .map_err(|_| Error::Actor("authority actor is unavailable".into()))?;
        receive
            .await
            .map_err(|_| Error::Actor("authority actor did not acknowledge shutdown".into()))
    }
}
