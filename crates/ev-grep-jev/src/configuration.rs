use core::str::FromStr;

use crate::{JevError, Provider};

impl Provider {
    /// The requested model, or the default, when the supported-model table lists it.
    #[inline]
    pub fn select_model(self, requested: Option<&str>) -> Result<String, JevError> {
        let model = requested.unwrap_or_else(|| self.read_default_model());
        if !self.list_supported_models().contains(&model) {
            return Err(JevError::UnsupportedModel {
                model: model.to_owned(),
                provider: self,
            });
        }
        Ok(model.to_owned())
    }
}

impl FromStr for Provider {
    type Err = JevError;

    #[inline]
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openrouter" => Ok(Self::OpenRouter),
            "typesafe" => Ok(Self::TypeSafe),
            _ => Err(JevError::UnknownProvider),
        }
    }
}
