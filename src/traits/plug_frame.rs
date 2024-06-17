use crate::{
    error::ToCodeExt,
    prelude::{Error, ViewRect},
};

/// The host must provide a [PlugFrame] implementation when calling [crate::editor::Editor::create_view]
pub trait PlugFrame
where
    Self: 'static,
{
    /// This method is called by the plugin's editor when requesting a resize event from the host.
    #[allow(unused_variables)]
    fn resize_view(&self, rect: ViewRect) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }
}

pub(crate) struct PlugFrameWrapper {
    plug_frame: Box<dyn PlugFrame>,
    #[cfg(target_os = "linux")]
    run_loop: crate::host::run_loop::RunLoop,
}

#[cfg(target_os = "linux")]
impl vst3::Steinberg::Linux::IRunLoopTrait for PlugFrameWrapper {
    unsafe fn registerEventHandler(
        &self,
        handler: *mut vst3::Steinberg::Linux::IEventHandler,
        fd: vst3::Steinberg::Linux::FileDescriptor,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = vst3::ComRef::from_raw(handler) else {
            return vst3::Steinberg::kInvalidArgument;
        };
        self.run_loop
            .register_event_handler(handler.to_com_ptr(), fd)
            .map_err(|error| {
                tracing::error!(%error, "failed to register event handler");
                Error::False
            })
            .to_code()
    }

    unsafe fn registerTimer(
        &self,
        handler: *mut vst3::Steinberg::Linux::ITimerHandler,
        milliseconds: vst3::Steinberg::Linux::TimerInterval,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = vst3::ComRef::from_raw(handler) else {
            return vst3::Steinberg::kInvalidArgument;
        };
        self.run_loop
            .register_timer(handler.to_com_ptr(), milliseconds)
            .map_err(|error| {
                tracing::error!(%error, "failed to register timer handler");
                Error::False
            })
            .to_code()
    }

    unsafe fn unregisterEventHandler(
        &self,
        handler: *mut vst3::Steinberg::Linux::IEventHandler,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = vst3::ComRef::from_raw(handler) else {
            return vst3::Steinberg::kInvalidArgument;
        };
        self.run_loop.unregister_event_handler(handler.to_com_ptr());
        vst3::Steinberg::kResultOk
    }
    unsafe fn unregisterTimer(
        &self,
        handler: *mut vst3::Steinberg::Linux::ITimerHandler,
    ) -> vst3::Steinberg::tresult {
        let Some(handler) = vst3::ComRef::from_raw(handler) else {
            return vst3::Steinberg::kInvalidArgument;
        };
        self.run_loop.unregister_timer(handler.to_com_ptr());
        vst3::Steinberg::kResultOk
    }
}
