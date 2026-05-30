//! This is (Yet Another) Async Runtime for WASI with first-class support
//! for `.await`-ing on `wasi::io::poll::Pollable`. It is an ad-hoc implementation
//! tailored for Leptos but it could be exported into a standalone crate.
//!
//! It is based on the `futures` crate's `LocalPool` and makes use of
//! no `unsafe` code.
//!
//! # Performance Notes
//!
//! I haven't benchmarked this runtime but since it makes no use of unsafe code
//! and Rust `core`'s `Context` was prematurely optimised for multi-threading
//! environment, I had no choice but using synchronisation primitives to make
//! the API happy.
//!
//! IIRC, `wasm32` targets have an implementation of synchronisation primitives
//! that are just stubs, downgrading them to their single-threaded counterpart
//! so the overhead should be minimal.
//!
//! Also, you can customise the behaviour of the `Executor` using the
//! `Mode` enum to trade-off reactivity for less host context switch
//! with the `Mode::Stalled` variant.

#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
mod p2 {
    use any_spawner::CustomExecutor;
    use futures::{
        FutureExt, Stream,
        channel::mpsc::{UnboundedReceiver, UnboundedSender},
        executor::{LocalPool, LocalSpawner},
        task::{LocalSpawnExt, SpawnExt},
    };
    use parking_lot::Mutex;
    use std::{
        cell::RefCell,
        future::Future,
        mem,
        rc::Rc,
        sync::{Arc, OnceLock},
        task::{Context, Poll, Wake, Waker},
    };
    use wasi::{
        clocks::monotonic_clock::{Duration, subscribe_duration},
        io::poll::{Pollable, poll},
    };

    struct TableEntry(Pollable, Waker);

    static POLLABLE_SINK: OnceLock<UnboundedSender<TableEntry>> =
        OnceLock::new();

    /// Suspends the current execution context for the specified duration (in nanoseconds).
    ///
    /// This asynchronous function relies on the WASI Preview 2 cooperative polling executor
    /// to register a clock timer and wake up the future when it expires.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use leptos_wasi::executor::sleep;
    ///
    /// # async fn run() {
    /// // Sleep for 1 second (1,000,000,000 nanoseconds)
    /// sleep(1_000_000_000).await;
    /// # }
    /// ```
    pub async fn sleep(duration: Duration) {
        WaitPoll::new(subscribe_duration(duration)).await
    }

    /// A future that resolves when the associated WASI `Pollable` resource becomes ready.
    ///
    /// This structure is used internally by the cooperative executor to block on WASI pollables
    /// (such as I/O streams and timers) within Rust's async model.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use leptos_wasi::executor::WaitPoll;
    /// use wasi::clocks::monotonic_clock::subscribe_duration;
    ///
    /// # async fn run() {
    /// // Wait on a 500ms monotonic clock pollable
    /// let pollable = subscribe_duration(500_000_000);
    /// WaitPoll::new(pollable).await;
    /// # }
    /// ```
    pub struct WaitPoll(WaitPollInner);

    enum WaitPollInner {
        Unregistered(Pollable),
        Registered(Arc<WaitPollWaker>),
    }

    impl WaitPoll {
        /// Creates a new `WaitPoll` future wrapping a WASI `Pollable`.
        pub fn new(pollable: Pollable) -> Self {
            Self(WaitPollInner::Unregistered(pollable))
        }
    }

    impl Future for WaitPoll {
        type Output = ();

