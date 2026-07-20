//! Subscription behavior, driven headlessly against the tokio driver's
//! ActiveSubscriptions: keyed diffing, state-conditional lifecycles, and
//! stream cancellation on removal.

use std::time::Duration;

use eye_declare::driver_tokio::{ActiveSubscriptions, SyncReport};
use eye_declare::{App, Ctx, Element, Fluent, Runtime, Subscriptions, text};

#[derive(Clone)]
enum Msg {
    Tick,
}

/// Polls while `count < 3` — the state-conditional pattern (e.g. "poll the
/// server while a session is active").
struct Poller {
    count: usize,
}

impl App for Poller {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, _msg: Msg, _ctx: &mut Ctx<'_, Self>) {
        self.count += 1;
    }

    fn tail(&self) -> impl Element + '_ {
        text(format!("polled {}", self.count))
    }

    fn subscriptions(&self) -> Subscriptions<Msg> {
        Subscriptions::new().when(self.count < 3, |s| {
            s.every("poll", Duration::from_millis(5), || Msg::Tick)
        })
    }
}

#[tokio::test]
async fn every_fires_until_condition_drops() {
    let mut rt = Runtime::new(Poller { count: 0 }, 20, 24);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut subs = ActiveSubscriptions::new(tx.clone());

    subs.sync(rt.app().subscriptions());

    // Ticks arrive and drive updates until the model stops declaring the
    // subscription; the diff then cancels it.
    while rt.app().count < 3 {
        let msg = rx.recv().await.expect("subscription should tick");
        let _ = rt.process(msg);
        subs.sync(rt.app().subscriptions());
    }

    // The poll is gone: no further ticks arrive.
    let extra = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(extra.is_err(), "no ticks after the condition dropped");
    assert_eq!(rt.app().count, 3);
}

#[tokio::test]
async fn sync_reports_lifecycle_and_interval_restarts() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
    let mut subs = ActiveSubscriptions::new(tx);

    let declare =
        |ms: u64| Subscriptions::new().every("poll", Duration::from_millis(ms), || Msg::Tick);

    // First declaration starts it.
    assert_eq!(
        subs.sync(declare(50)),
        SyncReport {
            started: vec!["poll".into()],
            stopped: vec![],
        }
    );

    // Same key, same interval: untouched.
    assert_eq!(subs.sync(declare(50)), SyncReport::default());

    // Same key, new interval: restart.
    assert_eq!(
        subs.sync(declare(10)),
        SyncReport {
            started: vec!["poll".into()],
            stopped: vec!["poll".into()],
        }
    );

    // Key disappears: stopped.
    assert_eq!(
        subs.sync(Subscriptions::new()),
        SyncReport {
            started: vec![],
            stopped: vec!["poll".into()],
        }
    );
}

#[tokio::test]
async fn stream_subscription_cancels_on_removal() {
    #[derive(Clone)]
    struct Item(u8);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Item>();
    let mut subs = ActiveSubscriptions::new(tx.clone());

    // A stream: one immediate item, then a long sleep before the second.
    let declared = Subscriptions::new().stream("events", || {
        futures::stream::unfold(0u8, |n| async move {
            match n {
                0 => Some((Item(0), 1)),
                1 => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Some((Item(1), 2))
                }
                _ => None,
            }
        })
    });
    subs.sync(declared);

    let first = rx.recv().await.expect("first stream item");
    assert_eq!(first.0, 0);

    // Remove the key while the stream sleeps: it must end without the
    // second item. Dropping our tx makes recv() return None exactly when
    // the driver task's tx clone is gone.
    subs.sync(Subscriptions::new());
    drop(tx);
    drop(subs);
    let next = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("cancelled stream should end promptly");
    assert!(next.is_none(), "no items after removal");
}
