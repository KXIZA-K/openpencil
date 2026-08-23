//! Owner relay-lane pool behaviour: how the pool recycles idle lanes on its
//! own budget, replaces a lane a guest has paired with, and refuses to
//! over-provision once a tunnel ends. Split out of `bridge/tests.rs` at the
//! 800-line cap; pure code motion.

use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_tunnel_does_not_leave_the_pool_over_provisioned() {
    let owner_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let owner_addr = owner_listener.local_addr().unwrap();
    let owner_driver = tokio::spawn(async move {
        // Accept the tunnel's local end, then drop it to end the tunnel.
        let (stream, _) = owner_listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(stream);
        tokio::time::sleep(Duration::from_secs(3)).await;
    });
    let relay_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = relay_listener.local_addr().unwrap();
    // Concurrently open relay connections. The relay caps waiting peers per
    // route, so a pool that replaces both the paired lane AND its own
    // replacement would be rejected in production; here it shows up as an
    // over-count.
    let open = Arc::new(AtomicUsize::new(0));
    let server_open = Arc::clone(&open);
    let server = tokio::spawn(async move {
        let mut first = true;
        while let Ok((stream, _)) = relay_listener.accept().await {
            let pair = std::mem::replace(&mut first, false);
            let open = Arc::clone(&server_open);
            tokio::spawn(async move {
                let Ok(mut socket) = accept_async(stream).await else {
                    return;
                };
                receive_hello(&mut socket, RelayRole::Owner).await;
                if pair {
                    mark_ready_and_paired(&mut socket).await;
                } else if socket
                    .send(Message::Binary(RelayServerStatus::Ready.encode().to_vec()))
                    .await
                    .is_err()
                {
                    return;
                }
                open.fetch_add(1, Ordering::SeqCst);
                while socket.next().await.is_some() {}
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });

    let mut limits = test_limits();
    limits.idle = Duration::from_secs(6);
    limits.lifetime = Duration::from_secs(12);
    limits.owner_pair = Duration::from_secs(6);
    let bridge =
        RelayOwnerBridge::start_test(endpoint(relay_addr), handshake(6), owner_addr, 1, limits)
            .await
            .unwrap();

    // Wait for the tunnel to come up and then finish.
    let deadline = StdInstant::now() + Duration::from_secs(3);
    while bridge.status().active_tunnels == 0 && StdInstant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(bridge.status().active_tunnels, 1);
    let deadline = StdInstant::now() + Duration::from_secs(3);
    while bridge.status().active_tunnels > 0 && StdInstant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(bridge.status().active_tunnels, 0);

    // The pool settles back to exactly one waiting lane: the tunnel's slot was
    // already replaced when it paired, so its end owes nothing further.
    let deadline = StdInstant::now() + Duration::from_secs(2);
    while open.load(Ordering::SeqCst) != 1 && StdInstant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        open.load(Ordering::SeqCst),
        1,
        "the pool must hold exactly `lane_count` unpaired lanes once the tunnel ends"
    );
    assert_eq!(bridge.status().waiting_lanes, 1);

    bridge.stop().await.unwrap();
    server.abort();
    owner_driver.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_paired_owner_lane_is_replaced_so_the_waiting_queue_never_empties() {
    let owner_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let owner_addr = owner_listener.local_addr().unwrap();
    let owner_driver = tokio::spawn(async move {
        // Hold the tunnel's local end open for the duration of the assertion.
        let (stream, _) = owner_listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        drop(stream);
    });
    let relay_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = relay_listener.local_addr().unwrap();
    // The first lane is paired with a guest; every later lane is admitted and
    // left waiting.
    let server = tokio::spawn(async move {
        let mut first = true;
        while let Ok((stream, _)) = relay_listener.accept().await {
            let pair = std::mem::replace(&mut first, false);
            tokio::spawn(async move {
                let Ok(mut socket) = accept_async(stream).await else {
                    return;
                };
                receive_hello(&mut socket, RelayRole::Owner).await;
                if pair {
                    mark_ready_and_paired(&mut socket).await;
                } else if socket
                    .send(Message::Binary(RelayServerStatus::Ready.encode().to_vec()))
                    .await
                    .is_err()
                {
                    return;
                }
                while socket.next().await.is_some() {}
            });
        }
    });

    let mut limits = test_limits();
    limits.idle = Duration::from_secs(6);
    limits.lifetime = Duration::from_secs(12);
    limits.owner_pair = Duration::from_secs(6);
    let bridge =
        RelayOwnerBridge::start_test(endpoint(relay_addr), handshake(6), owner_addr, 1, limits)
            .await
            .unwrap();

    let deadline = StdInstant::now() + Duration::from_secs(3);
    let mut status = bridge.status();
    while (status.active_tunnels == 0 || status.waiting_lanes == 0) && StdInstant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
        status = bridge.status();
    }
    assert_eq!(
        status.active_tunnels, 1,
        "the guest lane must be tunnelling"
    );
    assert!(
        status.waiting_lanes >= 1,
        "a paired lane must be replaced, or the next guest has nothing to pair with"
    );

    bridge.stop().await.unwrap();
    server.abort();
    owner_driver.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_lanes_recycle_on_the_client_budget_and_keep_the_pool_populated() {
    let owner_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let owner_addr = owner_listener.local_addr().unwrap();
    let relay_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = relay_listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    // A relay that admits owner lanes and never pairs them — an idle session
    // waiting for its first guest.
    let server_accepted = Arc::clone(&accepted);
    let server = tokio::spawn(async move {
        while let Ok((stream, _)) = relay_listener.accept().await {
            let accepted = Arc::clone(&server_accepted);
            tokio::spawn(async move {
                let Ok(mut socket) = accept_async(stream).await else {
                    return;
                };
                receive_hello(&mut socket, RelayRole::Owner).await;
                if socket
                    .send(Message::Binary(RelayServerStatus::Ready.encode().to_vec()))
                    .await
                    .is_err()
                {
                    return;
                }
                accepted.fetch_add(1, Ordering::SeqCst);
                while socket.next().await.is_some() {}
            });
        }
    });

    let mut limits = test_limits();
    limits.owner_pair = Duration::from_millis(120);
    // Long enough that only an undelayed re-dial can produce another lane
    // inside this test: a recycle must not serve the failure backoff.
    limits.retry = Duration::from_secs(30);
    let bridge =
        RelayOwnerBridge::start_test(endpoint(relay_addr), handshake(6), owner_addr, 2, limits)
            .await
            .unwrap();
    bridge
        .wait_until_ready(Duration::from_secs(2))
        .await
        .unwrap();

    // Staggered first budgets (60 ms and 120 ms) plus a further full cycle.
    let deadline = StdInstant::now() + Duration::from_secs(3);
    while accepted.load(Ordering::SeqCst) <= 2 && StdInstant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
        // A recycle is never a fault, so the pool must not go Degraded and
        // must keep at least one lane in the relay's waiting queue.
        let status = bridge.status();
        assert_eq!(status.last_error, None, "recycling is not a relay failure");
        assert_ne!(status.phase, RelayBridgePhase::Degraded);
    }
    assert!(
        accepted.load(Ordering::SeqCst) > 2,
        "owner lanes must re-dial on the client budget, not wait for the relay"
    );
    assert_eq!(bridge.status().phase, RelayBridgePhase::Waiting);
    assert!(bridge.status().waiting_lanes > 0);

    bridge.stop().await.unwrap();
    server.abort();
    drop(owner_listener);
}