        fn poll(
            self: std::pin::Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Self::Output> {
            match &mut self.get_mut().0 {
                this @ WaitPollInner::Unregistered(_) => {
                    let waker = Arc::new(WaitPollWaker::new(cx.waker()));

                    if let Some(sender) = POLLABLE_SINK.get() {
                        if let WaitPollInner::Unregistered(pollable) =
                            mem::replace(
                                this,
                                WaitPollInner::Registered(waker.clone()),
                            )
                        {
                            sender
                                .clone()
                                .unbounded_send(TableEntry(
                                    pollable,
                                    waker.into(),
                                ))
                                .expect("cannot spawn a new WaitPoll");

                            Poll::Pending
                        } else {
                            unreachable!();
                        }
                    } else {
                        panic!(
                            "cannot create a WaitPoll before creating an \
                             Executor"
                        );
                    }
                }
                WaitPollInner::Registered(waker) => {
                    let mut lock = waker.0.lock();
                    if lock.done {
                        Poll::Ready(())
                    } else {
                        // How can it happen?! :O
                        // Well, if, for some reason, the Task get woken up for
                        // another reason than the pollable associated with this
                        // WaitPoll got ready.
                        //
                        // We need to make sure we update the waker.
                        lock.task_waker = cx.waker().clone();
                        Poll::Pending
                    }
                }
            }
        }
    }

    struct WaitPollWaker(Mutex<WaitPollWakerInner>);

    struct WaitPollWakerInner {
        done: bool,
        task_waker: Waker,
    }

    impl WaitPollWaker {
        fn new(waker: &Waker) -> Self {
            Self(Mutex::new(WaitPollWakerInner {
                done: false,
                task_waker: waker.clone(),
            }))
        }
    }

    impl Wake for WaitPollWaker {
        fn wake(self: std::sync::Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            let mut lock = self.0.lock();
            lock.task_waker.wake_by_ref();
            lock.done = true;
        }
    }

    /// Controls how often the `Executor` checks for `Pollable` readiness.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use leptos_wasi::executor::{Executor, Mode};
    ///
    /// // Create an executor that yields control and polls only when stalled
    /// let executor = Executor::new(Mode::Stalled);
    /// ```
    pub enum Mode {
        /// Will check as often as possible for readiness, this have some
        /// performance overhead.
        Premptive,

        /// Will only check for readiness when no more progress can be made
        /// on pooled Futures.
        Stalled,
    }

    /// A single-threaded cooperative future executor for WASI Preview 2.
    ///
    /// It schedules and executes queued futures, blocking on registered WASI pollables
    /// when no further guest-side progress can be made.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use leptos_wasi::executor::{Executor, Mode};
    ///
    /// let executor = Executor::new(Mode::Stalled);
    /// executor.run_until(async {
    ///     println!("Task is running under the WASIp2 executor!");
    /// });
    /// ```
    #[derive(Clone)]
    pub struct Executor(Rc<ExecutorInner>);

    struct ExecutorInner {
        pool: RefCell<LocalPool>,
        spawner: LocalSpawner,
        rx: RefCell<UnboundedReceiver<TableEntry>>,
        mode: Mode,
    }

    impl Executor {
        /// Creates a new cooperative polling executor with the specified `Mode`.
        pub fn new(mode: Mode) -> Self {
            let pool = LocalPool::new();
            let spawner = pool.spawner();
            let (tx, rx) = futures::channel::mpsc::unbounded();

            POLLABLE_SINK
                .set(tx.clone())
                .expect("calling Executor::new two times is not supported");

            Self(Rc::new(ExecutorInner {
                pool: RefCell::new(pool),
                spawner,
                rx: RefCell::new(rx),
                mode,
            }))
        }

        /// Runs the provided future to completion on the current thread, polling and blocking
        /// on WASI resource readiness as needed.
        pub fn run_until<T>(&self, fut: T) -> T::Output
        where
            T: Future + 'static,
        {
            let (tx, mut rx) =
                futures::channel::oneshot::channel::<T::Output>();
            self.spawn_local(Box::pin(fut.then(|val| async move {
                if tx.send(val).is_err() {
                    panic!(
                        "failed to send the return value of the future passed \
                         to run_until"
                    );
                }
            })));

            loop {
                match rx.try_recv() {
                    Err(_) => panic!(
                        "internal error: sender of run until has been dropped"
                    ),
                    Ok(Some(val)) => return val,
                    Ok(None) => {
                        self.poll_local();
                    }
                }
            }
        }
    }

