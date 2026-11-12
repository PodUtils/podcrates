//! The hints of a new WARP device (PodUtils/podnet issue 141): a name, a
//! model and a locale. The caller fills them (podnet reads
//! `PODNET_WARP_DEVICE_NAME`, `PODNET_WARP_DEVICE_MODEL` and
//! `PODNET_WARP_DEVICE_LOCALE`); the crate reads no variable.

use pod_core::{Class, Failure, Stage};

/// The name, the model and the locale that a new device declares. The
/// default gives podnet's body: model `PC`, locale `en_US`, no name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceHints {
    pub name: Option<String>,
    pub model: Option<String>,
    pub locale: Option<String>,
}

impl DeviceHints {
    /// A name of 1 to 64 printable characters, a model of 1 to 32, a
    /// locale as `ll_CC`.
    pub fn check(&self) -> Result<(), Failure> {
        let bad = |text: String| Err(Failure::new(Stage::Warp, Class::Usage, text));
        let printable = |s: &str| s.chars().all(|c| !c.is_control());
        if let Some(name) = &self.name
            && (name.is_empty() || name.chars().count() > 64 || !printable(name))
        {
            return bad(format!("the device name has 1 to 64 printable characters: {name:?}"));
        }
        if let Some(model) = &self.model
            && (model.is_empty() || model.chars().count() > 32 || !printable(model))
        {
            return bad(format!("the device model has 1 to 32 printable characters: {model:?}"));
        }
        if let Some(locale) = &self.locale {
            let b = locale.as_bytes();
            let ok = b.len() == 5
                && b[..2].iter().all(u8::is_ascii_lowercase)
                && b[2] == b'_'
                && b[3..].iter().all(u8::is_ascii_uppercase);
            if !ok {
                return bad(format!("the device locale is as en_US: {locale:?}"));
            }
        }
        Ok(())
    }
}
