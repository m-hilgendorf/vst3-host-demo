use vst3::Steinberg::Vst::{IUnitHandler2Trait, IUnitHandlerTrait};

use crate::error::{Error, ToCodeExt};

#[allow(unused_variables)]
pub trait UnitHandler {
    /// Called by the plugin when a program list has been updated.
    fn notify_program_list_change(&self, list_id: i32, program_index: i32) -> Result<(), Error>{
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

impl IUnitHandlerTrait for &dyn UnitHandler {
    unsafe fn notifyProgramListChange(&self,listId:vst3::Steinberg::Vst::ProgramListID,programIndex:vst3::Steinberg::int32,) -> vst3::Steinberg::tresult {
        self.notify_program_list_change(listId, programIndex).to_code()
    }
    unsafe fn notifyUnitSelection(&self,unitId:vst3::Steinberg::Vst::UnitID,) -> vst3::Steinberg::tresult {
        self.notify_unit_selection(unitId).to_code()
    }
}

impl IUnitHandler2Trait for &dyn UnitHandler {
    unsafe fn notifyUnitByBusChange(&self) -> vst3::Steinberg::tresult {
        self.notify_unit_by_bus_change().to_code()
    }
}