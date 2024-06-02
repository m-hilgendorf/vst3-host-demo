use std::{mem::MaybeUninit, os::raw::c_void};

use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};
use vst3::{
    Class, ComPtr,
    Steinberg::{
        kInvalidArgument, kPlatformTypeHWND, kPlatformTypeNSView, kPlatformTypeX11EmbedWindowID,
        kResultOk, IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, ViewRect,
    },
};

#[cfg(target_os = "linux")]
use vst3::Steinberg::Linux::{IEventHandler, IRunLoop, IRunLoopTrait, ITimerHandler};

use crate::error::{Error, ToCodeExt, ToResultExt};

pub trait PlugFrame {
    fn resize_view(&self, rect: ViewRect) -> Result<(), Error>;

    #[cfg(target_os = "linux")]
    fn register_event_handler(
        &self,
        event_handler: ComPtr<IEventHandler>,
        fd: i32,
    ) -> Result<(), Error>;

    #[cfg(target_os = "linux")]
    fn unregister_event_handler(&self, event_handler: ComPtr<IEventHandler>) -> Result<(), Error>;

    #[cfg(target_os = "linux")]
    fn register_timer(&self, timer: ComPtr<ITimerHandler>, ms: u64) -> Result<(), Error>;

    #[cfg(target_os = "linux")]
    fn unregister_timer(&self, timer: ComPtr<ITimerHandler>) -> Result<(), Error>;
}

pub(crate) struct PlugFrameWrapper {
    plug_frame: Box<dyn PlugFrame>,
    #[cfg(target_os = "linux")]
    run_loop: crate::run_loop::RunLoop,
}

impl IPlugFrameTrait for PlugFrameWrapper {
    unsafe fn resizeView(
        &self,
        _view: *mut IPlugView,
        newSize: *mut ViewRect,
    ) -> vst3::Steinberg::tresult {
        self.plug_frame.resize_view(*newSize).to_code()
    }
}

#[cfg(target_os = "linux")]
impl IRunLoopTrait for PlugFrameWrapper {
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

impl Class for PlugFrameWrapper {
    #[cfg(not(target_os = "linux"))]
    type Interfaces = (IPlugFrame,);

    #[cfg(target_os = "linux")]
    type Interfaces = (IPlugFrame, IRunLoop);
}

pub struct View {
    view: ComPtr<IPlugView>,
}

impl View {
    pub(crate) fn new(view: ComPtr<IPlugView>) -> Self {
        Self { view }
    }

    pub fn attach(&self, window: &impl HasRawWindowHandle) -> Result<(), Error> {
        match window.raw_window_handle() {
            RawWindowHandle::Win32(win32) => unsafe {
                self.view
                    .attached(win32.hwnd, kPlatformTypeHWND)
                    .as_result()?;
            },
            RawWindowHandle::AppKit(appkit) => unsafe {
                self.view
                    .attached(appkit.ns_view, kPlatformTypeNSView)
                    .as_result()?;
            },
            RawWindowHandle::Xcb(xcb) => unsafe {
                let handle = xcb.window as usize as *mut c_void;
                self.view
                    .attached(handle, kPlatformTypeX11EmbedWindowID)
                    .as_result()?;
            },
            RawWindowHandle::Xlib(xlib) => unsafe {
                let handle = xlib.window as usize as *mut c_void;
                self.view
                    .attached(handle, kPlatformTypeX11EmbedWindowID)
                    .as_result()?;
            },

            other => {
                eprintln!("window handle: {other:?}");
                return Err(Error::NotImplemented);
            }
        }
        Ok(())
    }

    pub fn size(&self) -> Result<baseview::Size, Error> {
        unsafe {
            let mut rect = MaybeUninit::zeroed();
            self.view.getSize(rect.as_mut_ptr()).as_result()?;
            let rect = rect.assume_init();
            let width = (rect.right - rect.left) as _;
            let height = (rect.bottom - rect.top) as _;
            Ok(baseview::Size { width, height })
        }
    }
}
