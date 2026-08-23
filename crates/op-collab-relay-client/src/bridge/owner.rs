use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::Instant;

use super::protocol::{establish_relay_with_ready_hook, RelayHandshake};
use super::BridgeHandle;
use crate::auth::{AuthMode, RelayAuthenticator, RelayCredentialProvider};
use crate::endpoint::RelayEndpoint;
use crate::error::{
    RelayBridgePhase, RelayBridgeStatus, RelayClientError, RelayFailureKind, RelayStopError,
    TunnelError,
};
use crate::limits::{
    owner_lane_first_pair_budget, RelayLimits, DEFAULT_OWNER_LANE_COUNT, MAX_OWNER_LANE_COUNT,
};
use crate::reauth_budget::ReauthBudget;
use crate::session::{cancelled, pump, sleep_or_cancel, ClientReauthContext};

/// A bounded pool of owner relay lanes.
pub struct RelayOwnerBridge {
    lane_count: usize,
    handle: BridgeHandle,
}

impl RelayOwnerBridge {
    pub async fn start(
        endpoint: RelayEndpoint,
        route: RelayHandshake,
        local_owner_socket: SocketAddr,
        lane_count: usize,
        authenticator: Arc<dyn RelayAuthenticator>,
    ) -> Result<Self, RelayClientError> {
        Self::start_inner(
            endpoint,
            route,
            local_owner_socket,
            lane_count,
            AuthMode::ChallengeBound(authenticator),
            RelayLimits::default(),
        )
        .await
    }

    pub async fn start_default_lanes(
        endpoint: RelayEndpoint,
        route: RelayHandshake,
        local_owner_socket: SocketAddr,
        authenticator: Arc<dyn RelayAuthenticator>,
    ) -> Result<Self, RelayClientError> {
        Self::start(
            endpoint,
            route,
            local_owner_socket,
            DEFAULT_OWNER_LANE_COUNT,
            authenticator,
        )
        .await
    }

    /// Starts explicit reduced-assurance ticket-to-DH owner lanes.
    pub async fn start_ticket_binding_only(
        endpoint: RelayEndpoint,
        route: RelayHandshake,
        local_owner_socket: SocketAddr,
        lane_count: usize,
        credentials: Arc<dyn RelayCredentialProvider>,
    ) -> Result<Self, RelayClientError> {
        Self::start_inner(
            endpoint,
            route,
            local_owner_socket,
            lane_count,
            AuthMode::TicketBindingOnly(credentials),
            RelayLimits::default(),
        )
        .await
    }

    pub async fn start_default_lanes_ticket_binding_only(
        endpoint: RelayEndpoint,
        route: RelayHandshake,
        local_owner_socket: SocketAddr,
        credentials: Arc<dyn RelayCredentialProvider>,
    ) -> Result<Self, RelayClientError> {
        Self::start_ticket_binding_only(
            endpoint,
            route,
            local_owner_socket,
            DEFAULT_OWNER_LANE_COUNT,
            credentials,
        )
        .await
    }

    /// Starts anonymous relay lanes for local development.
    ///
    /// This API is absent from release builds.
    #[cfg(any(test, debug_assertions))]
    pub async fn start_unauthenticated_for_development(
        endpoint: RelayEndpoint,
        route: RelayHandshake,
        local_owner_socket: SocketAddr,
        lane_count: usize,
    ) -> Result<Self, RelayClientError> {
        if !endpoint.is_numeric_loopback() {
            return Err(RelayClientError::DevelopmentEndpointNotLoopback);
        }
        Self::start_inner(
            endpoint,
            route,
            local_owner_socket,
            lane_count,
            AuthMode::DevelopmentAnonymous,
            RelayLimits::default(),
        )
        .await
    }

    pub fn lane_count(&self) -> usize {
        self.lane_count
    }

    pub fn status(&self) -> RelayBridgeStatus {
        self.handle.status()
    }

    pub fn subscribe(&self) -> watch::Receiver<RelayBridgeStatus> {
        self.handle.subscribe()
    }

    pub async fn stop(self) -> Result<(), RelayStopError> {
        self.handle.stop().await
    }

