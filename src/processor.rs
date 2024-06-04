use crate::{
    application::{HostApplication, HostApplicationWrapper},
    component::{BusDirection, MediaType},
    editor::{Editor, StateStream},
    error::{Error, ToResultExt},
    util::ToRustString,
};
use bitflags::bitflags;
use std::{
    mem::{self, MaybeUninit},
    os::raw::c_void,
    ptr::{addr_of_mut, null_mut},
};
use vst3::{
    com_scrape_types::SmartPtr,
    ComPtr, ComWrapper,
    Steinberg::{
        kNotImplemented, kResultFalse, kResultOk, kResultTrue, tresult, FUnknown, FUnknownVtbl,
        IPluginBaseTrait,
        Vst::{
            AudioBusBuffers, BusInfo_::BusFlags_, BusTypes_, Event, IAudioProcessor,
            IAudioProcessorTrait, IComponent, IComponentTrait, IConnectionPoint,
            IConnectionPointTrait, IEventList, IEventListVtbl, IParamValueQueue,
            IParamValueQueueVtbl, IParameterChanges, IParameterChangesVtbl, IoModes_,
            ProcessContext, ProcessModes_, ProcessSetup, SpeakerArrangement,
            SymbolicSampleSizes_::kSample32,
        },
        TUID,
    },
};

/// Wrapper around the audio processor implementation of a plugin.
#[derive(Clone)]
pub struct Processor {
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    pub(crate) connection: Option<ComPtr<IConnectionPoint>>,
}

#[repr(i32)]
pub enum IoMode {
    Simple = IoModes_::kSimple as _,
    Advanced = IoModes_::kAdvanced as _,
    Offline = IoModes_::kOfflineProcessing as _,
}

#[repr(i32)]
pub enum ProcessMode {
    Offline = ProcessModes_::kOffline as _,
    Prefetch = ProcessModes_::kPrefetch as _,
    Realtime = ProcessModes_::kRealtime as _,
}

pub struct BusInfo {
    pub media_type: MediaType,
    pub dir: BusDirection,
    pub channel_count: usize,
    pub name: String,
    pub bus_type: BusType,
    pub flags: BusFlags,
}

pub struct RoutingInfo {
    pub media_type: MediaType,
    pub bus_index: usize,
    pub channel: i32,
}

#[repr(i32)]
pub enum BusType {
    Aux = BusTypes_::kAux as _,
    Main = BusTypes_::kMain as _,
}

impl TryFrom<i32> for BusType {
    type Error = Error;
    fn try_from(value: i32) -> Result<Self, Error> {
        match value as _ {
            BusTypes_::kAux => Ok(Self::Aux),
            BusTypes_::kMain => Ok(Self::Main),
            _ => Err(Error::InvalidArg),
        }
    }
}

bitflags! {
    pub struct BusFlags: u32 {
        const DefaultActive = BusFlags_::kDefaultActive as _;
        const ControlVoltage = BusFlags_::kIsControlVoltage as _;
    }
}

#[repr(C)]
pub struct InputParameterChanges<'a> {
    vtbl: *const IParamValueQueueVtbl,
    points: &'a [(i32, f64)],
    id: u32,
}

pub struct OutputParameterChanges<'a> {
    vtbl: *const IParamValueQueueVtbl,
    points: &'a mut [(i32, f64)],
    len: usize,
    id: u32,
}

#[repr(C)]
struct InputEventList<'a> {
    vtbl: *const IEventListVtbl,
    events: &'a [Event],
}

#[repr(C)]
struct OutputEventList<'a> {
    vtbl: *const IEventListVtbl,
    events: &'a mut [Event],
    len: usize,
}

