use core::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use vst3::Steinberg::{
    PFactoryInfo_::FactoryFlags_::{
        kClassesDiscardable, kComponentNonDiscardable, kLicenseCheck, kUnicode,
    },
    TUID,
};

use crate::{error::Error, util::parse_class_id};

/// Deserialization of a plugin's moduleinfo.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Info {
    pub classes: Vec<Class>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(rename = "Factory Info")]
    pub factory_info: FactoryInfo,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Class {
    #[serde(rename = "CID")]
    pub cid: CID,
    pub name: String,
    pub category: String,
    pub cardinality: i32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,

    #[serde(
        rename = "SDKVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub sdk_version: Option<String>,

    #[serde(
        rename = "Sub Categories",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub subcategories: Vec<String>,

    #[serde(
        rename = "Class Flags",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub class_flags: Option<u32>,
}

#[derive(Debug, Copy, Clone, Serialize, DeserializeFromStr)]
pub struct CID(pub TUID);

impl FromStr for CID {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(parse_class_id(s)?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FactoryInfo {
    pub vendor: String,
    #[serde(rename = "URL")]
    pub url: String,
    #[serde(rename = "E-Mail")]
    pub email: String,
    pub flags: FactoryFlags,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct FactoryFlags {
    #[serde(default)]
    pub unicode: bool,

    #[serde(default, rename = "Classes Discardable")]
    pub classes_discardable: bool,

    #[serde(default, rename = "Component Non Discardable")]
    pub component_non_discardable: bool,

    #[serde(default, rename = "License Check")]
    pub license_check: bool,
}

#[derive(Debug, Clone, SerializeDisplay, DeserializeFromStr)]
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

impl fmt::Display for ControllerClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComponentController => write!(f, "Component Controller Class"),
            Self::AudioModule => write!(f, "Audio Module Class"),
            Self::Service => write!(f, "System"),
            Self::Other(other) => write!(f, "{other}"),
        }
    }
}

impl From<i32> for FactoryFlags {
    fn from(value: i32) -> Self {
        let unicode = (value & (kUnicode as i32)) == 0;
        let classes_discardable = (value & (kClassesDiscardable as i32)) == 0;
        let component_non_discardable = (value & (kComponentNonDiscardable as i32)) == 0;
        let license_check = (value & (kLicenseCheck as i32)) == 0;
        Self {
            unicode,
            classes_discardable,
            component_non_discardable,
            license_check,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Info;

    #[test]
    fn parse() {
        let moduleinfo_json = include_str!("../../tests/moduleinfo.json");
        let info: Info = json5::from_str(&moduleinfo_json).unwrap();
        println!("{info:#?}");
    }
}
