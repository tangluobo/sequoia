//! Real encrypted loopback peers with an input fixture; never installs system hooks.
use super::*;
use tomato_core::input_share::KeyCode;
struct Fixture(Arc<Mutex<Vec<Update>>>);
impl InputAdapter for Fixture {
    fn send(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
    fn failed(&self) -> bool {
        false
    }
}
struct Harness {
    session: Session,
    input: mpsc::Sender<Event>,
    injected: Arc<Mutex<Vec<Update>>>,
}
fn node(id: &str) -> Node {
    Node {
        id: id.into(),
        name: id.into(),
        width: 1920,
        height: 1080,
    }
}
fn spawn(
    local: Node,
    hosting: bool,
    listener: Option<TcpListener>,
    initial: Option<Connection>,
) -> Harness {
    let state = Arc::new(Mutex::new(Snapshot {
        running: true,
        hosting,
        nodes: vec![local.clone()],
        ..Default::default()
    }));
    let (tx, rx) = mpsc::channel(512);
    let input = tx.clone();
    let injected = Arc::new(Mutex::new(Vec::new()));
    let actor = Actor {
        local,
        state: state.clone(),
        tx: tx.clone(),
        rx,
        capture: Box::new(Fixture(injected.clone())),
        peers: HashMap::new(),
        tasks: JoinSet::new(),
        hosting,
        key: [42; 16],
        owner: None,
    };
    let task = tokio::spawn(actor.run(listener, None, initial));
    Harness {
        session: Session { state, tx, task },
        input,
        injected,
    }
}
async fn wait_for(mut condition: impl FnMut() -> bool) {
    timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fixture deadline");
}
fn has_key(h: &Harness) -> bool {
    h.injected.lock().unwrap().iter().any(|u| {
        matches!(
            u,
            Update::Inject(Input::Key {
                key: KeyCode::Scan {
                    code: 30,
                    extended: false
                },
                down: true,
                ..
            })
        )
    })
}
#[test]
fn late_input_from_a_previous_owner_cannot_reclaim_control() {
    let local = node("host");
    let nodes = vec![local.clone(), node("a"), node("b")];
    let state = Arc::new(Mutex::new(Snapshot {
        nodes,
        ..Default::default()
    }));
    let (tx, rx) = mpsc::channel(16);
    let updates = Arc::new(Mutex::new(Vec::new()));
    let (a, _a) = mpsc::channel(16);
    let (b, mut b_events) = mpsc::channel(16);
    let peers = HashMap::from([
        (
            "a".into(),
            Peer {
                token: "a".into(),
                sender: a,
            },
        ),
        (
            "b".into(),
            Peer {
                token: "b".into(),
                sender: b,
            },
        ),
    ]);
    let mut actor = Actor {
        local,
        state,
        tx,
        rx,
        capture: Box::new(Fixture(updates.clone())),
        peers,
        tasks: JoinSet::new(),
        hosting: true,
        key: [42; 16],
        owner: None,
    };
    actor.input_from(
        "a",
        Routed {
            target: "host".into(),
            input: Input::Claim,
        },
    );
    actor.input_from(
        "b",
        Routed {
            target: "host".into(),
            input: Input::Claim,
        },
    );
    let before = updates.lock().unwrap().len();
    actor.input_from(
        "a",
        Routed {
            target: "host".into(),
            input: Input::Key {
                key: KeyCode::Scan {
                    code: 30,
                    extended: false,
                },
                down: true,
            },
        },
    );
    assert_eq!(actor.owner.as_deref(), Some("b"));
    assert_eq!(updates.lock().unwrap().len(), before);
    while b_events.try_recv().is_ok() {}
    actor.relinquish("a");
    assert!(actor.owner.is_none());
    let mut returned = false;
    while let Ok(event) = b_events.try_recv() {
        returned |= matches!(event, Wire::Yield);
    }
    assert!(
        returned,
        "Pausing another node must return the previous source to local input"
    );
}
#[tokio::test]
async fn three_nodes_share_order_route_keys_reclaim_and_release_on_disconnect() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let host = spawn(node("host"), true, Some(listener), None);
    let a = authenticate(TcpStream::connect(address).await?, [42; 16], node("a")).await?;
    let a = spawn(node("a"), false, None, Some(a));
    let b = authenticate(TcpStream::connect(address).await?, [42; 16], node("b")).await?;
    let b = spawn(node("b"), false, None, Some(b));
    wait_for(|| a.session.snapshot().nodes.len() == 3 && b.session.snapshot().nodes.len() == 3)
        .await;
    a.input
        .send(Event::Native(Routed {
            target: "a".into(),
            input: Input::Display {
                width: 2560,
                height: 1440,
            },
        }))
        .await?;
    wait_for(|| {
        b.session
            .snapshot()
            .nodes
            .iter()
            .any(|n| n.id == "a" && n.width == 2560 && n.height == 1440)
    })
    .await;
    let ids = vec!["b".into(), "host".into(), "a".into()];
    a.session.action(Action::Order { ids: ids.clone() }).await?;
    wait_for(|| {
        b.session
            .snapshot()
            .nodes
            .iter()
            .map(|n| n.id.clone())
            .collect::<Vec<_>>()
            == ids
    })
    .await;
    assert!(
        host.session
            .action(Action::Order {
                ids: vec!["b".into(); 3]
            })
            .await
            .is_err()
    );
    // A stale/invalid peer reorder is rejected by the coordinator, leaving the layout intact.
    let event = Input::Key {
        key: KeyCode::Scan {
            code: 30,
            extended: false,
        },
        down: true,
    };
    a.input
        .send(Event::Native(Routed {
            target: "b".into(),
            input: Input::Claim,
        }))
        .await?;
    a.input
        .send(Event::Native(Routed {
            target: "b".into(),
            input: event.clone(),
        }))
        .await?;
    wait_for(|| has_key(&b)).await;
    a.input
        .send(Event::Native(Routed {
            target: "b".into(),
            input: Input::Key {
                key: KeyCode::Virtual(0xaf),
                down: false,
            },
        }))
        .await?;
    wait_for(|| {
        b.injected.lock().unwrap().iter().any(|u| {
            matches!(
                u,
                Update::Inject(Input::Key {
                    key: KeyCode::Virtual(0xaf),
                    down: false
                })
            )
        })
    })
    .await;
    for input in [
        Input::Move { x: 777, y: 333 },
        Input::Button {
            button: 0,
            down: true,
        },
        Input::Wheel {
            delta: 120,
            horizontal: true,
        },
    ] {
        a.input
            .send(Event::Native(Routed {
                target: "b".into(),
                input,
            }))
            .await?;
    }
    wait_for(|| {
        b.injected.lock().unwrap().iter().any(|u| {
            matches!(
                u,
                Update::Inject(Input::Wheel {
                    delta: 120,
                    horizontal: true
                })
            )
        })
    })
    .await;
    {
        let events = b.injected.lock().unwrap();
        let moved = events
            .iter()
            .position(|u| matches!(u, Update::Inject(Input::Move { x: 777, y: 333 })))
            .unwrap();
        let pressed = events
            .iter()
            .position(|u| {
                matches!(
                    u,
                    Update::Inject(Input::Button {
                        button: 0,
                        down: true
                    })
                )
            })
            .unwrap();
        let wheel = events
            .iter()
            .position(|u| matches!(u, Update::Inject(Input::Wheel { .. })))
            .unwrap();
        assert!(moved < pressed && pressed < wheel);
    }
    b.injected.lock().unwrap().clear();
    b.input
        .send(Event::Native(Routed {
            target: "b".into(),
            input: Input::Claim,
        }))
        .await?;
    wait_for(|| {
        a.injected
            .lock()
            .unwrap()
            .iter()
            .any(|u| matches!(u, Update::Return))
    })
    .await;
    wait_for(|| {
        b.injected
            .lock()
            .unwrap()
            .iter()
            .any(|u| matches!(u, Update::Inject(Input::Release)))
    })
    .await;
    b.input
        .send(Event::Native(Routed {
            target: "host".into(),
            input: event,
        }))
        .await?;
    wait_for(|| has_key(&host)).await;
    host.injected.lock().unwrap().clear();
    drop(b);
    wait_for(|| host.session.snapshot().nodes.len() == 2).await;
    assert!(
        host.injected
            .lock()
            .unwrap()
            .iter()
            .any(|u| matches!(u, Update::Inject(Input::Release)))
    );
    drop(host);
    wait_for(|| !a.session.snapshot().running).await;
    Ok(())
}
#[tokio::test]
async fn wrong_pairing_code_does_not_register_a_node() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let host = spawn(node("host"), true, Some(listener), None);
    assert!(
        authenticate(
            TcpStream::connect(address).await?,
            [0; 16],
            node("intruder")
        )
        .await
        .is_err()
    );
    assert_eq!(host.session.snapshot().nodes.len(), 1);
    Ok(())
}