pub struct ProcessData<'a> {
    /// The process mode.
    pub mode: ProcessMode,

    /// Number of samples in the buffer.
    pub num_samples: usize,

    /// Input audio data.
    pub input_buffers: &'a mut [*mut f32],

    /// Output audio data.
    pub output_buffers: &'a mut [*mut f32],

    /// Input events (offset, Event).
    pub input_events: &'a [Event],

    /// Output events (offset, Event).
    pub output_events: &'a mut [Event],

    /// Input parameter changes.
    pub input_params: &'a [InputParameterChanges<'a>],

    /// Output parameter changes.
    pub output_params: &'a mut [OutputParameterChanges<'a>],

    /// Process context (playback info, tempo, etc).
    pub context: Option<&'a mut ProcessContext>,
}

impl Processor {
    pub(crate) fn new(component: ComPtr<IComponent>) -> Result<Self, Error> {
        let processor = component.cast().ok_or(Error::NoInterface)?;
        let connection = component.cast();
        Ok(Self {
            component,
            processor,
            connection,
        })
    }
}

impl Processor {
    #[cfg(not(target_os = "linux"))]
    pub fn initialize(&self, host: impl HostApplication + 'static) -> Result<(), Error> {
        let host = HostApplicationWrapper::new(host)?;
        let host = ComWrapper::new(host).to_com_ptr().unwrap();
        let ptr = host.ptr();
        unsafe {
            self.component.initialize(ptr).as_result()?;
        }
        mem::forget(host);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    pub fn initialize(
        &self,
        host: impl HostApplication + 'static,
        callback: impl Fn(crate::run_loop::MainThreadEvent) + Send + Sync + 'static,
    ) -> Result<(), Error> {
        let host = HostApplicationWrapper::new(host, callback)?;
        let host = ComWrapper::new(host).to_com_ptr().unwrap();
        let ptr = host.ptr();
        unsafe {
            self.component.initialize(ptr).as_result()?;
        }
        mem::forget(host);
        Ok(())
    }

    pub fn terminate(&self) -> Result<(), Error> {
        unsafe { self.component.terminate().as_result() }
    }

    pub fn set_io_mode(&self, io_mode: IoMode) -> Result<(), Error> {
        unsafe { self.component.setIoMode(io_mode as i32).as_result() }
    }

    pub fn get_bus_count(&self, media_type: MediaType, dir: BusDirection) -> usize {
        unsafe {
            self.component
                .getBusCount(media_type as _, dir as _)
                .try_into()
                .unwrap()
        }
    }

    pub fn get_bus_info(
        &self,
        media_type: MediaType,
        dir: BusDirection,
        index: usize,
    ) -> Result<BusInfo, Error> {
        unsafe {
            let mut info = MaybeUninit::zeroed();
            self.component
                .getBusInfo(
                    media_type as _,
                    dir as _,
                    index.try_into().unwrap(),
                    info.as_mut_ptr(),
                )
                .as_result()?;
            let info = info.assume_init();
            let media_type = info.mediaType.try_into()?;
            let dir = info.direction.try_into()?;
            let channel_count = info.channelCount.try_into().unwrap();
            let bus_type = info.busType.try_into()?;
            let name = (&info.name).to_rust_string();
            let flags = BusFlags::from_bits_retain(info.flags);
            Ok(BusInfo {
                media_type,
                dir,
                channel_count,
                name,
                bus_type,
                flags,
            })
        }
    }

    pub fn set_active(&self, active: bool) -> Result<(), Error> {
        let active = if active {
            kResultTrue as _
        } else {
            kResultFalse as _
        };
        unsafe { self.component.setActive(active).as_result() }
    }

    /// Set the state of the plugin, from a previous call to [Self::get_state]. Not real time safe.
    pub fn set_state(&self, state: &[u8]) -> Result<(), Error> {
        let state = StateStream::from(state);
        let state = ComWrapper::new(state);
        unsafe {
            self.component
                .setState(state.as_com_ref().unwrap().ptr())
                .as_result()?;
        }
        Ok(())
    }

    /// Get the state of the plugin. Not real time safe.
    pub fn get_state(&self) -> Result<Vec<u8>, Error> {
        let state = ComWrapper::new(StateStream::default());
        unsafe {
            let state = state.as_com_ref().unwrap();
            self.component.getState(state.ptr()).as_result()?;
        };
        Ok(state.data())
    }

    /// Connect this plugin to its editor.
    pub fn connect(&self, editor: &Editor) {
        if let (Some(processor), Some(editor)) =
            (self.connection.as_ref(), editor.connection.as_ref())
        {
            unsafe {
                processor.connect(editor.as_ptr());
                editor.connect(processor.as_ptr());
            }
        }
    }

    /// Synchronize this plugins' state with its editor.
    pub fn synchronize(&self, editor: &Editor) {
        if let Ok(state) = self.get_state() {
            editor.set_component_state(&state).ok();
        }
    }
}

impl Processor {
    pub fn set_bus_arrangements(
        &self,
        inputs: &mut [SpeakerArrangement],
        outputs: &mut [SpeakerArrangement],
    ) -> Result<(), Error> {
        let num_inputs = inputs.len().try_into().unwrap();
        let num_outputs = outputs.len().try_into().unwrap();
        let ec = unsafe {
            self.processor.setBusArrangements(
                inputs.as_mut_ptr(),
                num_inputs,
                outputs.as_mut_ptr(),
                num_outputs,
            )
        };
        ec.as_result()
    }

