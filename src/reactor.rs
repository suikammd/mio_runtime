use mio::unix::SourceFd;
use mio::{event::Event, Events, Interest, Poll, Token};
use std::{
    os::unix::prelude::RawFd,
    task::{Context, Waker},
};

struct Completion {
    waker: Option<Waker>,
}

impl Completion {
    pub fn new(waker: Waker) -> Self {
        Self { waker: Some(waker) }
    }

    fn complete(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake()
        }
    }
}

pub(crate) struct Reactor {
    poller: Poll,
    completion: rustc_hash::FxHashMap<usize, Completion>,
    events: Events,
}

impl Reactor {
    fn new() -> Self {
        Self {
            poller: Poll::new().unwrap(),
            completion: rustc_hash::FxHashMap::default(),
            events: Events::with_capacity(1024),
        }
    }

    pub fn add(&self, fd: RawFd) {
        println!("[reactor] add fd: {}", fd);
        let flags =
            nix::fcntl::OFlag::from_bits(nix::fcntl::fcntl(fd, nix::fcntl::F_GETFL).unwrap())
                .unwrap();
        let flags_nonblocking = flags | nix::fcntl::OFlag::O_NONBLOCK;
        nix::fcntl::fcntl(fd, nix::fcntl::F_SETFL(flags_nonblocking)).unwrap();

        self.poller
            .registry()
            .register(
                &mut SourceFd(&fd),
                Token(fd as usize),
                Interest::READABLE | Interest::WRITABLE,
            )
            .unwrap();
    }

    pub fn wait(&mut self) {
        println!("[reactor wait] start to loop events");
        if let Err(e) = self.poller.poll(&mut self.events, None) {
            println!("[reactor] wait, get error {}", e);
            return;
        }

        for event in self.events.iter() {
            if event.is_readable() {
                println!("[reactor wait] event {} is readable", event.token().0);
                if let Some(mut c) = self.completion.remove(&(event.token().0 * 2)) {
                    println!(
                        "[reactor token] token {:?} read completion removed and woken",
                        event.token()
                    );
                    c.complete()
                }
            }

            if event.is_writable() {
                println!("[reactor wait] event {} is writable", event.token().0);
                if let Some(mut c) = self.completion.remove(&(event.token().0 * 2 + 1)) {
                    println!(
                        "[reactor token] token {:?} write completion removed and woken",
                        event.token()
                    );
                    c.complete()
                }
            }
        }
    }

    pub fn modify_readable(&mut self, fd: RawFd, cx: &Context) {
        println!("[reactor] modify_readable fd {}", fd);
        self.push_completion(2 * fd as usize, cx);
    }

    pub fn modify_writable(&mut self, fd: RawFd, cx: &Context) {
        println!("[reactor] modify_writable fd {}", fd);
        self.push_completion(2 * fd as usize + 1, cx);
    }

    pub fn delete(&mut self, fd: RawFd) {
        println!("[reactor] delete fd {}", fd);

        self.completion.remove(&(fd as usize * 2));
        println!("[reactor] complete remove read fd {}", fd);

        self.completion.remove(&(fd as usize * 2 + 1));
        println!("[reactor] complete remove write fd {}", fd);
    }

    fn push_completion(&mut self, key: usize, cx: &Context) {
        self.completion.insert(
            key,
            Completion {
                waker: Some(cx.waker().clone()),
            },
        );
    }
}

impl Default for Reactor {
    fn default() -> Self {
        Self::new()
    }
}
