use alloc::fmt;
use core::str::FromStr;

use serde::Serialize;

use crate::JevError;

/// The models validated against the labeled evals for each provider, default first. Add a model
/// only after an eval run on it.
const OPENROUTER_MODELS: [&str; 1] = ["typesafe/jev-1.13-20260917"];
const TYPESAFE_MODELS: [&str; 1] = ["jev-1.13.0"];

/// A decision-model provider and its endpoint, models and key variable.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    OpenRouter,
    TypeSafe,
}

impl Provider {
    /// The models validated against the labeled evals for this provider, default first.
    #[inline]
    #[must_use]
    pub const fn list_supported_models(self) -> &'static [&'static str] {
        match self {
            Self::OpenRouter => &OPENROUTER_MODELS,
            Self::TypeSafe => &TYPESAFE_MODELS,
        }
    }

    /// The model a search uses when none is requested.
    #[inline]
    #[must_use]
    pub const fn read_default_model(self) -> &'static str {
        match self {
            Self::OpenRouter => OPENROUTER_MODELS[0],
            Self::TypeSafe => TYPESAFE_MODELS[0],
        }
    }

    /// The provider's decision endpoint URL.
    #[inline]
    #[must_use]
    pub const fn read_endpoint(self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/alpha/decisions",
            Self::TypeSafe => "https://api.typesafe.ai/v1/systemone",
        }
    }

    /// The environment variable that holds the provider's API key.
    #[inline]
    #[must_use]
    pub const fn read_key_variable(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::TypeSafe => "TYPESAFE_API_KEY",
        }
    }

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

impl fmt::Display for Provider {
    #[inline]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::OpenRouter => "openrouter",
            Self::TypeSafe => "typesafe",
        })
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