    pub fn get_bus_arrangement(
        &self,
        dir: BusDirection,
        index: usize,
    ) -> Result<SpeakerArrangement, Error> {
        let mut ret = 0;
        let ec = unsafe {
            self.processor.getBusArrangement(
                dir as i32,
                index.try_into().unwrap(),
                addr_of_mut!(ret),
            )
        };
        ec.as_result().map(|()| ret)
    }

    pub fn get_latency_samples(&self) -> u32 {
        unsafe { self.processor.getLatencySamples() }
    }

    pub fn get_tail_samples(&self) -> u32 {
        unsafe { self.processor.getTailSamples() }
    }

    pub fn setup_processing(
        &self,
        process_mode: ProcessMode,
        max_buffer_size: usize,
        sample_rate: f64,
    ) -> Result<(), Error> {
        unsafe {
            let mut setup = ProcessSetup {
                processMode: process_mode as _,
                symbolicSampleSize: kSample32 as _,
                maxSamplesPerBlock: max_buffer_size.try_into().unwrap(),
                sampleRate: sample_rate,
            };
            self.processor
                .setupProcessing(addr_of_mut!(setup))
                .as_result()
        }
    }

    pub fn set_processing(&self, is_processing: bool) -> Result<(), Error> {
        let state = if is_processing {
            kResultTrue
        } else {
            kResultFalse
        };
        let ec = unsafe { self.processor.setProcessing(state as u8) };
        ec.as_result()
    }

    pub fn process(&self, mut context: ProcessData<'_>) -> Result<(), Error> {
        // Create the input buffers.
        let mut input_buffers = AudioBusBuffers {
            numChannels: context.input_buffers.len().try_into().unwrap(),
            silenceFlags: 0,
            __field0: vst3::Steinberg::Vst::AudioBusBuffers__type0 {
                channelBuffers32: context.input_buffers.as_mut_ptr(),
            },
        };

        // Create the output buffers.
        let mut output_buffers = AudioBusBuffers {
            numChannels: context.output_buffers.len().try_into().unwrap(),
            silenceFlags: 0,
            __field0: vst3::Steinberg::Vst::AudioBusBuffers__type0 {
                channelBuffers32: context.output_buffers.as_mut_ptr(),
            },
        };

        // Wrap input/output events.
        let mut input_events = InputEventList::new(context.input_events);
        let mut output_events = OutputEventList::new(context.output_events);

        // Wrap input/output parameter changes.
        let input_parameter_changes = InputParameterChangesInterface::new(context.input_params);
        let mut output_parameter_changes =
            OutputParameterChangesInterface::new(context.output_params);

        // Create the process context.
        let process_context = context
            .context
            .map(|ctx| addr_of_mut!(*ctx))
            .unwrap_or(null_mut());

        // Call the plugin's process function.
        let ec = unsafe {
            let mut data = vst3::Steinberg::Vst::ProcessData {
                processMode: context.mode as i32,
                symbolicSampleSize: kSample32 as _,
                numSamples: context.num_samples.try_into().unwrap(),
                numInputs: context.input_buffers.len().try_into().unwrap(),
                numOutputs: context.output_buffers.len().try_into().unwrap(),
                inputs: addr_of_mut!(input_buffers),
                outputs: addr_of_mut!(output_buffers),
                inputParameterChanges: input_parameter_changes.as_ptr(),
                outputParameterChanges: output_parameter_changes.as_ptr(),
                inputEvents: input_events.as_ptr(),
                outputEvents: output_events.as_ptr(),
                processContext: process_context,
            };
            self.processor.process(addr_of_mut!(data))
        };

        // Truncate the output events.
        let num_output_events = output_events.len;
        context.output_events = &mut context.output_events[0..num_output_events];

        ec.as_result()
    }
}

impl<'a> InputEventList<'a> {
    fn new(events: &'a [Event]) -> Self {
        Self {
            vtbl: &Self::VTBL as *const _,
            events,
        }
    }

