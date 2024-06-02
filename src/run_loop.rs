use either::Either;
use libc::epoll_event;
use std::{
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr::{addr_of, addr_of_mut, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, RwLock,
    },
    thread::JoinHandle,
};
use vst3::{
    com_scrape_types::SmartPtr,
    ComPtr,
    Steinberg::Linux::{IEventHandler, IEventHandlerTrait, ITimerHandler, ITimerHandlerTrait},
};

pub struct RunLoop {
    inner: Arc<RwLock<Inner>>,
}

impl Clone for RunLoop {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

struct Inner {
    epollfd: OwnedFd,
    handlers: Vec<(i32, Either<ComPtr<IEventHandler>, ComPtr<ITimerHandler>>)>,
    worker_thread: Option<JoinHandle<std::io::Result<()>>>,
    shutdown: bool,
}

enum Message {
    Add(i32),
    Remove(i32),
}

impl RunLoop {
    pub fn new() -> std::io::Result<Self> {
        let epollfd = unsafe {
            let fd = libc::epoll_create1(0);
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            OwnedFd::from_raw_fd(fd)
        };
        let handlers = vec![];
        let inner = Inner {
            epollfd,
            handlers,
            worker_thread: None,
            shutdown: false,
        };
        let run_loop = Self {
            inner: Arc::new(RwLock::new(inner)),
        };
        let thread = std::thread::spawn({
            let run_loop = run_loop.clone();
            move || run_loop.worker_thread()
        });
        run_loop
            .inner
            .write()
            .unwrap()
            .worker_thread
            .replace(thread);
        Ok(run_loop)
    }

    pub fn register_event_handler(
        &self,
        handler: ComPtr<IEventHandler>,
        fd: i32,
    ) -> std::io::Result<()> {
        self.register_fd(fd)?;
        self.inner
            .write()
            .unwrap()
            .handlers
            .push((fd, Either::Left(handler)));
        Ok(())
    }

    pub fn unregister_event_handler(&self, handler: ComPtr<IEventHandler>) {
        let mut inner = self.inner.write().unwrap();
        let Some(index) = inner.handlers.iter().position(|(_, handler_)| {
            let Either::Left(handler_) = handler_ else {
                return false;
            };
            handler_.ptr() == handler.ptr()
        }) else {
            return;
        };
        let (fd, _) = inner.handlers.remove(index);
        drop(inner);
        self.unregister_fd(fd).ok();
    }

    pub fn register_timer(&self, handler: ComPtr<ITimerHandler>, ms: u64) -> std::io::Result<()> {
        unsafe {
            let fd =
                libc::timerfd_create(libc::CLOCK_REALTIME, libc::TFD_NONBLOCK | libc::TFD_CLOEXEC);
            if fd < 0 {
                return (Err(std::io::Error::last_os_error()));
            }
            let value = libc::itimerspec {
                it_interval: libc::timespec {
                    tv_sec: 0,
                    tv_nsec: (1000 * ms) as _,
                },
                it_value: libc::timespec {
                    tv_sec: 0,
                    tv_nsec: 0,
                },
            };
            let ec = libc::timerfd_settime(fd, 0, addr_of!(value), null_mut());
            if ec < 0 {
                return Err(std::io::Error::last_os_error());
            }
            self.register_fd(fd)?;
            self.inner
                .write()
                .unwrap()
                .handlers
                .push((fd, Either::Right(handler)));
        }
        Ok(())
    }

    pub fn unregister_timer(&self, handler: ComPtr<ITimerHandler>) {
        let mut inner = self.inner.write().unwrap();
        let Some(index) = inner.handlers.iter().position(|(_, handler_)| {
            let Either::Right(handler_) = handler_ else {
                return false;
            };
            handler_.ptr() == handler.ptr()
        }) else {
            return;
        };
        let (fd, _) = inner.handlers.remove(index);
        drop(inner);
        unsafe {
            libc::close(fd);
        }
        self.unregister_fd(fd).ok();
    }

    fn register_fd(&self, fd: i32) -> std::io::Result<()> {
        unsafe {
            #[repr(C)]
            union U {
                fd: i32,
                u64: u64,
            };
            let u = U { fd };
            let mut ev = epoll_event {
                events: libc::EPOLLIN as _,
                u64: u.u64,
            };
            let inner = self.inner.write().unwrap();
            if libc::epoll_ctl(
                inner.epollfd.as_raw_fd(),
                libc::EPOLL_CTL_ADD,
                fd,
                addr_of_mut!(ev),
            ) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn unregister_fd(&self, fd: i32) -> std::io::Result<()> {
        unsafe {
            #[repr(C)]
            union U {
                fd: i32,
                u64: u64,
            };
            let u = U { fd };
            let mut ev = epoll_event {
                events: libc::EPOLLIN as _,
                u64: u.u64,
            };
            let inner = self.inner.write().unwrap();
            if libc::epoll_ctl(
                inner.epollfd.as_raw_fd(),
                libc::EPOLL_CTL_DEL,
                fd,
                addr_of_mut!(ev),
            ) < 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        }
    }

    fn worker_thread(&self) -> std::io::Result<()> {
        unsafe {
            let mut events: [libc::epoll_event; 64] = MaybeUninit::zeroed().assume_init();
            loop {
                let inner = self.inner.read().unwrap();
                if inner.shutdown {
                    break;
                }
                let nfds = libc::epoll_wait(
                    inner.epollfd.as_raw_fd(),
                    events.as_mut_ptr(),
                    events.len().try_into().unwrap(),
                    1,
                );
                if nfds < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for idx in 0..nfds.try_into().unwrap() {
                    union U {
                        fd: i32,
                        u64: u64,
                    };
                    let u = U {
                        u64: events[idx].u64,
                    };
                    let Some(handler) = inner
                        .handlers
                        .iter()
                        .find_map(|(fd, handler)| (*fd == u.fd).then_some(handler))
                    else {
                        continue;
                    };
                    match handler {
                        Either::Left(handler) => {
                            handler.onFDIsSet(u.fd);
                        }
                        Either::Right(handler) => {
                            handler.onTimer();
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

impl Drop for RunLoop {
    fn drop(&mut self) {
        self.inner.write().unwrap().shutdown = true;
        let mut inner = self.inner.write().unwrap();
        if let Some(thread) = inner.worker_thread.take() {
            thread.join().ok();
        }
        for (fd, handler) in &inner.handlers {
            if handler.is_right() {
                unsafe {
                    libc::close(*fd);
                }
            }
        }
    }
}