    impl CustomExecutor for Executor {
        fn spawn(&self, fut: any_spawner::PinnedFuture<()>) {
            self.0.spawner.spawn(fut).unwrap();
        }

        fn spawn_local(&self, fut: any_spawner::PinnedLocalFuture<()>) {
            self.0.spawner.spawn_local(fut).unwrap();
        }

        fn poll_local(&self) {
            let mut pool = match self.0.pool.try_borrow_mut() {
                Ok(pool) => pool,
                // Nested call to poll_local(), noop.
                Err(_) => return,
            };

            match self.0.mode {
                Mode::Premptive => {
                    pool.try_run_one();
                }
                Mode::Stalled => pool.run_until_stalled(),
            };

            let (lower, upper) = self.0.rx.borrow().size_hint();
            let capacity = upper.unwrap_or(lower);
            let mut entries = Vec::with_capacity(capacity);
            let mut rx = self.0.rx.borrow_mut();

            while let Ok(entry) = rx.try_recv() {
                entries.push(Some(entry));
            }

            if entries.is_empty() {
                // This could happen if some Futures use Waker that are not
                // registered through [`WaitPoll`] or that we are blocked
                // because some Future returned `Poll::Pending` without
                // actually making sure their Waker is called at some point.
                return;
            }

            let pollables = entries
                .iter()
                .map(|entry| &entry.as_ref().unwrap().0)
                .collect::<Vec<_>>();

            let ready = poll(&pollables);

            if let Some(sender) = POLLABLE_SINK.get() {
                let sender = sender.clone();

                // Wakes futures subscribed to ready pollable.
                for index in ready {
                    let wake = entries[index as usize].take().unwrap().1;
                    wake.wake();
                }

                // Requeue not ready pollable.
                for entry in entries.into_iter().flatten() {
                    sender
                        .unbounded_send(entry)
                        .expect("the sender channel is closed");
                }
            } else {
                unreachable!();
            }
        }
    }
}

#[cfg(all(feature = "wasip2", not(feature = "wasip3")))]
pub use p2::*;

#[cfg(feature = "wasip3")]
mod p3 {
    use any_spawner::CustomExecutor;

    /// A custom executor for WASI Preview 3.
    ///
    /// Under WASIp3, task spawning is handled natively by the host runtime,
    /// so this executor routes task spawns directly to the host's event loop via the
    /// `wasip3::wit_bindgen::spawn` bridge.
    #[derive(Clone, Copy)]
    pub struct Wasip3Executor;

    impl CustomExecutor for Wasip3Executor {
        fn spawn(&self, fut: any_spawner::PinnedFuture<()>) {
            wasip3::wit_bindgen::spawn(fut);
        }

        fn spawn_local(&self, fut: any_spawner::PinnedLocalFuture<()>) {
            wasip3::wit_bindgen::spawn(fut);
        }

        fn poll_local(&self) {
            // No-op under WASIp3 as the host runtime manages execution.
        }
    }

    /// Initializes the global task spawner for WASI Preview 3.
    ///
    /// This registers the `Wasip3Executor` as the default local spawner, enabling
    /// standard Leptos features like `leptos::spawn_local` to execute as native host tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if the spawner has already been initialized with a different executor.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use leptos_wasi::executor::init_wasip3_spawner;
    ///
    /// fn main() {
    ///     init_wasip3_spawner().expect("failed to initialize spawner");
    /// }
    /// ```
    pub fn init_wasip3_spawner() -> Result<(), any_spawner::ExecutorError> {
        static INITIALIZED: std::sync::Once = std::sync::Once::new();
        let mut res = Ok(());
        INITIALIZED.call_once(|| {
            res = any_spawner::Executor::init_local_custom_executor(
                Wasip3Executor,
            );
        });
        res
    }
}

#[cfg(feature = "wasip3")]
pub use p3::*;