    fn as_ptr(&mut self) -> *mut IEventList {
        (self as *mut Self).cast()
    }

    const VTBL: IEventListVtbl = IEventListVtbl {
        base: STACK_OBJECT_FUNKNOWN_VTBL,
        getEvent: Self::get_event,
        getEventCount: Self::get_event_count,
        addEvent: Self::add_event,
    };

    unsafe extern "system" fn get_event(
        this: *mut IEventList,
        index: i32,
        event: *mut Event,
    ) -> tresult {
        let this = &*this.cast::<Self>();
        let index = usize::try_from(index).unwrap();
        if index >= this.events.len() {
            return kResultFalse;
        }
        *event = *this.events.get_unchecked(index);
        kResultOk
    }

    unsafe extern "system" fn get_event_count(this: *mut IEventList) -> i32 {
        let this = &*this.cast::<Self>();
        this.events.len().try_into().unwrap()
    }

    unsafe extern "system" fn add_event(_this: *mut IEventList, _event: *mut Event) -> tresult {
        kNotImplemented
    }
}

impl<'a> OutputEventList<'a> {
    fn new(events: &'a mut [Event]) -> Self {
        Self {
            vtbl: &Self::VTBL as *const _,
            events,
            len: 0,
        }
    }

    fn as_ptr(&mut self) -> *mut IEventList {
        (self as *mut Self).cast()
    }

    const VTBL: IEventListVtbl = IEventListVtbl {
        base: STACK_OBJECT_FUNKNOWN_VTBL,
        getEvent: Self::get_event,
        getEventCount: Self::get_event_count,
        addEvent: Self::add_event,
    };

    unsafe extern "system" fn get_event(
        this: *mut IEventList,
        index: i32,
        event: *mut Event,
    ) -> tresult {
        let this = &*this.cast::<Self>();
        let index = usize::try_from(index).unwrap();
        if index >= this.len {
            return kResultFalse;
        }
        *event = *this.events.get_unchecked(index);
        kResultOk
    }

    unsafe extern "system" fn get_event_count(this: *mut IEventList) -> i32 {
        let this = &*this.cast::<Self>();
        this.len.try_into().unwrap()
    }

    unsafe extern "system" fn add_event(this: *mut IEventList, event: *mut Event) -> tresult {
        let this = &mut *this.cast::<Self>();
        if this.len >= this.events.len() {
            return kResultFalse;
        }
        this.events[this.len] = *event;
        this.len += 1;
        kResultOk
    }
}

#[repr(C)]
struct InputParameterChangesInterface<'a> {
    vtbl: *const IParameterChangesVtbl,
    changes: &'a [InputParameterChanges<'a>],
}

