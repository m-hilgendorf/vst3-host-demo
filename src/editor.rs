use crate::{
    component::{ComponentHandler, ComponentHandlerWrapper},
    error::{Error, ToResultExt},
    prelude::Host,
    util::ToRustString,
    view::{PlugFrame, PlugFrameWrapper, View},
};
use bitflags::bitflags;
use core::slice;
use std::{
    marker::PhantomData,
    mem::{self, MaybeUninit},
    os::raw::c_void,
    ptr::addr_of_mut,
    sync::Mutex,
};
use vst3::{
    Class, ComPtr, ComWrapper,
    Steinberg::{
        kResultFalse, kResultOk, kResultTrue, tresult, IBStream, IBStreamTrait,
        IBStream_::IStreamSeekMode_,
        IPlugViewTrait,
        Vst::{
            IConnectionPoint, IEditController, IEditController2, IEditController2Trait,
            IEditControllerTrait, KnobModes_, ParameterInfo_::ParameterFlags_,
        },
    },
};

/// Represents the edit controller of a plugin. This type is not `Send` or `Sync`, as each method
/// **MUST** be called on the main thread.
pub struct Editor {
    editor: ComPtr<IEditController>,
    editor2: Option<ComPtr<IEditController2>>,
    pub(crate) connection: Option<ComPtr<IConnectionPoint>>,
    _marker: PhantomData<*mut ()>,
}

#[repr(i32)]
pub enum KnobMode {
    Circular = KnobModes_::kCircularMode as i32,
    Linear = KnobModes_::kLinearMode as i32,
    RelativCircular = KnobModes_::kRelativCircularMode as i32,
}

/// Parameter metadata.
pub struct ParameterInfo {
    pub id: u32,
    pub title: String,
    pub short_title: String,
    pub units: String,
    pub step_count: i32,
    pub default_normalized_value: f64,
    pub unit_id: i32,
    pub flags: ParameterFlags,
}

bitflags! {
    pub struct ParameterFlags: i32 {
        const NO_FLAGS = ParameterFlags_::kNoFlags as _;
        const CAN_AUTOMATE = ParameterFlags_::kCanAutomate as _;
        const IS_READ_ONLY = ParameterFlags_::kIsReadOnly as _;
        const IS_WRAP_AROUND = ParameterFlags_::kIsWrapAround as _;
        const IS_LIST = ParameterFlags_::kIsList as _;
        const IS_HIDDEN = ParameterFlags_::kIsHidden as _;
        const IS_PROGRAM_CHANGE = ParameterFlags_::kIsProgramChange as _;
        const IS_BYPASS = ParameterFlags_::kIsBypass as _;
    }
}

impl Editor {
    pub(crate) fn new(editor: ComPtr<IEditController>) -> Self {
        let editor2 = editor.cast();
        let connection = editor.cast();
        Self {
            editor,
            editor2,
            connection,
            _marker: PhantomData,
        }
    }

    /// Set the state of the plugin's component.
    pub fn set_component_state(&self, state: &[u8]) -> Result<(), Error> {
        let state = StateStream::from(state);
        let state = ComWrapper::new(state);
        unsafe {
            self.editor
                .setComponentState(state.as_com_ref().unwrap().as_ptr())
                .as_result()?;
        }
        Ok(())
    }

    /// Set the state of the plugin's controller, from a previous call to [Self::get_state]. Not real time safe.
    pub fn set_state(&self, state: &[u8]) -> Result<(), Error> {
        let state = StateStream::from(state);
        let state = ComWrapper::new(state);
        let state = state.to_com_ptr().unwrap();
        unsafe { self.editor.setState(state.as_ptr()).as_result() }
    }

    /// Get the state of the plugin's controller. Not real time safe.
    pub fn get_state(&self) -> Result<Vec<u8>, Error> {
        let state = StateStream::default();
        let state = ComWrapper::new(state);
        unsafe {
            let state = state.to_com_ptr().unwrap();
            self.editor.getState(state.as_ptr()).as_result()?;
        };
        let inner = state.inner.lock().unwrap();
        Ok(inner.data.clone())
    }

    /// Get the number of parameters of the plugin.
    pub fn parameter_count(&self) -> i32 {
        unsafe { self.editor.getParameterCount() }
    }

    /// Get the parameter info associated with the index.
    pub fn parameter_info(&self, index: i32) -> Result<ParameterInfo, Error> {
        let info = unsafe {
            let mut info = MaybeUninit::uninit();
            self.editor
                .getParameterInfo(index, info.as_mut_ptr())
                .as_result()?;
            info.assume_init()
        };
        let info = ParameterInfo {
            id: info.id,
            title: (&info.title).to_rust_string(),
            short_title: (&info.shortTitle).to_rust_string(),
            units: (&info.units).to_rust_string(),
            unit_id: info.unitId,
            step_count: info.stepCount,
            default_normalized_value: info.defaultNormalizedValue,
            flags: ParameterFlags::from_bits_retain(info.flags),
        };
        Ok(info)
    }

    /// Convert a normalized paramater value into a displayable string. Not real time safe.
    pub fn stringify_parameter_value(&self, id: u32, value: f64) -> Result<String, Error> {
        let mut buf = [0; 128];
        unsafe {
            self.editor
                .getParamStringByValue(id, value, addr_of_mut!(buf))
                .as_result()?;
        }
        Ok((&buf).to_rust_string())
    }

    /// Denormalize a normalized parameter value.
    pub fn denormalize_parameter_value(&self, id: u32, value: f64) -> f64 {
        unsafe { self.editor.normalizedParamToPlain(id, value) }
    }

