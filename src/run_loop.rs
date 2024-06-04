use either::Either;
use libc::epoll_event;
use std::{
    mem::MaybeUninit,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    ptr::{addr_of, addr_of_mut, null_mut},
    sync::{Arc, RwLock},
    thread::JoinHandle,
};
use vst3::{
    com_scrape_types::SmartPtr,
    ComPtr,
    Steinberg::Linux::{IEventHandler, IEventHandlerTrait, ITimerHandler, ITimerHandlerTrait},
};

pub struct MainThreadEvent {
    context: Either<(ComPtr<IEventHandler>, i32), ComPtr<ITimerHandler>>,
}

impl MainThreadEvent {
    pub fn handle(self) {
        match self.context {
            Either::Left((handler, fd)) => unsafe {
                handler.onFDIsSet(fd);
            },
            Either::Right(handler) => unsafe {
                handler.onTimer();
            },
        }
    }
}

pub(crate) struct RunLoop {
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    main_thread_callback: Box<dyn Fn(MainThreadEvent) + Sync + Send + 'static>,
    handlers: Vec<(i32, Either<ComPtr<IEventHandler>, ComPtr<ITimerHandler>>)>,
    worker_thread: Option<JoinHandle<std::io::Result<()>>>,
    shutdown: bool,
}

impl RunLoop {
    pub fn new(
        main_thread_callback: impl Fn(MainThreadEvent) + Sync + Send + 'static,
    ) -> std::io::Result<Self> {
        let handlers = vec![];
        let inner = Inner {
            main_thread_callback: Box::new(main_thread_callback),
            handlers,
            worker_thread: None,
            shutdown: false,
        };
        let run_loop = Self {
            inner: Arc::new(RwLock::new(inner)),
        };
        let thread = std::thread::spawn({
            let run_loop = run_loop.clone();
            move || {
                run_loop
                    .worker_thread()
                    .inspect_err(|error| eprintln!("run loop failed: {error}"))
            }
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
        let ptr = handler.as_ptr();
        let vtbl = unsafe { (*handler.ptr()).vtbl };
        eprintln!("IEventHandler*: {fd} {ptr:?} {vtbl:?}");
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
            handler_.as_ptr() == handler.as_ptr()
        }) else {
            return;
        };
        inner.handlers.remove(index);
    }

    pub fn register_timer(&self, handler: ComPtr<ITimerHandler>, ms: u64) -> std::io::Result<()> {
        unsafe {
            let fd =
                libc::timerfd_create(libc::CLOCK_REALTIME, libc::TFD_NONBLOCK | libc::TFD_CLOEXEC);
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
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
            handler_.as_ptr() == handler.as_ptr()
        }) else {
            return;
        };
        let (fd, _) = inner.handlers.remove(index);
        unsafe {
            libc::close(fd);
        }
    }

    fn worker_thread(&self) -> std::io::Result<()> {
        unsafe {
            let mut pollfds = vec![];
            loop {
                let inner = self.inner.read().unwrap();
                if inner.shutdown {
                    break;
                }
                pollfds.clear();
                pollfds.reserve(inner.handlers.len());
                for (fd, _) in &inner.handlers {
                    pollfds.push(libc::pollfd {
                        fd: *fd,
                        events: libc::POLLIN | libc::POLLOUT | libc::POLLERR | libc::POLLPRI,
                        revents: 0,
                    });
                }
                drop(inner);
                let nfds = libc::poll(pollfds.as_mut_ptr(), pollfds.len().try_into().unwrap(), 0);
                if nfds < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                for idx in 0..nfds.try_into().unwrap() {
                    let inner = self.inner.read().unwrap();
                    let fd = pollfds[idx].fd;
                    // eprintln!("handling event {fd}, revents: {}", pollfds[idx].revents);
                    let Some(handler) = inner
                        .handlers
                        .iter()
                        .find_map(|(fd_, handler)| (*fd_ == fd).then_some(handler))
                    else {
                        continue;
                    };
                    let context = handler.clone().map_left(|handler| (handler, fd));
                    (inner.main_thread_callback)(MainThreadEvent { context });
                }
            }
            Ok(())
        }
    }
}

impl Clone for RunLoop {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for RunLoop {
    fn drop(&mut self) {
        self.inner.write().unwrap().shutdown = true;
        let thread = self.inner.write().unwrap().worker_thread.take();
        if let Some(thread) = thread {
            thread.join().ok();
            let inner = self.inner.write().unwrap();
            for (fd, handler) in &inner.handlers {
                if handler.is_right() {
                    unsafe {
                        libc::close(*fd);
                    }
                }
            }
        }
    }
}