    /// Wait until at least one owner lane has been authenticated and accepted
    /// by the relay.
    pub async fn wait_until_ready(
        &self,
        timeout: std::time::Duration,
    ) -> Result<(), RelayClientError> {
        let mut status = self.subscribe();
        let ready = async {
            loop {
                match status.borrow().phase {
                    RelayBridgePhase::Waiting | RelayBridgePhase::Active => return Ok(()),
                    RelayBridgePhase::Stopped | RelayBridgePhase::Failed => {
                        return Err(RelayClientError::StoppedBeforeReady);
                    }
                    RelayBridgePhase::Starting | RelayBridgePhase::Degraded => {}
                }
                status
                    .changed()
                    .await
                    .map_err(|_| RelayClientError::StoppedBeforeReady)?;
            }
        };
        tokio::time::timeout(timeout, ready)
            .await
            .map_err(|_| RelayClientError::ReadyTimeout)?
    }

    async fn start_inner(
        endpoint: RelayEndpoint,
        route: RelayHandshake,
        local_owner_socket: SocketAddr,
        lane_count: usize,
        auth: AuthMode,
        limits: RelayLimits,
    ) -> Result<Self, RelayClientError> {
        if !(1..=MAX_OWNER_LANE_COUNT).contains(&lane_count) {
            return Err(RelayClientError::InvalidLaneCount {
                max: MAX_OWNER_LANE_COUNT,
            });
        }
        if !local_owner_socket.ip().is_loopback() || local_owner_socket.port() == 0 {
            return Err(RelayClientError::InvalidLocalSocket);
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (status_tx, status_rx) = watch::channel(RelayBridgeStatus::starting(lane_count));
        let task = tokio::spawn(run_owner(
            endpoint,
            route,
            local_owner_socket,
            lane_count,
            auth,
            cancel_rx,
            status_tx,
            limits,
        ));
        Ok(Self {
            lane_count,
            handle: BridgeHandle::new(cancel_tx, status_rx, task, limits),
        })
    }

    #[cfg(test)]
    pub(crate) async fn start_test(
        endpoint: RelayEndpoint,
        route: RelayHandshake,
        local_owner_socket: SocketAddr,
        lane_count: usize,
        limits: RelayLimits,
    ) -> Result<Self, RelayClientError> {
        Self::start_inner(
            endpoint,
            route,
            local_owner_socket,
            lane_count,
            AuthMode::DevelopmentAnonymous,
            limits,
        )
        .await
    }
}

impl fmt::Debug for RelayOwnerBridge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayOwnerBridge")
            .field("lane_count", &self.lane_count)
            .field("handle", &self.handle)
            .finish()
    }
}

enum LaneEvent {
    Active(oneshot::Sender<()>),
}

/// Why a lane task ended, as the pool needs to read it.
///
/// A recycle is the client's own scheduled retirement of an idle lane, not a
/// fault: it must neither surface as `last_error` nor serve a backoff delay,
/// or the pool would advertise a broken relay and leave the waiting queue
/// short for a full retry window every time it refreshes itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaneOutcome {
    Recycled,
    Cancelled,
    Failed(RelayFailureKind),
}

#[derive(Clone, Copy, Debug)]
struct LaneReport {
    became_ready: bool,
    became_active: bool,
    outcome: LaneOutcome,
}

