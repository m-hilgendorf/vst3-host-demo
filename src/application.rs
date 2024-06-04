use crate::{error::{Error, ToCodeExt as _}, run_loop::RunLoop};
use std::os::raw::c_void;
use vst3::{
    Class, ComPtr, Steinberg::{
        kInvalidArgument, kResultOk, tresult, Linux::{IEventHandler, IRunLoop, IRunLoopTrait, ITimerHandler}, Vst::{IHostApplication, IHostApplicationTrait, String128}, TUID
    }
};

pub trait HostApplication {
    fn name(&self) -> &str;
}

pub(crate) struct HostApplicationWrapper {
    host: Box<dyn HostApplication>,
    #[cfg(target_os = "linux")]
    run_loop: RunLoop,
}

impl Drop for HostApplicationWrapper {
    fn drop(&mut self) {
        eprintln!("HostApplicationWrapper::drop");
    }
}

impl HostApplicationWrapper {
    #[cfg(not(target_os = "linux"))]
    pub fn new(host: impl HostApplication + 'static) -> Result<Self, Error> {
        let host = Box::new(host);
        Ok(Self { host })
    }

    #[cfg(target_os = "linux")]
    pub fn new(host: impl HostApplication + 'static, callback: impl Fn(crate::run_loop::MainThreadEvent) + Send + Sync + 'static) -> Result<Self, Error> {
        let host = Box::new(host);
        let runloop = RunLoop::new(callback)
            .map_err(|error| {
                tracing::error!(%error, "failed to create run loop");
                Error::Internal
            })?;
        Ok(Self { host, run_loop: runloop })
    }
}

impl IHostApplicationTrait for HostApplicationWrapper {
    unsafe fn createInstance(
        &self,
        _cid: *mut TUID,
        _iid: *mut TUID,
        _obj: *mut *mut c_void,
    ) -> tresult {
        Err(Error::NoInterface).to_code()
    }

    unsafe fn getName(&self, name: *mut String128) -> tresult {
        let name_ = self.host.name().encode_utf16().take(128).enumerate();
        unsafe {
            let ptr = (&mut *name).as_mut_ptr();
            for (n, ch) in name_ {
                *ptr.add(n) = ch as i16;
            }
        }
        0
    }
}

#[cfg(target_os = "linux")]
impl IRunLoopTrait for HostApplicationWrapper {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: vst3::Steinberg::Linux::FileDescriptor,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop
            .register_event_handler(handler, fd)
            .map_err(|error| {
                tracing::error!(%error, "failed to register event handler");
                Error::Internal
            })
            .to_code()
    }

    unsafe fn unregisterEventHandler(
        &self,
        handler: *mut IEventHandler,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop.unregister_event_handler(handler);
        kResultOk
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut ITimerHandler,
        milliseconds: vst3::Steinberg::Linux::TimerInterval,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop
            .register_timer(handler, milliseconds)
            .map_err(|error| {
                tracing::error!(%error, "failed to register timer");
                Error::Internal
            })
            .to_code()
    }

    unsafe fn unregisterTimer(&self, handler: *mut ITimerHandler) -> vst3::Steinberg::tresult {
        let Some(handler) = ComPtr::from_raw(handler) else {
            return kInvalidArgument;
        };
        self.run_loop.unregister_timer(handler);
        kResultOk
    }
}
impl Class for HostApplicationWrapper {
    type Interfaces = (IHostApplication,IRunLoop,);
}
