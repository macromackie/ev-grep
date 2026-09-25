use std::{fmt, str::FromStr};

use anyhow::{Result, bail};
use serde::Serialize;

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

    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenRouter => "typesafe/jev-1.13-20260917",
            Self::TypeSafe => "jev-1.13.0",
        }
    }

    pub fn key_variable(self) -> &'static str {
        match self {
            Self::OpenRouter => "OPENROUTER_API_KEY",
            Self::TypeSafe => "TYPESAFE_API_KEY",
        }
    }

    pub fn model(self, requested: Option<&str>) -> Result<String> {
        let model = requested.unwrap_or(self.default_model());
        if model != self.default_model() {
            bail!(
                "unsupported model {model:?} for {self}; supported model: {}",
                self.default_model()
            );
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
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openrouter" => Ok(Self::OpenRouter),
            "typesafe" => Ok(Self::TypeSafe),
            _ => Err("provider must be openrouter or typesafe".into()),
        }
    }
}