impl<'a> InputParameterChangesInterface<'a> {
    fn new(changes: &'a [InputParameterChanges]) -> Self {
        Self {
            vtbl: (&Self::VTBL as *const _),
            changes,
        }
    }

    fn as_ptr(&self) -> *mut IParameterChanges {
        ((self as *const Self) as *mut Self).cast()
    }

    const VTBL: IParameterChangesVtbl = IParameterChangesVtbl {
        base: STACK_OBJECT_FUNKNOWN_VTBL,
        getParameterCount: Self::get_parameter_count,
        getParameterData: Self::get_parameter_data,
        addParameterData: Self::add_parameter_data,
    };

    unsafe extern "system" fn get_parameter_count(this: *mut IParameterChanges) -> i32 {
        let this = &*this.cast::<Self>();
        this.changes.len().try_into().unwrap()
    }

    unsafe extern "system" fn get_parameter_data(
        this: *mut IParameterChanges,
        index: i32,
    ) -> *mut IParamValueQueue {
        let this = &*this.cast::<Self>();
        let index = usize::try_from(index).unwrap();
        if index >= this.changes.len() {
            return null_mut();
        }
        return this.changes.get_unchecked(index).as_ptr();
    }

    unsafe extern "system" fn add_parameter_data(
        _this: *mut IParameterChanges,
        _id: *const u32,
        _index: *mut i32,
    ) -> *mut IParamValueQueue {
        null_mut()
    }
}

impl<'a> InputParameterChanges<'a> {
    pub fn new(id: u32, points: &'a [(i32, f64)]) -> Self {
        Self {
            vtbl: &Self::VTBL as *const _,
            points,
            id,
        }
    }

    fn as_ptr(&self) -> *mut IParamValueQueue {
        ((self as *const Self) as *mut Self).cast()
    }

    const VTBL: IParamValueQueueVtbl = IParamValueQueueVtbl {
        base: STACK_OBJECT_FUNKNOWN_VTBL,
        getPoint: Self::get_point_impl,
        getPointCount: Self::get_point_count_impl,
        addPoint: Self::add_point_impl,
        getParameterId: Self::get_parameter_id_impl,
    };

    unsafe extern "system" fn get_point_count_impl(this: *mut IParamValueQueue) -> i32 {
        let this = this.cast::<Self>();
        (*this).points.len().try_into().unwrap()
    }

    unsafe extern "system" fn get_point_impl(
        this: *mut IParamValueQueue,
        index: i32,
        offset: *mut i32,
        value: *mut f64,
    ) -> i32 {
        let this = &*this.cast::<Self>();
        let index = usize::try_from(index).unwrap();
        if index >= this.points.len() {
            return kResultFalse;
        }
        let point = this.points.get_unchecked(index);
        *offset = point.0;
        *value = point.1;
        kResultOk
    }

    unsafe extern "system" fn get_parameter_id_impl(this: *mut IParamValueQueue) -> u32 {
        let this = &*this.cast::<Self>();
        this.id
    }

    unsafe extern "system" fn add_point_impl(
        _this: *mut IParamValueQueue,
        _offset: i32,
        _value: f64,
        _index: *mut i32,
    ) -> i32 {
        kNotImplemented
    }
}

#[repr(C)]
struct OutputParameterChangesInterface<'a> {
    vtbl: *const IParameterChangesVtbl,
    changes: &'a mut [OutputParameterChanges<'a>],
    len: usize,
}

