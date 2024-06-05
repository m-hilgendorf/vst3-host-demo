use crate::error::{Error, ToCodeExt, ToResultExt};
use std::{mem::MaybeUninit, os::raw::c_void};
#[cfg(target_os = "linux")]
use vst3::Steinberg::Linux::{IEventHandler, IRunLoop, IRunLoopTrait, ITimerHandler};
use vst3::{
    Class, ComPtr, ComRef,
    Steinberg::{
        kInvalidArgument, kPlatformTypeHWND, kPlatformTypeNSView, kPlatformTypeX11EmbedWindowID,
        kResultOk, kResultTrue, IPlugFrame, IPlugFrameTrait, IPlugView, IPlugViewTrait, ViewRect,
    },
};
use winit::raw_window_handle::RawWindowHandle;

pub trait PlugFrame {
    fn resize_view(&self, _rect: ViewRect) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }
}

pub(crate) struct PlugFrameWrapper {
    plug_frame: Box<dyn PlugFrame>,
    #[cfg(target_os = "linux")]
    run_loop: crate::run_loop::RunLoop,
}

impl Drop for PlugFrameWrapper {
    fn drop(&mut self) {}
}

impl PlugFrameWrapper {
    pub(crate) fn new(
        plug_frame: impl PlugFrame + 'static,
        #[cfg(target_os = "linux")] run_loop: crate::run_loop::RunLoop,
    ) -> Result<Self, Error> {
        let plug_frame = Box::new(plug_frame);
        Ok(Self {
            plug_frame,
            #[cfg(target_os = "linux")]
            run_loop,
        })
    }
}

impl IPlugFrameTrait for PlugFrameWrapper {
    unsafe fn resizeView(
        &self,
        _view: *mut IPlugView,
        new_size: *mut ViewRect,
    ) -> vst3::Steinberg::tresult {
        self.plug_frame.resize_view(*new_size).to_code()
    }
}

#[cfg(target_os = "linux")]
impl IRunLoopTrait for PlugFrameWrapper {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut IEventHandler,
        fd: vst3::Steinberg::Linux::FileDescriptor,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = ComRef::from_raw(handler) else {
            return kInvalidArgument;
        };
        let handler = handler.to_com_ptr();
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
        let Some(handler) = ComRef::from_raw(handler) else {
            return kInvalidArgument;
        };
        let handler = handler.to_com_ptr();
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

    pub fn attach(&self, window: RawWindowHandle) -> Result<(), Error> {
        match window {
            RawWindowHandle::Win32(win32) => unsafe {
                self.view
                    .attached(win32.hwnd.get() as *mut c_void, kPlatformTypeHWND)
                    .as_result()?;
            },
            RawWindowHandle::AppKit(appkit) => unsafe {
                self.view
                    .attached(appkit.ns_view.as_ptr(), kPlatformTypeNSView)
                    .as_result()?;
            },
            RawWindowHandle::Xcb(xcb) => unsafe {
                let handle = xcb.window.get() as usize as *mut c_void;
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
            _ => {
                return Err(Error::NotImplemented);
            }
        }
        Ok(())
    }

    pub fn removed(&self) {
        unsafe {
            self.view.removed();
        }
    }

    pub fn size(&self) -> Result<ViewRect, Error> {
        unsafe {
            let mut rect = MaybeUninit::zeroed();
            self.view.getSize(rect.as_mut_ptr()).as_result()?;
            Ok(rect.assume_init())
        }
    }

    pub fn is_resizeable(&self) -> bool {
        unsafe { self.view.canResize() == kResultTrue }
    }
}