    /// Normalize a denormalized parameter value.
    pub fn normalize_parameter_value(&self, id: u32, value: f64) -> f64 {
        unsafe { self.editor.plainParamToNormalized(id, value) }
    }

    /// Create the view object for this plugin.
    pub fn create_view(&self, frame: impl PlugFrame + 'static, host: &Host) -> Result<View, Error> {
        let view_type = c"editor";
        unsafe {
            let iplugview = self.editor.createView(view_type.as_ptr());
            let view: ComPtr<vst3::Steinberg::IPlugView> =
                ComPtr::from_raw(iplugview).ok_or(Error::False)?;
            let frame = ComWrapper::new(PlugFrameWrapper::new(
                frame,
                #[cfg(target_os = "linux")]
                host.run_loop.clone(),
            )?)
            .to_com_ptr()
            .unwrap();
            view.setFrame(frame.as_ptr()).as_result()?;
            std::mem::forget(frame);
            Ok(View::new(view))
        }
    }

    /// Set the plugin knob mode. Returns [Error::NoInterface] if the plugin doesn't implement
    /// `IEditController2`.
    pub fn set_knob_mode(&self, mode: KnobMode) -> Result<(), Error> {
        let editor2 = self.editor2.as_ref().ok_or(Error::NoInterface)?;
        unsafe { editor2.setKnobMode(mode as i32).as_result() }
    }

    /// Open the help menu. Returns [Error::NoInterface] if the plugin doesn't implement
    /// `IEditController2`.
    pub fn open_help(&self, only_check: bool) -> Result<(), Error> {
        let editor2 = self.editor2.as_ref().ok_or(Error::NoInterface)?;
        unsafe {
            editor2
                .openHelp(if only_check {
                    kResultTrue
                } else {
                    kResultFalse
                } as _)
                .as_result()
        }
    }

    /// Open the about box. Returns [Error::NoInterface] if the plugin doesn't implenent
    /// `IEditController2`.
    pub fn open_about_box(&self, only_check: bool) -> Result<(), Error> {
        let editor2 = self.editor2.as_ref().ok_or(Error::NoInterface)?;
        unsafe {
            editor2
                .openAboutBox(if only_check {
                    kResultTrue
                } else {
                    kResultFalse
                } as _)
                .as_result()
        }
    }

    /// Set the component handler for the plugin's editor.
    pub fn set_component_handler(
        &self,
        handler: impl ComponentHandler + 'static,
    ) -> Result<(), Error> {
        let wrapper = ComWrapper::new(ComponentHandlerWrapper {
            handler: Box::new(handler),
        })
        .to_com_ptr()
        .unwrap();
        unsafe {
            self.editor
                .setComponentHandler(wrapper.as_ptr())
                .as_result()?;
        };
        mem::forget(wrapper);
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct StateStream {
    inner: Mutex<StateStreamInner>,
}

#[derive(Default)]
struct StateStreamInner {
    offset: usize,
    data: Vec<u8>,
}

impl StateStream {
    pub fn data(&self) -> Vec<u8> {
        self.inner.lock().unwrap().data.clone()
    }
}

impl From<&[u8]> for StateStream {
    fn from(value: &[u8]) -> Self {
        Self {
            inner: Mutex::new(StateStreamInner {
                offset: 0,
                data: value.to_vec(),
            }),
        }
    }
}

impl Class for StateStream {
    type Interfaces = (IBStream,);
}

#[allow(non_snake_case)]
impl IBStreamTrait for StateStream {
    unsafe fn read(&self, buffer: *mut c_void, numBytes: i32, numBytesRead: *mut i32) -> tresult {
        let mut inner = self.inner.lock().unwrap();
        let read_len = usize::try_from(numBytes).unwrap();
        let inner_len = inner.data.len() - inner.offset;
        let len = read_len.min(inner_len);
        slice::from_raw_parts_mut(buffer.cast::<u8>(), len)
            .copy_from_slice(&inner.data[inner.offset..(inner.offset + len)]);
        *numBytesRead = len.try_into().unwrap();
        inner.offset += len;
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut c_void,
        numBytes: i32,
        numBytesWritten: *mut i32,
    ) -> tresult {
        let mut inner = self.inner.lock().unwrap();
        let slice = slice::from_raw_parts(buffer.cast::<u8>(), numBytes.try_into().unwrap());
        inner.data.extend_from_slice(slice);
        inner.offset += slice.len();
        if !numBytesWritten.is_null() {
            *numBytesWritten = slice.len().try_into().unwrap();
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: i64, mode: i32, result: *mut i64) -> tresult {
        let mut inner = self.inner.lock().unwrap();
        inner.offset = match mode as u32 {
            IStreamSeekMode_::kIBSeekCur => {
                (inner.offset + usize::try_from(pos).unwrap()).min(inner.data.len())
            }
            IStreamSeekMode_::kIBSeekEnd => {
                inner.data.len().saturating_sub(pos.try_into().unwrap())
            }
            IStreamSeekMode_::kIBSeekSet => {
                if pos < 0 || usize::try_from(pos).unwrap() >= inner.data.len() {
                    return Error::InvalidArg as _;
                }
                usize::try_from(pos).unwrap()
            }
            _ => return Error::InvalidArg as _,
        };

        unsafe {
            *result = inner.offset.try_into().unwrap();
        };
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut i64) -> tresult {
        let inner = self.inner.lock().unwrap();
        *pos = inner.offset.try_into().unwrap();
        kResultOk
    }
}