impl<'a> OutputParameterChangesInterface<'a> {
    fn new(changes: &'a mut [OutputParameterChanges<'a>]) -> Self {
        Self {
            vtbl: (&Self::VTBL as *const _),
            changes,
            len: 0,
        }
    }

    fn as_ptr(&mut self) -> *mut IParameterChanges {
        (self as *mut Self).cast()
    }

    const VTBL: IParameterChangesVtbl = IParameterChangesVtbl {
        base: STACK_OBJECT_FUNKNOWN_VTBL,
        getParameterCount: Self::get_parameter_count,
        getParameterData: Self::get_parameter_data,
        addParameterData: Self::add_parameter_data,
    };

    unsafe extern "system" fn get_parameter_count(this: *mut IParameterChanges) -> i32 {
        let this = &*this.cast::<Self>();
        this.len.try_into().unwrap()
    }

    unsafe extern "system" fn get_parameter_data(
        this: *mut IParameterChanges,
        index: i32,
    ) -> *mut IParamValueQueue {
        let this = &mut *this.cast::<Self>();
        let index = usize::try_from(index).unwrap();
        if index >= this.len {
            return null_mut();
        }
        return this.changes.get_unchecked_mut(index).as_ptr();
    }

    unsafe extern "system" fn add_parameter_data(
        this: *mut IParameterChanges,
        id: *const u32,
        index: *mut i32,
    ) -> *mut IParamValueQueue {
        let this = &mut *this.cast::<Self>();
        let id = *id;
        if this.len >= this.changes.len() {
            return null_mut();
        }
        let queue = &mut this.changes.get_unchecked_mut(this.len);
        queue.id = id;
        *index = this.len.try_into().unwrap();
        this.len += 1;
        queue.as_ptr()
    }
}

impl<'a> OutputParameterChanges<'a> {
    pub fn new(id: u32, points: &'a mut [(i32, f64)]) -> Self {
        Self {
            vtbl: &Self::VTBL as *const _,
            len: 0,
            points,
            id,
        }
    }

    fn as_ptr(&mut self) -> *mut IParamValueQueue {
        (self as *mut Self).cast()
    }

    const VTBL: IParamValueQueueVtbl = IParamValueQueueVtbl {
        base: STACK_OBJECT_FUNKNOWN_VTBL,
        getPoint: Self::get_point_impl,
        getPointCount: Self::get_point_count_impl,
        addPoint: Self::add_point_impl,
        getParameterId: Self::get_parameter_id_impl,
    };

    unsafe extern "system" fn get_point_count_impl(this: *mut IParamValueQueue) -> i32 {
        let this = this.cast::<Self>();
        (*this).len.try_into().unwrap()
    }

    unsafe extern "system" fn get_point_impl(
        this: *mut IParamValueQueue,
        index: i32,
        offset: *mut i32,
        value: *mut f64,
    ) -> i32 {
        let this = &*this.cast::<Self>();
        let index: usize = index.try_into().unwrap();
        if index >= this.len {
            return kResultFalse;
        }
        let point = this.points.get_unchecked(index);
        *offset = point.0;
        *value = point.1;
        kResultOk
    }

    unsafe extern "system" fn get_parameter_id_impl(this: *mut IParamValueQueue) -> u32 {
        let this = &*this.cast::<Self>();
        this.id
    }

    unsafe extern "system" fn add_point_impl(
        this: *mut IParamValueQueue,
        offset: i32,
        value: f64,
        index: *mut i32,
    ) -> i32 {
        let this = &mut *this.cast::<Self>();
        if this.len >= this.points.len() {
            return kResultFalse;
        }
        this.points[this.len] = (offset, value);
        this.len += 1;
        *index = this.len.try_into().unwrap();
        kResultOk
    }
}

const STACK_OBJECT_FUNKNOWN_VTBL: FUnknownVtbl = FUnknownVtbl {
    queryInterface: query_interface,
    addRef: add_ref,
    release: release,
};

unsafe extern "system" fn query_interface(
    _this: *mut FUnknown,
    _iid: *const TUID,
    _out: *mut *mut c_void,
) -> tresult {
    kNotImplemented
}

unsafe extern "system" fn add_ref(_this: *mut FUnknown) -> u32 {
    debug_assert!(
        false,
        "invalid call to FUnknown::addRef on IParamValueQueue"
    );
    u32::MAX
}

unsafe extern "system" fn release(_this: *mut FUnknown) -> u32 {
    debug_assert!(
        false,
        "invalid call to FUnknown::release on IParamValueQueue"
    );
    u32::MAX
}
