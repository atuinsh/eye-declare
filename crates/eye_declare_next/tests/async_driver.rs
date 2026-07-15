//! Async driver behavior, driven headlessly: spawned streams feed
//! `update`; dropping a `Task` cancels mid-stream; `perform` delivers a
//! one-shot message.

use std::time::Duration;

use eye_declare_engine::test_terminal::TestTerminal;
use eye_declare_next::driver_tokio::spawn_effects;
use eye_declare_next::{App, Ctx, Element, Runtime, Task, col, text};
use futures::StreamExt;

#[derive(Clone)]
enum Msg {
    Start,
    Delta(String),
    Done,
    Cancel,
}

#[derive(Default)]
struct Agent {
    slow: bool,
    response: String,
    task: Option<Task>,
    turns: usize,
}

impl App for Agent {
    type Msg = Msg;
    type Output = ();

    fn update(&mut self, msg: Msg, ctx: &mut Ctx<'_, Self>) {
        match msg {
            Msg::Start => {
                let stream: futures::stream::BoxStream<'static, Msg> = if self.slow {
                    futures::stream::unfold(0u8, |n| async move {
                        match n {
                            0 => Some((Msg::Delta("first".into()), 1)),
                            1 => {
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                Some((Msg::Delta("second".into()), 2))
                            }
                            _ => None,
                        }
                    })
                    .boxed()
                } else {
                    futures::stream::iter([
                        Msg::Delta("hel".into()),
                        Msg::Delta("lo".into()),
                        Msg::Done,
                    ])
                    .boxed()
                };
                self.task = Some(ctx.spawn(stream));
            }
            Msg::Delta(s) => self.response.push_str(&s),
            Msg::Done => {
                let content = std::mem::take(&mut self.response);
                ctx.push(text(format!("ai: {content}")));
                self.task = None;
                self.turns += 1;
            }
            Msg::Cancel => {
                // Cancellation is an assignment: dropping the Task cancels
                // the spawned stream.
                self.task = None;
            }
        }
    }

    fn tail(&self) -> impl Element + '_ {
        col().child(text(format!("~ {}", self.response)))
    }
}

#[tokio::test]
async fn spawned_stream_drives_updates() {
    let mut rt = Runtime::new(Agent::default(), 20, 24);
    let mut term = TestTerminal::new(20, 24);
    term.feed(&rt.present());

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (bytes, _) = rt.process(Msg::Start);
    term.feed(&bytes);
    spawn_effects(rt.take_effects(), &tx);

    while rt.app().turns == 0 {
        let msg = rx.recv().await.expect("stream should keep feeding");
        let (bytes, _) = rt.process(msg);
        term.feed(&bytes);
        spawn_effects(rt.take_effects(), &tx);
    }

    // The completed turn is a committed block; the tail reset below it.
    assert_eq!(term.viewport_lines()[0], "ai: hello");
    assert_eq!(term.viewport_lines()[1], "~");
}

#[tokio::test]
async fn dropping_task_cancels_stream() {
    let mut rt = Runtime::new(
        Agent {
            slow: true,
            ..Agent::default()
        },
        20,
        24,
    );

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (_, _) = rt.process(Msg::Start);
    spawn_effects(rt.take_effects(), &tx);

    // First item arrives, then the stream sleeps.
    let first = rx.recv().await.expect("first item");
    let (_, _) = rt.process(first);
    assert_eq!(rt.app().response, "first");

    // Cancel while the stream sleeps; the spawned task must end without
    // producing "second". Dropping our tx makes rx.recv() return None
    // exactly when the task's tx clone is gone.
    let (_, _) = rt.process(Msg::Cancel);
    drop(tx);
    let next = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("cancelled task should end promptly");
    assert!(next.is_none(), "no further items after cancellation");
    assert_eq!(rt.app().response, "first");
}

#[tokio::test]
async fn perform_delivers_one_message() {
    #[derive(Clone)]
    enum FetchMsg {
        Start,
        Got(&'static str),
    }

    struct OneShot {
        got: Option<&'static str>,
        task: Option<Task>,
    }

    impl App for OneShot {
        type Msg = FetchMsg;
        type Output = ();

        fn update(&mut self, msg: FetchMsg, ctx: &mut Ctx<'_, Self>) {
            match msg {
                FetchMsg::Start => {
                    self.task = Some(ctx.perform(async {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        FetchMsg::Got("data")
                    }));
                }
                FetchMsg::Got(value) => {
                    self.got = Some(value);
                    self.task = None;
                }
            }
        }

        fn tail(&self) -> impl Element + '_ {
            text(self.got.unwrap_or("waiting"))
        }
    }

    let mut rt = Runtime::new(
        OneShot {
            got: None,
            task: None,
        },
        20,
        24,
    );
    let mut term = TestTerminal::new(20, 24);
    term.feed(&rt.present());
    assert_eq!(term.viewport_lines()[0], "waiting");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (bytes, _) = rt.process(FetchMsg::Start);
    term.feed(&bytes);
    spawn_effects(rt.take_effects(), &tx);

    let msg = rx
        .recv()
        .await
        .expect("perform delivers exactly one message");
    let (bytes, _) = rt.process(msg);
    term.feed(&bytes);
    assert_eq!(term.viewport_lines()[0], "data");

    // The future completed; the stream behind it ends.
    drop(tx);
    assert!(rx.recv().await.is_none());
}
