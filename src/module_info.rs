use std::str::FromStr;

use serde::Deserialize;
use serde_with::DeserializeFromStr;
use vst3::Steinberg::TUID;

use crate::{error::Error, util::parse_class_id};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ModuleInfo {
    pub classes: Vec<Class>,
    pub name: String,
    #[serde(rename = "Factory Info")]
    pub factory_info: FactoryInfo,
    pub version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Class {
    #[serde(rename = "UPPERCASE")]
    pub cid: CID,
    pub name: String,
    pub category: String,
    pub version: String,
    pub vendor: String,
    #[serde(rename = "SDKVersion")]
    pub sdk_version: String,
    #[serde(default, rename = "Sub Categories")]
    pub subcategories: Vec<String>,
    #[serde(rename = "Class Flags")]
    pub class_flags: i32,
    pub cardinality: i32,
}

#[derive(DeserializeFromStr)]
pub struct CID(pub TUID);

impl FromStr for CID {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(parse_class_id(s)?))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FactoryInfo {
    pub name: String,
    pub version: String,
    #[serde(rename = "URL")]
    pub url: String,
    #[serde(rename = "E-mail")]
    pub email: String,
    pub flags: FactoryFlags,
}

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FactoryFlags {
    pub unicode: bool,
    pub classes_discardable: bool,
    pub component_non_discardable: bool,
    pub license_check: bool,
}

#[derive(Debug, Clone, DeserializeFromStr)]
pub enum ControllerClass {
    ComponentController,
    AudioModule,
    Service,
    Other(String),
}

impl FromStr for ControllerClass {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Component Controller Class" => Ok(Self::ComponentController),
            "Audio Module Class" => Ok(Self::AudioModule),
            "System" => Ok(Self::Service),
            _ => Ok(Self::Other(s.to_owned())),
        }
    }
}
