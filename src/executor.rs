use crate::reactor::Reactor;
use futures::future::LocalBoxFuture;
use futures::{Future, FutureExt};
use std::marker::PhantomData;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, task::Context};

scoped_tls::scoped_thread_local!(static EX: Executor);

#[inline]
pub(crate) fn get_reactor() -> Rc<RefCell<Reactor>> {
    EX.with(|ex| ex.reactor.clone())
}

pub struct Executor {
    task_queue: TaskQueue,
    reactor: Rc<RefCell<Reactor>>,
    _marker: PhantomData<Rc<()>>,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            task_queue: TaskQueue::default(),
            reactor: Rc::new(RefCell::new(Reactor::default())),
            _marker: PhantomData,
        }
    }

    pub fn spawn(f: impl Future<Output = ()> + 'static) {
        println!("[executor] spawn task");
        let t = Rc::new(Task {
            future: RefCell::new(f.boxed_local()),
        });
        EX.with(|ex| ex.task_queue.push(t))
    }

    pub fn block_on<F, T, O>(&self, f: F) -> O
    where
        F: Fn() -> T,
        T: Future<Output = O>,
    {
        println!("[executor] block on...");
        let _waker = waker_fn::waker_fn(|| {});
        let cx = &mut Context::from_waker(&_waker);
        EX.set(self, || {
            let fut = f();
            pin_utils::pin_mut!(fut);

            loop {
                println!("[executor] start poll outer future");
                if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                    println!("[executor] finish poll outer future get res");
                    break res;
                }
                println!("[executor] finish poll outer future");

                while let Some(task) = self.task_queue.pop() {
                    println!("[executor] start pull task");
                    let mut future = task.future.borrow_mut();
                    let waker = waker(task.clone());
                    let mut cx = Context::from_waker(&waker);
                    let _ = future.as_mut().poll(&mut cx);
                    println!("[executor] finish pull task");
                }

                println!("[executor] start pull outer future");
                if let Poll::Ready(res) = fut.as_mut().poll(cx) {
                    break res;
                }
                println!("[executor] finish pull outer future");

                self.reactor.borrow_mut().wait();
            }
        })
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

struct TaskQueue {
    queue: RefCell<VecDeque<Rc<Task>>>,
}

impl TaskQueue {
    pub fn new() -> TaskQueue {
        const DEFAULT_TASK_QUEUE_SIZE: usize = 2048;
        Self::new_with_capacity(DEFAULT_TASK_QUEUE_SIZE)
    }

    pub fn new_with_capacity(capacity: usize) -> TaskQueue {
        TaskQueue {
            queue: RefCell::new(VecDeque::with_capacity(capacity)),
        }
    }

    pub(crate) fn push(&self, task: Rc<Task>) {
        self.queue.borrow_mut().push_back(task)
    }

    pub(crate) fn pop(&self) -> Option<Rc<Task>> {
        self.queue.borrow_mut().pop_front()
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

struct Task {
    future: RefCell<LocalBoxFuture<'static, ()>>,
}

impl RcWake for Task {
    fn wake_by_ref(arc_self: &Rc<Self>) {
        EX.with(|ex| ex.task_queue.push(arc_self.clone()))
    }
}

use std::mem::{self, ManuallyDrop};
use std::task::{Poll, RawWaker, RawWakerVTable, Waker};

pub trait RcWake {
    fn wake(self: Rc<Self>) {
        Self::wake_by_ref(&self)
    }

    fn wake_by_ref(arc_self: &Rc<Self>);
}

fn waker<W>(wake: Rc<W>) -> Waker
where
    W: RcWake,
{
    let ptr = Rc::into_raw(wake) as *const ();
    let vtable = &Helper::<W>::VTABLE;
    unsafe { Waker::from_raw(RawWaker::new(ptr, vtable)) }
}

#[allow(clippy::redundant_clone)] // The clone here isn't actually redundant.
unsafe fn increase_refcount<T: RcWake>(data: *const ()) {
    // Retain Arc, but don't touch refcount by wrapping in ManuallyDrop
    let arc = mem::ManuallyDrop::new(Rc::<T>::from_raw(data as *const T));
    // Now increase refcount, but don't drop new refcount either
    let _arc_clone: mem::ManuallyDrop<_> = arc.clone();
}

struct Helper<F>(F);

impl<T: RcWake> Helper<T> {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::clone_waker,
        Self::wake,
        Self::wake_by_ref,
        Self::drop_waker,
    );

    unsafe fn clone_waker(data: *const ()) -> RawWaker {
        increase_refcount::<T>(data);
        let vtable = &Helper::<T>::VTABLE;
        RawWaker::new(data, vtable)
    }

    unsafe fn wake(ptr: *const ()) {
        let rc: Rc<T> = Rc::from_raw(ptr as *const T);
        RcWake::wake(rc);
    }

    unsafe fn wake_by_ref(ptr: *const ()) {
        let arc = ManuallyDrop::new(Rc::<T>::from_raw(ptr as *const T));
        RcWake::wake_by_ref(&arc);
    }

    unsafe fn drop_waker(ptr: *const ()) {
        drop(Rc::from_raw(ptr as *const Task));
    }
}
