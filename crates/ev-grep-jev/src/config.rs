use std::{fmt, str::FromStr};

use serde::Serialize;

use crate::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    OpenRouter,
    TypeSafe,
}

impl Provider {
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/alpha/decisions",
            Self::TypeSafe => "https://api.typesafe.ai/v1/systemone",
        }
    }

    /// The models validated against the labeled evals for this provider, default first. Add a model only after an
    /// eval run on it.
    pub fn supported_models(self) -> &'static [&'static str] {
        match self {
            Self::OpenRouter => &["typesafe/jev-1.13-20260917"],
            Self::TypeSafe => &["jev-1.13.0"],
        }
    }

    pub fn default_model(self) -> &'static str {
        self.supported_models()[0]
    }

    pub fn key_variable(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::TypeSafe => "TYPESAFE_API_KEY",
        }
    }

    /// The requested model, or the default, when the supported-model table lists it.
    pub fn model(self, requested: Option<&str>) -> Result<String, Error> {
        let model = requested.unwrap_or(self.default_model());
        if !self.supported_models().contains(&model) {
            return Err(Error::UnsupportedModel {
                provider: self,
                model: model.to_owned(),
            });
        }
        Ok(model.to_owned())
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OpenRouter => "openrouter",
            Self::TypeSafe => "typesafe",
        })
    }
}

impl FromStr for Provider {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openrouter" => Ok(Self::OpenRouter),
            "typesafe" => Ok(Self::TypeSafe),
            _ => Err(Error::UnknownProvider),
        }
    }
}
