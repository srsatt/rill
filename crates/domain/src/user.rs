use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "admin" => Some(Self::Admin),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub role: Role,
    pub disabled: bool,
}
