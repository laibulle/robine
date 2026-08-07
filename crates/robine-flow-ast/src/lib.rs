//! Représentation canonique, pure et versionnée de Robine Flow.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DSL_NAME: &str = "robine-flow";
pub const DSL_VERSION: u16 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlowAst {
    pub dsl: String,
    pub dsl_version: u16,
    pub root: Form,
}

impl FlowAst {
    pub fn new(root: Form) -> Result<Self, AstError> {
        validate_root(&root)?;
        Ok(Self {
            dsl: DSL_NAME.into(),
            dsl_version: DSL_VERSION,
            root,
        })
    }

    pub fn validate(&self) -> Result<(), AstError> {
        if self.dsl != DSL_NAME {
            return Err(AstError::UnknownDsl(self.dsl.clone()));
        }
        if self.dsl_version != DSL_VERSION {
            return Err(AstError::UnsupportedVersion(self.dsl_version));
        }
        validate_root(&self.root)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Form {
    List(Vec<Form>),
    Symbol(String),
    Keyword(String),
    String(String),
    Bool(bool),
    Nil,
    Number {
        literal: String,
        unit: Option<String>,
    },
}

impl Form {
    pub fn symbol(&self) -> Option<&str> {
        if let Self::Symbol(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn list(&self) -> Option<&[Form]> {
        if let Self::List(value) = self {
            Some(value)
        } else {
            None
        }
    }
    pub fn is_atom(&self) -> bool {
        !matches!(self, Self::List(_))
    }
}

fn validate_root(root: &Form) -> Result<(), AstError> {
    let Some(forms) = root.list() else {
        return Err(AstError::RootMustBeList);
    };
    if forms.first().and_then(Form::symbol) != Some("flow") {
        return Err(AstError::RootMustStartWithFlow);
    }
    let mut expected = 0_u8;
    let mut trigger = false;
    let mut body = false;
    for section in &forms[1..] {
        let Some(items) = section.list() else {
            return Err(AstError::RootSectionMustBeList);
        };
        let Some(name) = items.first().and_then(Form::symbol) else {
            return Err(AstError::RootSectionNameMissing);
        };
        let position = match name {
            "meta" => 0,
            "inputs" => 1,
            "on" => 2,
            "when" => 3,
            "do" => 4,
            value => return Err(AstError::UnknownRootSection(value.into())),
        };
        if position < expected {
            return Err(AstError::RootSectionOrder(name.into()));
        }
        expected = position + 1;
        trigger |= name == "on";
        body |= name == "do";
    }
    if !trigger {
        return Err(AstError::TriggerMissing);
    }
    if !body {
        return Err(AstError::BodyMissing);
    }
    Ok(())
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AstError {
    #[error("unknown DSL {0}")]
    UnknownDsl(String),
    #[error("unsupported DSL version {0}")]
    UnsupportedVersion(u16),
    #[error("flow root must be a list")]
    RootMustBeList,
    #[error("flow root must start with the symbol flow")]
    RootMustStartWithFlow,
    #[error("flow root section must be a list")]
    RootSectionMustBeList,
    #[error("flow root section has no symbol name")]
    RootSectionNameMissing,
    #[error("unknown flow root section {0}")]
    UnknownRootSection(String),
    #[error("flow root section {0} is out of order or repeated")]
    RootSectionOrder(String),
    #[error("flow trigger on is required")]
    TriggerMissing,
    #[error("flow body do is required")]
    BodyMissing,
}
