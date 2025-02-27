use crate::{
    error::ToCodeExt,
    prelude::{BusDirection, Error, MediaType},
};
use bitflags::bitflags;

/// A `ComponentHandler` implementation must be passed to all plugin instances, and is used by the
/// plugin to communicate back with the host. All methods are `&self` and `Self must be [Send] and
/// [Sync], since there are no guarantees what the plugin does with this interface.
#[allow(unused_variables)]
pub trait ComponentHandler
where
    Self: Sync + Send + 'static,
{
    /// Called by the plugin when a parameter is about to be changed.
    fn begin_edit(&self, id: u32) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin after a parameter has been changed.
    fn end_edit(&self, id: u32) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin to notify the host the parameter has been changed.
    fn perform_edit(&self, id: u32, value: f64) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin to request the host terminate and reinitialize the component.
    fn restart_component(&self, flags: RestartFlags) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by hte plugin before a series of group parameter changes. The host should keep
    /// a timestamp that is shared by all edits started with `begin_edit`.
    fn start_group_edit(&self) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin to terminate a sequence of edits started with [start_group_edit].
    fn end_group_edit(&self) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin to request the host open the editor view. The "name" field
    /// shall be passed to `create-editor`.
    fn request_open_editor(&self, name: &str) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Indicate to the host that some internal state besides parameters has changed. If `true` the
    /// host should save the plugin's state before exit.
    fn set_dirty(&self, dirty: bool) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin to activate or deactivate a bus.
    fn request_bus_activation(
        &self,
        typ: MediaType,
        dir: BusDirection,
        index: i32,
        state: bool,
    ) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin when a program list has been updated.
    fn notify_program_list_change(&self, list_id: i32, program_index: i32) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin when a module is selected.
    fn notify_unit_selection(&self, unit_id: i32) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }

    /// Called by the plugin when the assignment returned by `get_unit_by_bus` was changed.
    fn notify_unit_by_bus_change(&self) -> Result<(), Error> {
        Err(Error::NotImplemented)
    }
}

bitflags! {
    pub struct RestartFlags: i32 {
        const RELOAD_COMPONENT = vst3::Steinberg::Vst::RestartFlags_::kReloadComponent as _;
        const IO_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kIoChanged as _;
        const PARAM_VALUES_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kParamValuesChanged as _;
        const LATENCY_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kLatencyChanged as _;
        const PARAM_TITLES_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kParamTitlesChanged as _;
        const MIDI_CC_ASSIGNMENTS_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kMidiCCAssignmentChanged as _;
        const NOTE_EXPRESSION_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kNoteExpressionChanged as _;
        const IO_TITLES_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kIoTitlesChanged as _;
        const PREFETCHABLE_SUPPORT_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kPrefetchableSupportChanged as _;
        const ROUTING_INFO_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kRoutingInfoChanged as _;
        const KEYSWITCH_CHANGED = vst3::Steinberg::Vst::RestartFlags_::kKeyswitchChanged as _;
    }
}

pub(crate) struct ComponentHandlerWrapper {
    pub handler: Box<dyn ComponentHandler>,
}

impl vst3::Steinberg::Vst::IComponentHandlerTrait for ComponentHandlerWrapper {
    unsafe fn beginEdit(&self, id: u32) -> vst3::Steinberg::tresult {
        self.handler.begin_edit(id).to_code()
    }

    unsafe fn endEdit(&self, id: u32) -> vst3::Steinberg::tresult {
        self.handler.end_edit(id).to_code()
    }

    unsafe fn performEdit(&self, id: u32, value: f64) -> vst3::Steinberg::tresult {
        self.handler.perform_edit(id, value).to_code()
    }

    unsafe fn restartComponent(&self, flags: vst3::Steinberg::int32) -> vst3::Steinberg::tresult {
        let flags = RestartFlags::from_bits_retain(flags);
        self.handler.restart_component(flags).to_code()
    }
}

impl vst3::Steinberg::Vst::IComponentHandler2Trait for ComponentHandlerWrapper {
    unsafe fn startGroupEdit(&self) -> vst3::Steinberg::tresult {
        self.handler.start_group_edit().to_code()
    }

    unsafe fn finishGroupEdit(&self) -> vst3::Steinberg::tresult {
        self.handler.end_group_edit().to_code()
    }

    unsafe fn requestOpenEditor(
        &self,
        name: vst3::Steinberg::FIDString,
    ) -> vst3::Steinberg::tresult {
        let str = std::ffi::CStr::from_ptr(name.cast()).to_string_lossy();
        self.handler.request_open_editor(str.as_ref()).to_code()
    }

    unsafe fn setDirty(&self, state: vst3::Steinberg::TBool) -> vst3::Steinberg::tresult {
        self.handler
            .set_dirty(state == vst3::Steinberg::kResultTrue as u8)
            .to_code()
    }
}

impl vst3::Steinberg::Vst::IComponentHandlerBusActivationTrait for ComponentHandlerWrapper {
    unsafe fn requestBusActivation(
        &self,
        r#type: vst3::Steinberg::Vst::MediaType,
        dir: vst3::Steinberg::Vst::BusDirection,
        index: vst3::Steinberg::int32,
        state: vst3::Steinberg::TBool,
    ) -> vst3::Steinberg::tresult {
        let typ = match r#type as u32 {
            vst3::Steinberg::Vst::MediaTypes_::kAudio => MediaType::Audio,
            vst3::Steinberg::Vst::MediaTypes_::kEvent => MediaType::Event,
            _ => return vst3::Steinberg::kInvalidArgument,
        };
        let dir = match dir as u32 {
            vst3::Steinberg::Vst::BusDirections_::kInput => BusDirection::Input,
            vst3::Steinberg::Vst::BusDirections_::kOutput => BusDirection::Output,
            _ => return vst3::Steinberg::kInvalidArgument,
        };
        let state = state == (vst3::Steinberg::kResultTrue as u8);
        self.handler
            .request_bus_activation(typ, dir, index, state)
            .to_code()
    }
}

#[allow(non_snake_case)]
impl vst3::Steinberg::Vst::IUnitHandlerTrait for ComponentHandlerWrapper {
    unsafe fn notifyProgramListChange(
        &self,
        listId: vst3::Steinberg::Vst::ProgramListID,
        programIndex: vst3::Steinberg::int32,
    ) -> vst3::Steinberg::tresult {
        self.handler
            .notify_program_list_change(listId, programIndex)
            .to_code()
    }
    unsafe fn notifyUnitSelection(
        &self,
        unitId: vst3::Steinberg::Vst::UnitID,
    ) -> vst3::Steinberg::tresult {
        self.handler.notify_unit_selection(unitId).to_code()
    }
}

impl vst3::Steinberg::Vst::IUnitHandler2Trait for ComponentHandlerWrapper {
    unsafe fn notifyUnitByBusChange(&self) -> vst3::Steinberg::tresult {
        self.handler.notify_unit_by_bus_change().to_code()
    }
}

impl vst3::Class for ComponentHandlerWrapper {
    type Interfaces = (
        vst3::Steinberg::Vst::IComponentHandler,
        vst3::Steinberg::Vst::IComponentHandler2,
        vst3::Steinberg::Vst::IComponentHandlerBusActivation,
        vst3::Steinberg::Vst::IUnitHandler,
        vst3::Steinberg::Vst::IUnitHandler2,
    );
}