/// Classify a finished lane. A pair-phase timeout on a lane that never paired
/// is the scheduled recycle; everything else keeps its failure identity.
fn lane_outcome(became_active: bool, result: Result<(), TunnelError>) -> LaneOutcome {
    match result.err() {
        None => LaneOutcome::Recycled,
        Some(TunnelError::Failure(RelayFailureKind::PairTimeout)) if !became_active => {
            LaneOutcome::Recycled
        }
        Some(error) => error
            .failure_kind()
            .map_or(LaneOutcome::Cancelled, LaneOutcome::Failed),
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_owner(
    endpoint: RelayEndpoint,
    route: RelayHandshake,
    local_owner_socket: SocketAddr,
    lane_count: usize,
    auth: AuthMode,
    mut cancel: watch::Receiver<bool>,
    status_tx: watch::Sender<RelayBridgeStatus>,
    limits: RelayLimits,
) {
    let (event_tx, mut event_rx) = mpsc::channel(MAX_OWNER_LANE_COUNT);
    let (ready_tx, mut ready_rx) = mpsc::channel(MAX_OWNER_LANE_COUNT);
    let factory = LaneFactory {
        endpoint,
        route,
        local_owner_socket,
        auth,
        cancel: cancel.clone(),
        event_tx,
        ready_tx,
        limits,
    };
    let mut lanes = JoinSet::new();
    // Staggered first budgets, so the pool refreshes one lane at a time and
    // the relay's waiting queue is never left empty.
    for index in 0..lane_count {
        factory.spawn(
            &mut lanes,
            owner_lane_first_pair_budget(limits.owner_pair, index, lane_count),
            false,
        );
    }
    // Lane tasks that are not tunnelling: what the relay's waiting queue is
    // being kept stocked with. Tracked separately from `waiting` (lanes the
    // relay has already accepted) so a lane still dialling still counts.
    let mut unpaired_lanes = lane_count;

    let mut waiting = 0_usize;
    let mut active = 0_usize;
    let mut last_error = None;
    publish_owner(&status_tx, waiting, active, last_error);

    loop {
        tokio::select! {
            biased;
            _ = cancelled(&mut cancel) => break,
            ready = ready_rx.recv() => {
                if ready.is_some() {
                    waiting = waiting.saturating_add(1).min(lane_count);
                    last_error = None;
                    publish_owner(&status_tx, waiting, active, last_error);
                }
            }
            event = event_rx.recv() => {
                if let Some(LaneEvent::Active(acknowledge)) = event {
                    waiting = waiting.saturating_sub(1);
                    active = active.saturating_add(1).min(MAX_OWNER_LANE_COUNT);
                    last_error = None;
                    publish_owner(&status_tx, waiting, active, last_error);
                    let _ = acknowledge.send(());
                    // A paired lane has left the relay's waiting queue for
                    // good, so the pool owes it a replacement: without one it
                    // shrinks by a lane on every join and the Nth guest waits
                    // for a counterpart that is no longer there.
                    unpaired_lanes = unpaired_lanes.saturating_sub(1);
                    factory.refill(&mut lanes, &mut unpaired_lanes, lane_count, false);
                }
            }
            completed = lanes.join_next() => {
                let Some(completed) = completed else {
                    break;
                };
                let report = match completed {
                    Ok(report) => report,
                    Err(_) => LaneReport {
                        became_ready: false,
                        became_active: false,
                        outcome: LaneOutcome::Failed(RelayFailureKind::RelayIo),
                    },
                };
                if report.became_active {
                    // Replaced already, when it left the waiting queue.
                    active = active.saturating_sub(1);
                } else {
                    if report.became_ready {
                        waiting = waiting.saturating_sub(1);
                    }
                    unpaired_lanes = unpaired_lanes.saturating_sub(1);
                }
                if let LaneOutcome::Failed(kind) = report.outcome {
                    last_error = Some(kind);
                }
                publish_owner(&status_tx, waiting, active, last_error);
                // A scheduled recycle re-dials at once; only a real failure
                // serves the backoff.
                let delayed = matches!(report.outcome, LaneOutcome::Failed(_));
                factory.refill(&mut lanes, &mut unpaired_lanes, lane_count, delayed);
            }
        }
    }
    lanes.abort_all();
    while lanes.join_next().await.is_some() {}
    status_tx.send_replace(RelayBridgeStatus::stopped());
}

/// Everything one lane task needs, plus the pool's spawn policy.
///
/// The pool creates lanes from three places (start-up, replacing a paired
/// lane, replacing a finished one); holding the shared handles here keeps
/// those call sites to the two values that actually differ.
struct LaneFactory {
    endpoint: RelayEndpoint,
    route: RelayHandshake,
    local_owner_socket: SocketAddr,
    auth: AuthMode,
    cancel: watch::Receiver<bool>,
    event_tx: mpsc::Sender<LaneEvent>,
    ready_tx: mpsc::Sender<()>,
    limits: RelayLimits,
}

impl LaneFactory {
    fn spawn(&self, lanes: &mut JoinSet<LaneReport>, pair_budget: Duration, delayed: bool) {
        let spawn = LaneSpawn {
            endpoint: self.endpoint.clone(),
            route: self.route.clone(),
            local_owner_socket: self.local_owner_socket,
            auth: self.auth.clone(),
            cancel: self.cancel.clone(),
            event_tx: self.event_tx.clone(),
            ready_tx: self.ready_tx.clone(),
            limits: self.limits,
            pair_budget,
            delayed,
        };
        lanes.spawn(async move { run_lane(spawn).await });
    }

    /// Bring the unpaired-lane count back to `lane_count`, without letting the
    /// pool's total (unpaired plus tunnelling) exceed the hard ceiling.
    fn refill(
        &self,
        lanes: &mut JoinSet<LaneReport>,
        unpaired_lanes: &mut usize,
        lane_count: usize,
        delayed: bool,
    ) {
        while *unpaired_lanes < lane_count && lanes.len() < MAX_OWNER_LANE_COUNT {
            self.spawn(lanes, self.limits.owner_pair, delayed);
            *unpaired_lanes = unpaired_lanes.saturating_add(1);
        }
    }
}

/// One lane task's inputs.
struct LaneSpawn {
    endpoint: RelayEndpoint,
    route: RelayHandshake,
    local_owner_socket: SocketAddr,
    auth: AuthMode,
    cancel: watch::Receiver<bool>,
    event_tx: mpsc::Sender<LaneEvent>,
    ready_tx: mpsc::Sender<()>,
    limits: RelayLimits,
    /// How long this lane may sit unpaired before the client recycles it.
    pair_budget: Duration,
    delayed: bool,
}

async fn run_lane(spawn: LaneSpawn) -> LaneReport {
    let LaneSpawn {
        endpoint,
        route,
        local_owner_socket,
        auth,
        mut cancel,
        event_tx,
        ready_tx,
        limits,
        pair_budget,
        delayed,
    } = spawn;
    let mut became_ready = false;
    let mut became_active = false;
    let result = async {
        if delayed && !sleep_or_cancel(limits.retry, &mut cancel).await {
            return Err(TunnelError::Cancelled);
        }
        let started_at = Instant::now();
        // One budget per lane connection: a lane that reconnects starts over,
        // and a lane that is spammed cannot borrow another lane's headroom.
        let mut reauth_budget = ReauthBudget::new(limits);
        let socket = establish_relay_with_ready_hook(
            &endpoint,
            &route,
            op_collab_relay_protocol::RelayRole::Owner,
            &auth,
            &mut reauth_budget,
            &mut cancel,
            started_at,
            limits,
            pair_budget,
            || {
                became_ready = true;
                let _ = ready_tx.try_send(());
            },
        )
        .await?;
        let local = connect_local(local_owner_socket, &mut cancel, limits).await?;
        let (acknowledge, acknowledged) = oneshot::channel();
        tokio::select! {
            _ = cancelled(&mut cancel) => return Err(TunnelError::Cancelled),
            result = event_tx.send(LaneEvent::Active(acknowledge)) => {
                if result.is_err() {
                    return Err(TunnelError::Cancelled);
                }
            }
        }
        tokio::select! {
            _ = cancelled(&mut cancel) => return Err(TunnelError::Cancelled),
            result = acknowledged => {
                if result.is_err() {
                    return Err(TunnelError::Cancelled);
                }
            }
        }
        became_active = true;
        pump(
            socket,
            local,
            ClientReauthContext {
                auth: &auth,
                role: op_collab_relay_protocol::RelayRole::Owner,
                route: route.route(),
            },
            &mut reauth_budget,
            &mut cancel,
            started_at,
            limits,
        )
        .await
    }
    .await;
    LaneReport {
        became_ready,
        became_active,
        outcome: lane_outcome(became_active, result),
    }
}

async fn connect_local(
    local_owner_socket: SocketAddr,
    cancel: &mut watch::Receiver<bool>,
    limits: RelayLimits,
) -> Result<TcpStream, TunnelError> {
    let local = tokio::select! {
        _ = cancelled(cancel) => return Err(TunnelError::Cancelled),
        result = tokio::time::timeout(limits.connect, TcpStream::connect(local_owner_socket)) => {
            match result {
                Err(_) => return Err(TunnelError::Failure(RelayFailureKind::ConnectTimeout)),
                Ok(Err(_)) => return Err(TunnelError::Failure(RelayFailureKind::LocalIo)),
                Ok(Ok(local)) => local,
            }
        }
    };
    local
        .set_nodelay(true)
        .map_err(|_| TunnelError::Failure(RelayFailureKind::LocalIo))?;
    Ok(local)
}

fn publish_owner(
    status: &watch::Sender<RelayBridgeStatus>,
    waiting_lanes: usize,
    active_tunnels: usize,
    last_error: Option<RelayFailureKind>,
) {
    let phase = if active_tunnels > 0 {
        RelayBridgePhase::Active
    } else if waiting_lanes > 0 {
        RelayBridgePhase::Waiting
    } else if last_error.is_some() {
        RelayBridgePhase::Degraded
    } else {
        RelayBridgePhase::Starting
    };
    status.send_replace(RelayBridgeStatus {
        phase,
        waiting_lanes,
        active_tunnels,
        last_error,
    });
}
