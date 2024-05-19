use std::cell::RefCell;

use vst3::Steinberg::{tresult, Vst::IParamValueQueueTrait};
use crate::error::{Error, ToCodeExt};


//
pub struct ParamValueQueue {
    id: u32,
    points: RefCell<[(i32, f64)]>,
}



